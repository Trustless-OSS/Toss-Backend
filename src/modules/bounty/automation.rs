//! The bounty issue state machine.
//!
//! This module owns the rules that decide whether a bounty may move forward and
//! the actions that move it. It is deliberately free of any queue concerns: the
//! `advance-issue`, `push-milestone` and `release-payout` workers in
//! [`crate::infra::jobs`] call into it, and it never imports `bullmq`.
//!
//! Two invariants drive everything here:
//!
//! 1. **Rules are re-checked live.** Every decision re-reads Postgres, GitHub and
//!    the escrow contract. A job that was enqueued minutes ago never acts on the
//!    state that existed when it was created.
//! 2. **Money moves only forward, once.** If the chain already shows the
//!    milestone released, the only permitted action is repairing Postgres. A
//!    receiver or amount that does not match the contributor's wallet blocks the
//!    payout instead of retrying it.

use rust_decimal::{prelude::ToPrimitive, Decimal};
use serde_json::json;
use tracing::{info, warn};
use uuid::Uuid;

use crate::{
    error::AppError,
    modules::{
        bounty::repository::{
            get_assignment_for_issue, get_issue_with_repo, update_assignment_payout_status,
            update_issue_status,
        },
        contributor::repository::get_contributor_by_github_id,
        escrow::service::{
            fetch_milestone_state, push_milestone_on_chain, release_escrow_milestone,
        },
        github::{
            auth::{
                fetch_github_issue, fetch_github_pull_request, post_comment, GitHubPullRequest,
            },
            handlers::helpers::{
                dispute_milestone, explorer_tx_url, extract_issue_number, maintainer_github_id,
                resolve_milestone_dispute, split_amounts,
            },
        },
    },
    shared::models::{Assignment, Contributor, Issue, Repo},
    state::AppState,
};

/// Everything the rules need about one issue, read in a single pass.
#[derive(Debug, Clone)]
pub struct IssueContext {
    pub issue: Issue,
    pub repo: Repo,
    pub assignment: Option<Assignment>,
    pub contributor: Option<Contributor>,
}

/// The next step the automation is allowed to take for an issue.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    /// The contributor has not connected a payout wallet yet. Not a failure —
    /// the flow resumes on its own once the wallet arrives.
    WaitForWallet { github_username: String },
    /// The wallet is known and the milestone is not on-chain yet.
    PushMilestone {
        payout_address: String,
        payout_chain: String,
    },
    /// Every live rule passed; the milestone may be paid out.
    ReleasePayout {
        milestone_index: i32,
        split_percentage: Option<i32>,
    },
    /// The chain already released this milestone but Postgres still says
    /// otherwise. Repair the database; never send funds again.
    RepairDatabase { milestone_index: i32 },
    /// Nothing left to do for this issue.
    Settled,
    /// Waiting on an external event that is not the wallet (issue still open,
    /// nobody assigned yet, …).
    Waiting { reason: String },
    /// A live rule failed in a way that must never be retried into a payment.
    Blocked { reason: String },
}

impl Decision {
    /// A short, stable label for logs and job return values.
    pub fn label(&self) -> &'static str {
        match self {
            Self::WaitForWallet { .. } => "wait-for-wallet",
            Self::PushMilestone { .. } => "push-milestone",
            Self::ReleasePayout { .. } => "release-payout",
            Self::RepairDatabase { .. } => "repair-database",
            Self::Settled => "settled",
            Self::Waiting { .. } => "waiting",
            Self::Blocked { .. } => "blocked",
        }
    }

    /// Why the flow is parked, if it is.
    ///
    /// Only states that resolve through an *external* change are worth
    /// revisiting on a timer; terminal and blocked states are not.
    pub fn park_reason(&self) -> Option<String> {
        match self {
            Self::WaitForWallet { .. } => Some("waiting for a payout wallet".to_string()),
            Self::Waiting { reason } => Some(reason.clone()),
            _ => None,
        }
    }

    /// Whether the automation should revisit this issue on a timer.
    pub fn wants_recheck(&self) -> bool {
        self.park_reason().is_some()
    }
}

