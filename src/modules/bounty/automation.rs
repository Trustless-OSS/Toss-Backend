use rust_decimal::Decimal;
use serde_json::json;
use tracing::{info, warn};
use uuid::Uuid;

use crate::{
    error::AppError,
    modules::{
        bounty::repository::{
            get_assignment_for_issue, get_bounty_with_repo, update_assignment_payout_status,
            update_bounty_status,
        },
        escrow::trustless_work::escrow_service::TrustlessWorkAPI,
        github::{
            auth::{fetch_github_issue, post_comment, GitHubPullRequest},
            handlers::helpers::{
                dispute_milestone, explorer_tx_url, extract_issue_number,
                resolve_milestone_dispute, split_amounts,
            },
        },
        notification::{
            kinds::{Kind, Notify},
            service::Service,
        },
    },
    shared::models::{Bounty, Profile, Repo, Wallet},
    state::AppState,
};

#[derive(Debug, Clone)]
pub struct IssueContext {
    pub issue: Bounty,
    pub repo: Repo,
    pub assignee: Option<Profile>,
    pub wallet: Option<Wallet>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    WaitForWallet {
        github_username: String,
    },
    PushMilestone {
        payout_address: String,
        payout_chain: String,
    },
    ReleasePayout {
        milestone_index: i32,
        split_percentage: Option<i32>,
    },
    RepairDatabase {
        milestone_index: i32,
    },
    Settled,
    Waiting {
        reason: String,
    },
    Blocked {
        reason: String,
    },
}

impl Decision {
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

    pub fn park_reason(&self) -> Option<String> {
        match self {
            Self::WaitForWallet { .. } => Some("waiting for a payout wallet".to_string()),
            Self::Waiting { reason } => Some(reason.clone()),
            _ => None,
        }
    }

    pub fn wants_recheck(&self) -> bool {
        self.park_reason().is_some()
    }
}

pub async fn load_context(
    state: &AppState,
    bounty_id: Uuid,
) -> Result<Option<IssueContext>, AppError> {
    let Some((bounty, repo)) = get_bounty_with_repo(state, bounty_id).await? else {
        return Ok(None);
    };

    let (assignee, wallet) = match get_assignment_for_issue(state, bounty.id).await? {
        Some((b, profile)) => {
            let wallet = if let Some(ref p) = profile {
                get_primary_wallet(state, p.id).await?
            } else {
                None
            };
            let _ = b;
            (profile, wallet)
        }
        None => (None, None),
    };

    Ok(Some(IssueContext {
        issue: bounty,
        repo,
        assignee,
        wallet,
    }))
}

async fn get_primary_wallet(
    state: &AppState,
    profile_id: Uuid,
) -> Result<Option<Wallet>, AppError> {
    use crate::{
        error::{map_db_err, require_db},
        shared::models::schema,
    };

    let mut db = require_db(&state.db)?;
    let wallets = schema::Wallet::filter_by_profile_id(profile_id)
        .exec(&mut db)
        .await
        .map_err(map_db_err)?;

    Ok(wallets.into_iter().find(|w| w.is_primary).map(Wallet::from))
}

