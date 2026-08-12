use axum::{
    extract::{Path, State},
    Json,
};
use tracing::error;
use uuid::Uuid;

use crate::{
    error::AppError,
    middleware::auth::AuthedUser,
    modules::bounty::model::{Milestone, MilestoneResponse, RetryIssueResponse},
    modules::escrow::service::{push_milestone_on_chain, release_escrow_milestone},
    modules::github::auth::{fetch_github_issue_state, post_comment},
    modules::repo::repository::{
        get_assignment_for_issue, get_issue_by_repo_and_github_id, get_issue_with_repo,
        get_repo_by_github_id, is_assigned_contributor, is_maintainer,
        update_assignment_payout_status, update_issue_status, upsert_contributor_wallet,
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

        push_milestone_on_chain(&state, &repo, &issue, &payout_address, &payout_chain).await?;

        let comment = format!(
        "✅ **Wallet Connected!** Bounty of **{} USDC** is now locked in escrow. Merge the PR to release the funds.",
        issue.reward_amount
    );
        if let Err(error) =
            post_comment(&state, &repo.full_name, issue.github_issue_number, &comment).await
        {
            error!(%error, "failed to post comment after wallet connect");
        }

        Ok(Json(MilestoneResponse {
            ok: true,
            repo_full_name: repo.full_name,
            issue_number: issue.github_issue_number,
        }))
    }

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

        let (assignment, contributor) = get_assignment_for_issue(&state, issue.id)
            .await?
            .ok_or_else(|| AppError::bad_request("No assignment found"))?;

        if issue.status == "pending" {
            let contributor = contributor.ok_or_else(|| {
                AppError::bad_request("Contributor has not connected a wallet yet")
            })?;

            let payout_address = contributor
                .payout_address
                .as_deref()
                .or(contributor.stellar_wallet.as_deref())
                .ok_or_else(|| {
                    AppError::bad_request("Contributor has not connected a wallet yet")
                })?;
            let payout_chain = contributor.payout_chain.as_deref().unwrap_or("stellar");

            push_milestone_on_chain(&state, &repo, &issue, payout_address, payout_chain).await?;

            return Ok(Json(RetryIssueResponse {
                ok: true,
                step: Some("pushed"),
                status: Some("active"),
                tx_hash: None,
                message: None,
            }));
        }

        if issue.status == "active" {
            let gh_state =
                fetch_github_issue_state(&state, &repo.full_name, issue.github_issue_number)
                    .await?;

            if gh_state != "closed" {
                return Err(AppError::bad_request(
                    "This issue is still open on GitHub. Please close it or merge the PR first.",
                ));
            }

            let tx_hash = release_escrow_milestone(&state, &repo, &issue).await?;
            update_assignment_payout_status(&state, assignment.id, "released").await?;
            update_issue_status(&state, issue.id, "completed", None).await?;

            return Ok(Json(RetryIssueResponse {
                ok: true,
                step: Some("released"),
                status: None,
                tx_hash: Some(tx_hash),
                message: None,
            }));
        }

        Ok(Json(RetryIssueResponse {
            ok: true,
            step: None,
            status: None,
            tx_hash: None,
            message: Some("Process is already up to date"),
        }))
    }
}