/// Load every record the rules depend on for one issue.
///
/// Returns `Ok(None)` when the issue (or its repository) no longer exists, which
/// makes a stale job a no-op rather than a failure.
pub async fn load_context(
    state: &AppState,
    issue_id: Uuid,
) -> Result<Option<IssueContext>, AppError> {
    let Some((issue, repo)) = get_issue_with_repo(state, issue_id).await? else {
        return Ok(None);
    };

    let (assignment, contributor) = match get_assignment_for_issue(state, issue.id).await? {
        Some((assignment, contributor)) => (Some(assignment), contributor),
        None => (None, None),
    };

    Ok(Some(IssueContext {
        issue,
        repo,
        assignment,
        contributor,
    }))
}

/// Decide what the automation may do next for this issue.
///
/// This re-reads the escrow contract and (when the payout gate is reached)
/// GitHub, so the answer reflects the world as it is right now. Transient
/// failures propagate as `Err` so the caller's queue can retry them with
/// backoff.
pub async fn evaluate(state: &AppState, ctx: &IssueContext) -> Result<Decision, AppError> {
    let IssueContext {
        issue,
        repo,
        assignment,
        contributor,
    } = ctx;

    if issue.status == "cancelled" {
        return Ok(Decision::Settled);
    }

    let Some(assignment) = assignment.as_ref() else {
        return Ok(Decision::Waiting {
            reason: "issue has no assignment yet".to_string(),
        });
    };

    let Some(contributor) = contributor.as_ref() else {
        return Ok(Decision::Waiting {
            reason: "assignment has no contributor record yet".to_string(),
        });
    };

    let Some(contract_id) = repo.escrow_contract_id.as_deref() else {
        return Ok(Decision::Blocked {
            reason: "repository has no escrow deployed".to_string(),
        });
    };

    // ── Chain truth first ────────────────────────────────────────────────────
    // Reading the milestone before anything else means an already-paid issue can
    // never fall through into a second payout, no matter what Postgres says.
    let chain = match issue.milestone_index {
        Some(index) => fetch_milestone_state(state, contract_id, index).await?,
        None => None,
    };

    if let Some(chain) = chain.as_ref() {
        if chain.released {
            let db_agrees = issue.status == "completed" && assignment.payout_status == "released";
            return Ok(if db_agrees {
                Decision::Settled
            } else {
                Decision::RepairDatabase {
                    milestone_index: chain.index,
                }
            });
        }
    }

    if assignment.payout_status == "released" {
        // Postgres claims the money left but the chain disagrees. Paying again to
        // "fix" this is exactly the failure mode we must never have, so stop and
        // surface it for a human instead.
        return Ok(Decision::Blocked {
            reason: "database marks the payout as released but the chain does not".to_string(),
        });
    }

    // ── Wallet gate ──────────────────────────────────────────────────────────
    let payout_address = contributor
        .payout_address
        .as_deref()
        .or(contributor.stellar_wallet.as_deref())
        .map(str::trim)
        .filter(|address| !address.is_empty());

    let Some(payout_address) = payout_address else {
        return Ok(Decision::WaitForWallet {
            github_username: contributor.github_username.clone(),
        });
    };
    let payout_chain = contributor
        .payout_chain
        .as_deref()
        .unwrap_or("stellar")
        .to_string();

    // ── Milestone gate ───────────────────────────────────────────────────────
    let Some(chain) = chain else {
        return Ok(Decision::PushMilestone {
            payout_address: payout_address.to_string(),
            payout_chain,
        });
    };

    // ── Receiver / amount must match the contributor's wallet ────────────────
    // A receiver mismatch means the milestone on-chain is stale — the contributor
    // changed wallets since it was pushed. Paying the stale address would send
    // money to the wrong place, so re-point the milestone instead. This moves no
    // funds; it only rewrites the milestone to the contributor's current wallet,
    // exactly as the original push did.
    if chain.receiver.as_deref() != Some(payout_address) {
        warn!(
            issue = issue.github_issue_number,
            on_chain = chain.receiver.as_deref().unwrap_or("unset"),
            "on-chain milestone receiver is stale; re-pushing before any payout"
        );
        return Ok(Decision::PushMilestone {
            payout_address: payout_address.to_string(),
            payout_chain,
        });
    }

    // An amount mismatch is different: rewards only change while an issue is
    // pending, so seeing one here means something moved that should not have.
    // Stop and let a human look rather than paying either figure.
    if let Some(amount) = chain.amount {
        if amount != issue.reward_amount {
            return Ok(Decision::Blocked {
                reason: format!(
                    "on-chain milestone amount ({amount}) does not match the issue reward ({})",
                    issue.reward_amount
                ),
            });
        }
    }

    // ── Completion gate: live GitHub re-read of the merged PR ────────────────
    match confirm_live_merge(state, repo, issue, assignment, Some(contributor)).await? {
        MergeConfirmation::Merged => {}
        MergeConfirmation::NotMerged { reason } => {
            return Ok(Decision::Waiting { reason });
        }
        MergeConfirmation::Blocked { reason } => {
            return Ok(Decision::Blocked { reason });
        }
    }

    let split_percentage = assignment
        .completion_percentage
        .filter(|value| *value > Decimal::ZERO && *value < Decimal::from(100))
        .and_then(|value| value.to_i32());

    Ok(Decision::ReleasePayout {
        milestone_index: chain.index,
        split_percentage,
    })
}