pub async fn evaluate(state: &AppState, ctx: &IssueContext) -> Result<Decision, AppError> {
    let IssueContext {
        issue,
        repo,
        assignee,
        wallet,
    } = ctx;

    if issue.status == "cancelled" || issue.status == "paid" {
        return Ok(Decision::Settled);
    }

    let Some(assignee) = assignee.as_ref() else {
        return Ok(Decision::Waiting {
            reason: "bounty has no assignee yet".to_string(),
        });
    };

    if issue.reward_amount.unwrap_or(Decimal::ZERO) <= Decimal::ZERO {
        return Ok(Decision::Waiting {
            reason: "bounty reward amount has not been configured yet".to_string(),
        });
    }

    let Some(contract_id) = repo.escrow_contract_id.as_deref() else {
        return Ok(Decision::Blocked {
            reason: "repository has no escrow deployed".to_string(),
        });
    };

    let chain = match issue.milestone_index {
        Some(index) => {
            TrustlessWorkAPI::new(state.clone())
                .fetch_milestone_state(contract_id, index)
                .await?
        }
        None => None,
    };

    if let Some(chain) = chain.as_ref() {
        if chain.released {
            let db_agrees = issue.status == "paid";
            return Ok(if db_agrees {
                Decision::Settled
            } else {
                Decision::RepairDatabase {
                    milestone_index: chain.index,
                }
            });
        }
    }

    if issue.status == "paid" {
        return Ok(Decision::Blocked {
            reason: "database marks the payout as paid but the chain does not".to_string(),
        });
    }

    let payout_address = wallet
        .as_ref()
        .map(|w| w.address.trim())
        .filter(|a| !a.is_empty());

    let Some(payout_address) = payout_address else {
        return Ok(Decision::WaitForWallet {
            github_username: assignee.username.clone(),
        });
    };

    let payout_chain = wallet
        .as_ref()
        .map(|w| w.chain.as_str())
        .unwrap_or("stellar")
        .to_string();

    let Some(chain) = chain else {
        return Ok(Decision::PushMilestone {
            payout_address: payout_address.to_string(),
            payout_chain,
        });
    };

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

    if let Some(amount) = chain.amount {
        if let Some(reward) = issue.reward_amount {
            if amount != reward {
                return Ok(Decision::Blocked {
                    reason: format!(
                        "on-chain milestone amount ({amount}) does not match the issue reward ({reward})"
                    ),
                });
            }
        }
    }

    match confirm_live_merge(state, repo, issue, assignee).await? {
        MergeConfirmation::Merged => {
            info!(
                issue = issue.github_issue_number,
                milestone = chain.index,
                "PR merged and issue closed; authorizing release"
            );
        }
        MergeConfirmation::NotMerged { reason } => {
            return Ok(Decision::Waiting { reason });
        }
        MergeConfirmation::Blocked { reason } => {
            return Ok(Decision::Blocked { reason });
        }
    }

    Ok(Decision::ReleasePayout {
        milestone_index: chain.index,
        split_percentage: None,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum MergeConfirmation {
    Merged,
    NotMerged { reason: String },
    Blocked { reason: String },
}

async fn confirm_live_merge(
    state: &AppState,
    repo: &Repo,
    issue: &Bounty,
    _assignee: &Profile,
) -> Result<MergeConfirmation, AppError> {
    if issue.merged_at.is_none() {
        return Ok(MergeConfirmation::NotMerged {
            reason: "no merged PR recorded yet".to_string(),
        });
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

pub async fn push_milestone(
    state: &AppState,
    ctx: &IssueContext,
    payout_address: &str,
    payout_chain: &str,
) -> Result<i32, AppError> {
    let milestone_index = TrustlessWorkAPI::new(state.clone())
        .push_milestone(&ctx.repo, &ctx.issue, payout_address, payout_chain)
        .await?;

    let contract_id = ctx.repo.escrow_contract_id.as_deref().unwrap_or("");
    let username = ctx
        .assignee
        .as_ref()
        .map(|p| p.username.as_str())
        .unwrap_or("contributor");

    // Send BountyLocked notification to assignee and maintainers
    if let Some(assignee) = &ctx.assignee {
        let n = Notify {
            recipient: assignee.id,
            kind: Kind::BountyLocked,
            title: format!("Bounty Locked: #{}", ctx.issue.github_issue_number),
            body: format!(
                "Your bounty of {} USDC is now locked in escrow",
                ctx.issue.reward_amount.unwrap_or(Decimal::ZERO)
            ),
            ref_id: Some(ctx.issue.id),
            data: serde_json::json!({
                "repoId": ctx.repo.id,
                "issueNumber": ctx.issue.github_issue_number,
                "milestoneIndex": milestone_index,
            }),
            dedupe_key: format!("locked:{}:{}", ctx.issue.id, milestone_index),
            actor: None,
        };
        Service::notify_contributor_quiet(state, n).await;
    }

    let n_maintainers = Notify {
        recipient: ctx.repo.id, // Placeholder; Service::notify_maintainers will set actual recipients
        kind: Kind::BountyLocked,
        title: format!("Bounty Locked: #{}", ctx.issue.github_issue_number),
        body: format!(
            "Bounty of {} USDC locked in escrow at milestone {}",
            ctx.issue.reward_amount.unwrap_or(Decimal::ZERO),
            milestone_index
        ),
        ref_id: Some(ctx.issue.id),
        data: serde_json::json!({
            "repoId": ctx.repo.id,
            "issueNumber": ctx.issue.github_issue_number,
            "milestoneIndex": milestone_index,
        }),
        dedupe_key: format!("locked:{}:{}", ctx.issue.id, milestone_index),
        actor: None,
    };
    Service::notify_maintainers_quiet(state, ctx.repo.id, n_maintainers).await;

    if let Err(error) = post_comment(
        state,
        &ctx.repo.full_name,
        ctx.issue.github_issue_number,
        &format!(
            "### 🔒 Bounty Locked\n\n\
             **{} USDC** is locked in escrow for @{username}.\n\n\
             [View On-Chain →](https://viewer.trustlesswork.com/{contract_id})\n\n\
             Merge the linked PR and the payout releases automatically.",
            ctx.issue.reward_amount.unwrap_or(Decimal::ZERO)
        ),
    )
    .await
    {
        warn!(%error, "failed to comment after pushing the milestone");
    }

    Ok(milestone_index)
}

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
    let tx_hash = TrustlessWorkAPI::new(state.clone())
        .release_milestone(&ctx.repo, &ctx.issue)
        .await?;

    update_assignment_payout_status(state, ctx.issue.id, "released").await?;
    update_bounty_status(state, ctx.issue.id, "paid", None).await?;

    let username = ctx
        .assignee
        .as_ref()
        .map(|p| p.username.as_str())
        .unwrap_or("contributor");
    let contract_id = ctx.repo.escrow_contract_id.as_deref().unwrap_or("");
    let explorer_url = explorer_tx_url(state, &tx_hash, contract_id);
    let reward = ctx.issue.reward_amount.unwrap_or(Decimal::ZERO);

    // Send PayoutReleased notification to assignee
    if let Some(assignee) = &ctx.assignee {
        let n = Notify {
            recipient: assignee.id,
            kind: Kind::PayoutReleased,
            title: format!("Bounty Payout Released: #{}", ctx.issue.github_issue_number),
            body: format!("Your bounty payout of {} USDC has been released", reward),
            ref_id: Some(ctx.issue.id),
            data: serde_json::json!({
                "repoId": ctx.repo.id,
                "issueNumber": ctx.issue.github_issue_number,
                "amount": reward.to_string(),
            }),
            dedupe_key: format!("released:{}:{}", ctx.issue.id, reward),
            actor: None,
        };
        Service::notify_contributor_quiet(state, n).await;
    }

    // Send PayoutReleased notification to maintainers
    let n_maintainers = Notify {
        recipient: ctx.repo.id, // Placeholder; Service::notify_maintainers will set actual recipients
        kind: Kind::PayoutReleased,
        title: format!("Bounty Payout Released: #{}", ctx.issue.github_issue_number),
        body: format!("Bounty payout of {} USDC released to @{}", reward, username),
        ref_id: Some(ctx.issue.id),
        data: serde_json::json!({
            "repoId": ctx.repo.id,
            "issueNumber": ctx.issue.github_issue_number,
            "amount": reward.to_string(),
        }),
        dedupe_key: format!("released:{}:{}", ctx.issue.id, reward),
        actor: None,
    };
    Service::notify_maintainers_quiet(state, ctx.repo.id, n_maintainers).await;

    if let Err(error) = post_comment(
        state,
        &ctx.repo.full_name,
        ctx.issue.github_issue_number,
        &format!(
            "### 🎉 Bounty Released!\n\n\
             **{reward} USDC** has been sent to @{username}.\n\n\
             | Recipient | Amount | Status |\n\
             | :--- | :--- | :--- |\n\
             | @{username} | {reward} USDC | [View Transaction]({explorer_url}) |\n\n\
             Thanks for your contribution! 🚀"
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
    let assignee = ctx
        .assignee
        .as_ref()
        .ok_or_else(|| AppError::internal("split release requires an assignee"))?;

    let wallet = ctx
        .wallet
        .as_ref()
        .ok_or_else(|| AppError::internal("split release requires a wallet"))?;

    let contract_id = ctx
        .repo
        .escrow_contract_id
        .as_deref()
        .ok_or_else(|| AppError::bad_request("No escrow deployed"))?;

    let contributor_wallet = wallet.address.as_str();
    let reward = ctx.issue.reward_amount.unwrap_or(Decimal::ZERO);

    let maintainer_wallet = {
        use crate::{
            error::{map_db_err, require_db},
            shared::models::schema,
        };
        let mut db = require_db(&state.db)?;
        let wallets = schema::Wallet::filter_by_profile_id(assignee.id)
            .exec(&mut db)
            .await
            .map_err(map_db_err)?;
        wallets
            .into_iter()
            .find(|w| w.chain == "stellar")
            .map(|w| w.address)
    };

    let Some(maintainer_wallet) = maintainer_wallet else {
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

    let (contributor_amount, maintainer_amount) = split_amounts(reward, percentage);

    dispute_milestone(
        state,
        contract_id,
        milestone_index,
        &state.config.platform_stellar_public_key,
    )
    .await?;

    let distributions = if contributor_wallet == maintainer_wallet {
        vec![json!({ "address": maintainer_wallet, "amount": reward })]
    } else {
        vec![
            json!({ "address": contributor_wallet, "amount": contributor_amount }),
            json!({ "address": maintainer_wallet, "amount": maintainer_amount }),
        ]
    };

    resolve_milestone_dispute(state, &ctx.repo, milestone_index, distributions).await?;

    update_assignment_payout_status(state, ctx.issue.id, "released").await?;
    update_bounty_status(state, ctx.issue.id, "paid", None).await?;

    // Send PayoutReleased notification to assignee
    let n = Notify {
        recipient: assignee.id,
        kind: Kind::PayoutReleased,
        title: format!("Bounty Payout Released: #{}", ctx.issue.github_issue_number),
        body: format!(
            "Your portion ({percentage}%) of the bounty payout of {} USDC has been released",
            contributor_amount
        ),
        ref_id: Some(ctx.issue.id),
        data: serde_json::json!({
            "repoId": ctx.repo.id,
            "issueNumber": ctx.issue.github_issue_number,
            "amount": contributor_amount.to_string(),
        }),
        dedupe_key: format!("released:{}:{}:{}", ctx.issue.id, reward, percentage),
        actor: None,
    };
    Service::notify_contributor_quiet(state, n).await;

    // Send PayoutReleased notification to maintainers
    let n_maintainers = Notify {
        recipient: ctx.repo.id, // Placeholder; Service::notify_maintainers will set actual recipients
        kind: Kind::PayoutReleased,
        title: format!("Bounty Payout Released: #{}", ctx.issue.github_issue_number),
        body: format!(
            "Split bounty payout released: contributor gets {} USDC, maintainer gets {} USDC",
            contributor_amount, maintainer_amount
        ),
        ref_id: Some(ctx.issue.id),
        data: serde_json::json!({
            "repoId": ctx.repo.id,
            "issueNumber": ctx.issue.github_issue_number,
            "amount": reward.to_string(),
        }),
        dedupe_key: format!("released:{}:{}:{}", ctx.issue.id, reward, percentage),
        actor: None,
    };
    Service::notify_maintainers_quiet(state, ctx.repo.id, n_maintainers).await;

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
            assignee.username
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

pub async fn repair_database(
    state: &AppState,
    ctx: &IssueContext,
    milestone_index: i32,
) -> Result<(), AppError> {
    if ctx.issue.status != "paid" {
        update_assignment_payout_status(state, ctx.issue.id, "released").await?;
    }

    if ctx.issue.status != "paid" {
        update_bounty_status(state, ctx.issue.id, "paid", None).await?;
    }

    warn!(
        issue = ctx.issue.github_issue_number,
        milestone_index, "milestone was already released on-chain; repaired Postgres only"
    );

    Ok(())
}

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
            ctx.issue.reward_amount.unwrap_or(Decimal::ZERO)
        ),
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

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
