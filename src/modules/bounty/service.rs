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
            model::{Milestone, MilestoneResponse, RejectIssueResponse, RetryIssueResponse},
            repository::{
                cancel_bounty_with_refund, get_assignment_for_issue,
                get_bounty_by_repo_and_github_id, get_bounty_with_repo, is_assigned_contributor,
            },
        },
        github::handlers::helpers::{hold_milestone_on_chain, reject_milestone_on_chain},
        notification::{
            kinds::{Kind, Notify},
            service::Service,
        },
        repo::repository::{get_repo_by_github_id, is_maintainer},
    },
    state::AppState,
};

#[derive(Debug, Default, Clone)]
pub struct BountyService;

impl BountyService {
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
            .trim()
            .to_ascii_lowercase();
        let payout_address = body
            .payout_address
            .as_deref()
            .unwrap_or(&body.wallet)
            .trim()
            .to_string();

        if payout_chain == "stellar" {
            crate::infra::stellar::accounts::require_usdc_payout_account(&state, &payout_address)
                .await?;
        }

        upsert_profile_wallet(
            &state,
            user.github_id,
            user.github_username.as_deref().unwrap_or(""),
            &payout_chain,
            &payout_address,
        )
        .await?;

        let bounty = get_bounty_by_repo_and_github_id(&state, repo.id, body.github_issue_id)
            .await?
            .ok_or_else(|| {
                AppError::bad_request("Issue is not in a valid state to connect wallet")
            })?;

        if !matches!(bounty.status.as_str(), "open" | "assigned" | "merged") {
            return Err(AppError::bad_request(
                "Issue is not in a valid state to connect wallet",
            ));
        }

        let outcome = state
            .queue
            .enqueue_advance_issue(BountyJobData::new(bounty.id, "milestone-push-requested"))
            .await?;

        if outcome == EnqueueOutcome::Unavailable {
            advance_inline(&state, bounty.id).await?;
        }

        info!(
            issue = bounty.github_issue_number,
            outcome = outcome.label(),
            "wallet connected; bounty automation queued"
        );