/// Whether GitHub currently reports the assigned contributor's PR as merged
/// and the bounty issue as closed.
///
/// The webhook records `pr_number`; payout still re-fetches the PR with the
/// GitHub App so a closed-without-merge issue, a later unlink, or an author
/// mismatch cannot release funds.
#[derive(Debug, Clone, PartialEq, Eq)]
enum MergeConfirmation {
    Merged,
    NotMerged { reason: String },
    Blocked { reason: String },
}

async fn confirm_live_merge(
    state: &AppState,
    repo: &Repo,
    issue: &Issue,
    assignment: &Assignment,
    contributor: Option<&Contributor>,
) -> Result<MergeConfirmation, AppError> {
    let Some(pr_number) = assignment.pr_number else {
        return Ok(MergeConfirmation::NotMerged {
            reason: "no merged PR recorded yet".to_string(),
        });
    };

    let pr =
        fetch_github_pull_request(state, repo.github_repo_id, &repo.full_name, pr_number).await?;
    let assigned_github_id = contributor.map(|value| value.github_user_id);
    let confirmation =
        confirm_payout_pull_request(&pr, issue.github_issue_number, assigned_github_id);
    if confirmation != MergeConfirmation::Merged {
        return Ok(confirmation);
    }

    let github_issue = fetch_github_issue(
        state,
        repo.github_repo_id,
        &repo.full_name,
        issue.github_issue_number,
    )
    .await?;
    if !issue_is_closed(&github_issue.state) {
        return Ok(MergeConfirmation::NotMerged {
            reason: format!(
                "issue #{} is not closed on GitHub",
                issue.github_issue_number
            ),
        });
    }

    Ok(MergeConfirmation::Merged)
}

fn issue_is_closed(state: &str) -> bool {
    state.eq_ignore_ascii_case("closed")
}

