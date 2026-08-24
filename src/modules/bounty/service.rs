use axum::{
    extract::{Path, State},
    Json,
};
use tracing::info;
use uuid::Uuid;

use crate::{
    error::AppError,
    infra::queue::{BountyJobData, EnqueueOutcome},
    middleware::auth::AuthedUser,
    modules::{
        bounty::{
            automation,
            model::{Milestone, MilestoneResponse, RetryIssueResponse},
            repository::{
                get_issue_by_repo_and_github_id, get_issue_with_repo, is_assigned_contributor,
            },
        },
        contributor::repository::upsert_contributor_wallet,
        repo::repository::{get_repo_by_github_id, is_maintainer},
    },
    state::AppState,
};

#[derive(Debug, Default, Clone)]
pub struct BountyService;

impl BountyService {
    /// Save the contributor's payout wallet and let the automation take over.
    ///
    /// The on-chain milestone is pushed by the `push-milestone` worker, which
    /// re-checks the rules first. When no queue is available the push happens
    /// inline so the endpoint keeps working in a degraded deployment.
    pub async fn push_milestone(
        State(state): State<AppState>,
        user: AuthedUser,
        Json(body): Json<Milestone>,
    ) -> Result<Json<MilestoneResponse>, AppError> {
        let repo = get_repo_by_github_id(&state, body.github_repo_id)
            .await?
            .ok_or_else(|| AppError::not_found("Repo not found"))?;

        if !is_assigned_contributor(&state, user.github_id, repo.id, body.github_issue_id).await? {
            return Err(AppError::forbidden(
                "Forbidden: Only the assigned contributor can connect their wallet for this issue",
            ));
        }

        let payout_chain = body
            .payout_chain
            .as_deref()
            .unwrap_or("stellar")
            .to_string();
        let payout_address = body
            .payout_address
            .as_deref()
            .unwrap_or(&body.wallet)
            .to_string();

        upsert_contributor_wallet(
            &state,
            user.github_id,
            user.github_username.as_deref().unwrap_or(""),
            &payout_chain,
            &payout_address,
        )
        .await?;

        let issue = get_issue_by_repo_and_github_id(&state, repo.id, body.github_issue_id)
            .await?
            .ok_or_else(|| {
                AppError::bad_request("Issue is not in a valid state to connect wallet")
            })?;

        if issue.status != "pending" && issue.status != "active" {
            return Err(AppError::bad_request(
                "Issue is not in a valid state to connect wallet",
            ));
        }

        let outcome = state
            .queue
            .enqueue_advance_issue(BountyJobData::new(issue.id, "milestone-push-requested"))
            .await?;

        if outcome == EnqueueOutcome::Unavailable {
            advance_inline(&state, issue.id).await?;
        }

        info!(
            issue = issue.github_issue_number,
            outcome = outcome.label(),
            "wallet connected; bounty automation queued"
        );

        Ok(Json(MilestoneResponse {
            ok: true,
            repo_full_name: repo.full_name,
            issue_number: issue.github_issue_number,
        }))
    }

    /// Emergency maintainer override.
    ///
    /// The documented happy path never needs this: every step advances by itself
    /// and transient failures retry with backoff. It stays as an admin escape
    /// hatch, and it does nothing more than ask the state machine to run now —
    /// the same rules still gate any movement of funds.
    pub async fn retry_issue(
        State(state): State<AppState>,
        user: AuthedUser,
        Path(issue_id): Path<Uuid>,
    ) -> Result<Json<RetryIssueResponse>, AppError> {
        let (issue, repo) = get_issue_with_repo(&state, issue_id)
            .await?
            .ok_or_else(|| AppError::not_found("Issue not found"))?;

        if !is_maintainer(&state, user.github_id, repo.id).await? {
            return Err(AppError::forbidden(
                "Forbidden: Only the repository owner or maintainer can retry",
            ));
        }

        let outcome = state
            .queue
            .enqueue_advance_issue(BountyJobData::new(issue.id, "manual-retry"))
            .await?;

        if outcome == EnqueueOutcome::Unavailable {
            advance_inline(&state, issue.id).await?;
            return Ok(Json(RetryIssueResponse {
                ok: true,
                step: Some("applied"),
                status: None,
                tx_hash: None,
                message: Some("Queue unavailable; the next step was applied inline"),
            }));
        }

        info!(
            issue = issue.github_issue_number,
            outcome = outcome.label(),
            "manual retry requested"
        );

        Ok(Json(RetryIssueResponse {
            ok: true,
            step: Some("queued"),
            status: None,
            tx_hash: None,
            message: Some("The bounty state machine will run shortly"),
        }))
    }
}

/// Run one state-machine step without a queue.
///
/// Only reached when Redis is unreachable. The rules are identical to the ones
/// the workers apply, so this cannot pay out anything the queue would not.
async fn advance_inline(state: &AppState, issue_id: Uuid) -> Result<(), AppError> {
    use automation::Decision;

    let Some(ctx) = automation::load_context(state, issue_id).await? else {
        return Ok(());
    };

    match automation::evaluate(state, &ctx).await? {
        Decision::PushMilestone {
            payout_address,
            payout_chain,
        } => {
            automation::push_milestone(state, &ctx, &payout_address, &payout_chain).await?;
        }
        Decision::ReleasePayout {
            milestone_index,
            split_percentage,
        } => {
            automation::release_payout(state, &ctx, milestone_index, split_percentage).await?;
        }
        Decision::RepairDatabase { milestone_index } => {
            automation::repair_database(state, &ctx, milestone_index).await?;
        }
        decision => {
            info!(
                issue = ctx.issue.github_issue_number,
                decision = decision.label(),
                "inline advance had nothing to do"
            );
        }
    }

    Ok(())
}
