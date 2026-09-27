use axum::{
    extract::{Path, State},
    Json,
};
use tracing::info;
use uuid::Uuid;

use crate::{
    error::{map_db_err, require_db, AppError},
    infra::queue::{BountyJobData, EnqueueOutcome},
    middleware::auth::AuthedUser,
    modules::{
        bounty::{
            automation,
            model::{Milestone, MilestoneResponse, RetryIssueResponse},
            repository::{
                get_bounty_by_repo_and_github_id, get_bounty_with_repo, is_assigned_contributor,
            },
        },
        repo::repository::{get_repo_by_github_id, is_maintainer},
    },
    shared::models::schema,
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
            .to_string();
        let payout_address = body
            .payout_address
            .as_deref()
            .unwrap_or(&body.wallet)
            .to_string();

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

        if bounty.status != "open" && bounty.status != "assigned" {
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
}

async fn upsert_profile_wallet(
    state: &AppState,
    github_id: i64,
    username: &str,
    chain: &str,
    address: &str,
) -> Result<(), AppError> {
    let mut db = require_db(&state.db)?;

    let profile = schema::Profile::upsert_by_github_id(github_id)
        .username(username.to_string())
        .exec(&mut db)
        .await
        .map_err(map_db_err)?;

    schema::Wallet::upsert_by_profile_id_and_chain_and_address(
        profile.id,
        chain.to_string(),
        address.to_string(),
    )
    .is_primary(true)
    .exec(&mut db)
    .await
    .map_err(map_db_err)?;

    Ok(())
}

async fn advance_inline(state: &AppState, bounty_id: Uuid) -> Result<(), AppError> {
    use automation::Decision;

    let Some(ctx) = automation::load_context(state, bounty_id).await? else {
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