fn confirm_payout_pull_request(
    pr: &GitHubPullRequest,
    issue_number: i32,
    assigned_github_id: Option<i64>,
) -> MergeConfirmation {
    if !pr.merged {
        return MergeConfirmation::NotMerged {
            reason: format!("PR #{} is not merged on GitHub", pr.number),
        };
    }

    if extract_issue_number(pr.body.as_deref()) != Some(issue_number) {
        return MergeConfirmation::Blocked {
            reason: format!(
                "PR #{} no longer references issue #{issue_number}",
                pr.number
            ),
        };
    }

    if let Some(expected) = assigned_github_id {
        if pr.user.id != expected {
            return MergeConfirmation::Blocked {
                reason: "PR author does not match the assigned contributor".to_string(),
            };
        }
    }

    MergeConfirmation::Merged
}

// ── Actions ──────────────────────────────────────────────────────────────────

/// Push the milestone on-chain and lock the reward for the contributor.
pub async fn push_milestone(
    state: &AppState,
    ctx: &IssueContext,
    payout_address: &str,
    payout_chain: &str,
) -> Result<i32, AppError> {
    let milestone_index =
        push_milestone_on_chain(state, &ctx.repo, &ctx.issue, payout_address, payout_chain).await?;

    let contract_id = ctx.repo.escrow_contract_id.as_deref().unwrap_or("");
    let username = ctx
        .contributor
        .as_ref()
        .map(|contributor| contributor.github_username.as_str())
        .unwrap_or("contributor");

    if let Err(error) = post_comment(
        state,
        &ctx.repo.full_name,
        ctx.issue.github_issue_number,
        &format!(
            "### 🔒 Bounty Locked\n\n\
             **{} USDC** is locked in escrow for @{username}.\n\n\
             [View On-Chain →](https://viewer.trustlesswork.com/{contract_id})\n\n\
             Merge the linked PR and the payout releases automatically.",
            ctx.issue.reward_amount
        ),
    )
    .await
    {
        // A failed comment must not roll back an on-chain milestone.
        warn!(%error, "failed to comment after pushing the milestone");
    }

    Ok(milestone_index)
}

/// Release the bounty, then record the payout in Postgres.
///
/// The caller must have re-evaluated the rules immediately before this call.
pub async fn release_payout(
    state: &AppState,
    ctx: &IssueContext,
    milestone_index: i32,
    split_percentage: Option<i32>,
) -> Result<(), AppError> {
    match split_percentage {
        Some(percentage) => release_split(state, ctx, milestone_index, percentage).await,
        None => release_full(state, ctx).await,
    }
}

async fn release_full(state: &AppState, ctx: &IssueContext) -> Result<(), AppError> {
    let assignment = ctx
        .assignment
        .as_ref()
        .ok_or_else(|| AppError::internal("release requires an assignment"))?;

    let tx_hash = release_escrow_milestone(state, &ctx.repo, &ctx.issue).await?;

    update_assignment_payout_status(state, assignment.id, "released").await?;
    update_issue_status(state, ctx.issue.id, "completed", None).await?;

    let username = ctx
        .contributor
        .as_ref()
        .map(|contributor| contributor.github_username.as_str())
        .unwrap_or("contributor");
    let contract_id = ctx.repo.escrow_contract_id.as_deref().unwrap_or("");
    let explorer_url = explorer_tx_url(state, &tx_hash, contract_id);

    if let Err(error) = post_comment(
        state,
        &ctx.repo.full_name,
        ctx.issue.github_issue_number,
        &format!(
            "### 🎉 Bounty Released!\n\n\
             **{} USDC** has been sent to @{username}.\n\n\
             | Recipient | Amount | Status |\n\
             | :--- | :--- | :--- |\n\
             | @{username} | {} USDC | [View Transaction]({explorer_url}) |\n\n\
             Thanks for your contribution! 🚀",
            ctx.issue.reward_amount, ctx.issue.reward_amount
        ),
    )
    .await
    {
        warn!(%error, "failed to comment after releasing the bounty");
    }

    info!(
        issue = ctx.issue.github_issue_number,
        %tx_hash,
        "bounty released automatically"
    );

    Ok(())
}