        Ok(Json(MilestoneResponse {
            ok: true,
            repo_full_name: repo.full_name,
            issue_number: bounty.github_issue_number,
        }))
    }

    pub async fn retry_issue(
        State(state): State<AppState>,
        user: AuthedUser,
        Path(bounty_id): Path<Uuid>,
    ) -> Result<Json<RetryIssueResponse>, AppError> {
        let (bounty, repo) = get_bounty_with_repo(&state, bounty_id)
            .await?
            .ok_or_else(|| AppError::not_found("Issue not found"))?;

        if !is_maintainer(&state, user.github_id, repo.id).await? {
            return Err(AppError::forbidden(
                "Forbidden: Only the repository owner or maintainer can retry",
            ));
        }

        let outcome = state
            .queue
            .enqueue_advance_issue(BountyJobData::new(bounty.id, "manual-retry"))
            .await?;

        if outcome == EnqueueOutcome::Unavailable {
            advance_inline(&state, bounty.id).await?;
            return Ok(Json(RetryIssueResponse {
                ok: true,
                step: Some("applied"),
                status: None,
                tx_hash: None,
                message: Some("Queue unavailable; the next step was applied inline"),
            }));
        }

        info!(
            issue = bounty.github_issue_number,
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

    pub async fn reject_issue(
        State(state): State<AppState>,
        user: AuthedUser,
        Path(bounty_id): Path<Uuid>,
    ) -> Result<Json<RejectIssueResponse>, AppError> {
        let (bounty, repo) = get_bounty_with_repo(&state, bounty_id)
            .await?
            .ok_or_else(|| AppError::not_found("Issue not found"))?;

        if !is_maintainer(&state, user.github_id, repo.id).await? {
            return Err(AppError::forbidden(
                "Forbidden: Only the repository owner or maintainer can reject bounties",
            ));
        }

        if bounty.status == "paid" || bounty.status == "cancelled" {
            return Err(AppError::bad_request(
                "Bounty has already been paid or cancelled",
            ));
        }

        let refunded_amount = reject_bounty_with_pool_refund(&state, &bounty, &repo).await?;

        Ok(Json(RejectIssueResponse {
            ok: true,
            refunded_amount: refunded_amount.to_string(),
        }))
    }

    pub async fn hold_issue(
        State(state): State<AppState>,
        user: AuthedUser,
        Path(bounty_id): Path<Uuid>,
    ) -> Result<Json<crate::modules::bounty::model::HoldIssueResponse>, AppError> {
        let (bounty, repo) = get_bounty_with_repo(&state, bounty_id)
            .await?
            .ok_or_else(|| AppError::not_found("Issue not found"))?;
        if !is_maintainer(&state, user.github_id, repo.id).await? {
            return Err(AppError::forbidden(
                "Forbidden: Only the repository owner or maintainer can hold bounties",
            ));
        }
        if bounty.status == "paid" || bounty.status == "cancelled" {
            return Err(AppError::bad_request(
                "Bounty has already been paid or cancelled",
            ));
        }
        let milestone_index = bounty
            .milestone_index
            .ok_or_else(|| AppError::bad_request("Bounty has not been pushed to escrow"))?;
        hold_milestone_on_chain(&state, &repo, milestone_index).await?;
        Ok(Json(crate::modules::bounty::model::HoldIssueResponse {
            ok: true,
        }))
    }
}

pub async fn reject_bounty_with_pool_refund(
    state: &AppState,
    bounty: &crate::shared::models::Bounty,
    repo: &crate::shared::models::Repo,
) -> Result<rust_decimal::Decimal, AppError> {
    let reward_amount = automation::escrow_reward_amount(state, repo, bounty).await?;

    if let Some(milestone_index) = bounty.milestone_index {
        reject_milestone_on_chain(state, repo, milestone_index).await?;
    }

    if let Some((_, Some(contributor))) = get_assignment_for_issue(state, bounty.id).await? {
        Service::notify_contributor_quiet(
            state,
            Notify {
                recipient: contributor.id,
                kind: Kind::BountyRejected,
                title: format!("Bounty Rejected: #{}", bounty.github_issue_number),
                body: format!(
                    "Your work on issue #{} has been rejected",
                    bounty.github_issue_number
                ),
                ref_id: Some(bounty.id),
                data: serde_json::json!({
                    "repoId": repo.id,
                    "issueNumber": bounty.github_issue_number,
                }),
                dedupe_key: format!("rejected:{}", bounty.id),
                actor: None,
            },
        )
        .await;
    }

    cancel_bounty_with_refund(state, repo, bounty.id, Some(reward_amount)).await?;
    Ok(reward_amount)
}

async fn upsert_profile_wallet(
    state: &AppState,
    github_id: i64,
    username: &str,
    chain: &str,
    address: &str,
) -> Result<(), AppError> {
    crate::modules::contributor::repository::upsert_contributor_wallet(
        state, github_id, username, chain, address,
    )
    .await
}

async fn advance_inline(state: &AppState, bounty_id: Uuid) -> Result<(), AppError> {
    use automation::Decision;

    let Some(ctx) = automation::load_context(state, bounty_id).await? else {
        return Ok(());
    };

    match automation::evaluate(state, &ctx).await? {
        Decision::PushMilestone { .. } => {
            let Some(fresh_ctx) = automation::load_context(state, bounty_id).await? else {
                return Ok(());
            };
            let Some(assignee) = fresh_ctx.assignee.as_ref() else {
                return Ok(());
            };
            let live_issue = crate::modules::github::auth::fetch_github_issue(
                state,
                fresh_ctx.repo.github_repo_id,
                &fresh_ctx.repo.full_name,
                fresh_ctx.issue.github_issue_number,
            )
            .await?;
            if !live_issue
                .assignees
                .iter()
                .any(|current| current.id == assignee.github_id)
            {
                return Ok(());
            }
            if let Decision::PushMilestone {
                payout_address,
                payout_chain,
            } = automation::evaluate(state, &fresh_ctx).await?
            {
                automation::push_milestone(state, &fresh_ctx, &payout_address, &payout_chain)
                    .await?;
            }
        }
        Decision::ReleasePayout {
            milestone_index,
            escrow_amount,
        } => {
            automation::release_payout(state, &ctx, milestone_index, escrow_amount).await?;
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