async fn release_split(
    state: &AppState,
    ctx: &IssueContext,
    milestone_index: i32,
    percentage: i32,
) -> Result<(), AppError> {
    let assignment = ctx
        .assignment
        .as_ref()
        .ok_or_else(|| AppError::internal("split release requires an assignment"))?;
    let contributor = ctx
        .contributor
        .as_ref()
        .ok_or_else(|| AppError::internal("split release requires a contributor"))?;

    let contract_id = ctx
        .repo
        .escrow_contract_id
        .as_deref()
        .ok_or_else(|| AppError::bad_request("No escrow deployed"))?;

    let Some(contributor_wallet) = contributor.stellar_wallet.as_deref() else {
        return Err(AppError::bad_request(
            "Split payout requires the contributor's Stellar wallet",
        ));
    };

    let maintainer = get_contributor_by_github_id(state, maintainer_github_id(&ctx.repo)).await?;
    let Some(maintainer_wallet) = maintainer.and_then(|value| value.stellar_wallet) else {
        // The maintainer half has nowhere to go; this is a human step, not a
        // transient failure, so report it and stop rather than retrying.
        post_comment(
            state,
            &ctx.repo.full_name,
            ctx.issue.github_issue_number,
            &format!(
                "⚠️ Partial payment is waiting for the maintainer to connect a Stellar wallet. [Connect here →]({}/connect)",
                state.config.app_url
            ),
        )
        .await?;
        return Ok(());
    };

    let (contributor_amount, maintainer_amount) =
        split_amounts(ctx.issue.reward_amount, percentage);

    dispute_milestone(
        state,
        contract_id,
        milestone_index,
        &state.config.platform_stellar_public_key,
    )
    .await?;

    let distributions = if contributor_wallet == maintainer_wallet {
        vec![json!({ "address": maintainer_wallet, "amount": ctx.issue.reward_amount })]
    } else {
        vec![
            json!({ "address": contributor_wallet, "amount": contributor_amount }),
            json!({ "address": maintainer_wallet, "amount": maintainer_amount }),
        ]
    };

    resolve_milestone_dispute(state, &ctx.repo, milestone_index, distributions).await?;

    update_assignment_payout_status(state, assignment.id, "released").await?;
    update_issue_status(state, ctx.issue.id, "completed", None).await?;

    if let Err(error) = post_comment(
        state,
        &ctx.repo.full_name,
        ctx.issue.github_issue_number,
        &format!(
            "### ✅ Payout Released ({percentage}%)\n\n\
             | Recipient | Amount | Role |\n\
             | :--- | :--- | :--- |\n\
             | @{} | **{contributor_amount} USDC** | Contributor |\n\
             | Maintainer | **{maintainer_amount} USDC** | Refund |\n\n\
             [View Escrow](https://viewer.trustlesswork.com/{contract_id})",
            contributor.github_username
        ),
    )
    .await
    {
        warn!(%error, "failed to comment after releasing the split payout");
    }

    info!(
        issue = ctx.issue.github_issue_number,
        percentage, "split bounty released automatically"
    );

    Ok(())
}

/// Bring Postgres in line with a chain that already released the milestone.
///
/// This moves no funds — it only repairs bookkeeping that drifted, which is the
/// one action allowed once the chain says "released".
pub async fn repair_database(
    state: &AppState,
    ctx: &IssueContext,
    milestone_index: i32,
) -> Result<(), AppError> {
    if let Some(assignment) = ctx.assignment.as_ref() {
        if assignment.payout_status != "released" {
            update_assignment_payout_status(state, assignment.id, "released").await?;
        }
    }

    if ctx.issue.status != "completed" {
        update_issue_status(state, ctx.issue.id, "completed", None).await?;
    }

    warn!(
        issue = ctx.issue.github_issue_number,
        milestone_index, "milestone was already released on-chain; repaired Postgres only"
    );

    Ok(())
}

/// Tell the contributor the payout is parked on their wallet.
///
/// Replaces the old "run `@Trustless-OSS /retry`" instruction: nothing needs to
/// be retried, the flow resumes by itself once the wallet is connected.
pub async fn notify_waiting_for_wallet(
    state: &AppState,
    ctx: &IssueContext,
    github_username: &str,
) -> Result<(), AppError> {
    let connect_url = format!(
        "{}/connect?issue={}&repo={}",
        state.config.app_url, ctx.issue.github_issue_id, ctx.repo.github_repo_id
    );

    post_comment(
        state,
        &ctx.repo.full_name,
        ctx.issue.github_issue_number,
        &format!(
            "### 🔑 Wallet Required\n\n\
             The **{} USDC** payout for @{github_username} is ready and waiting on a wallet.\n\n\
             [**Connect Wallet →**]({connect_url})\n\n\
             The payout releases automatically once the wallet is connected — no further action needed.",
            ctx.issue.reward_amount
        ),
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decision_from_percentage(percentage: Option<Decimal>) -> Option<i32> {
        percentage
            .filter(|value| *value > Decimal::ZERO && *value < Decimal::from(100))
            .and_then(|value| value.to_i32())
    }

    #[test]
    fn only_partial_completion_selects_a_split_payout() {
        assert_eq!(decision_from_percentage(None), None);
        assert_eq!(decision_from_percentage(Some(Decimal::ZERO)), None);
        assert_eq!(decision_from_percentage(Some(Decimal::from(100))), None);
        assert_eq!(decision_from_percentage(Some(Decimal::from(75))), Some(75));
    }

    #[test]
    fn only_externally_blocked_states_ask_for_a_recheck() {
        assert!(Decision::WaitForWallet {
            github_username: "octocat".to_string()
        }
        .wants_recheck());
        assert!(Decision::Waiting {
            reason: "still open".to_string()
        }
        .wants_recheck());

        assert!(!Decision::Settled.wants_recheck());
        assert!(!Decision::Blocked {
            reason: "receiver mismatch".to_string()
        }
        .wants_recheck());
        assert!(!Decision::ReleasePayout {
            milestone_index: 0,
            split_percentage: None
        }
        .wants_recheck());
    }

    fn pull(merged: bool, body: &str, user_id: i64) -> GitHubPullRequest {
        GitHubPullRequest {
            number: 12,
            state: if merged { "closed" } else { "open" }.to_string(),
            merged,
            body: Some(body.to_string()),
            user: crate::modules::github::auth::GitHubPullUser { id: user_id },
        }
    }

    #[test]
    fn payout_requires_a_live_merged_pr_linked_to_the_issue() {
        assert_eq!(
            confirm_payout_pull_request(&pull(true, "Closes #7", 42), 7, Some(42)),
            MergeConfirmation::Merged
        );
    }

    #[test]
    fn a_closed_unmerged_pr_does_not_release_the_payout() {
        assert!(matches!(
            confirm_payout_pull_request(&pull(false, "Closes #7", 42), 7, Some(42)),
            MergeConfirmation::NotMerged { .. }
        ));
    }

    #[test]
    fn a_merged_pr_for_a_different_issue_is_blocked() {
        assert!(matches!(
            confirm_payout_pull_request(&pull(true, "Closes #99", 42), 7, Some(42)),
            MergeConfirmation::Blocked { .. }
        ));
    }

    #[test]
    fn a_merged_pr_by_someone_else_is_blocked() {
        assert!(matches!(
            confirm_payout_pull_request(&pull(true, "Fixes #7", 99), 7, Some(42)),
            MergeConfirmation::Blocked { .. }
        ));
    }

    #[test]
    fn an_open_issue_does_not_count_as_closed() {
        assert!(!issue_is_closed("open"));
        assert!(issue_is_closed("closed"));
        assert!(issue_is_closed("Closed"));
    }
}
