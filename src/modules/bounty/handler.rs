use axum::{
    extract::{Path, State},
    Json,
};
use uuid::Uuid;

use crate::{
    error::{AppError, ErrorResponse},
    middleware::auth::AuthedUser,
    modules::bounty::{
        model::{
            HoldIssueResponse, Milestone, MilestoneResponse, RejectIssueResponse,
            RetryIssueResponse,
        },
        service::BountyService,
    },
    state::AppState,
};

#[utoipa::path(
    post,
    path = "/api/v1/milestones/push",
    tag = "Bounty",
    security(("bearer_auth" = [])),
    request_body = Milestone,
    responses(
        (status = 200, description = "Contributor wallet connected and milestone pushed to escrow", body = MilestoneResponse),
        (status = 400, description = "Issue is not in a valid state", body = ErrorResponse),
        (status = 401, description = "Missing or invalid bearer token", body = ErrorResponse),
        (status = 403, description = "Caller is not the assigned contributor", body = ErrorResponse),
        (status = 404, description = "Repository not found", body = ErrorResponse),
        (status = 500, description = "Failed to push milestone", body = ErrorResponse)
    )
)]
pub async fn push_milestone(
    state: State<AppState>,
    user: AuthedUser,
    body: Json<Milestone>,
) -> Result<Json<MilestoneResponse>, AppError> {
    BountyService::push_milestone(state, user, body).await
}

#[utoipa::path(
    post,
    path = "/api/v1/issues/{issueId}/retry",
    tag = "Bounty",
    security(("bearer_auth" = [])),
    params(("issueId" = Uuid, Path, description = "Issue UUID")),
    responses(
        (status = 200, description = "Retry step applied (push, release, or already up to date)", body = RetryIssueResponse),
        (status = 400, description = "Issue cannot be retried in its current state", body = ErrorResponse),
        (status = 401, description = "Missing or invalid bearer token", body = ErrorResponse),
        (status = 403, description = "Caller is not a maintainer", body = ErrorResponse),
        (status = 404, description = "Issue not found", body = ErrorResponse),
        (status = 500, description = "Failed to retry issue", body = ErrorResponse)
    )
)]
pub async fn retry_issue(
    state: State<AppState>,
    user: AuthedUser,
    issue_id: Path<Uuid>,
) -> Result<Json<RetryIssueResponse>, AppError> {
    BountyService::retry_issue(state, user, issue_id).await
}

#[utoipa::path(
    post,
    path = "/api/v1/issues/{issueId}/reject",
    tag = "Bounty",
    security(("bearer_auth" = [])),
    params(("issueId" = Uuid, Path, description = "Issue UUID")),
    responses(
        (status = 200, description = "Bounty cancelled and reward returned to the free escrow pool", body = RejectIssueResponse),
        (status = 400, description = "Issue has already been paid or cancelled", body = ErrorResponse),
        (status = 401, description = "Missing or invalid bearer token", body = ErrorResponse),
        (status = 403, description = "Caller is not a maintainer", body = ErrorResponse),
        (status = 404, description = "Issue or repository not found", body = ErrorResponse),
        (status = 500, description = "Failed to cancel bounty", body = ErrorResponse)
    )
)]
pub async fn reject_issue(
    state: State<AppState>,
    user: AuthedUser,
    issue_id: Path<Uuid>,
) -> Result<Json<RejectIssueResponse>, AppError> {
    BountyService::reject_issue(state, user, issue_id).await
}

#[utoipa::path(
    post,
    path = "/api/v1/issues/{issueId}/hold",
    tag = "Bounty",
    security(("bearer_auth" = [])),
    params(("issueId" = Uuid, Path, description = "Issue UUID")),
    responses(
        (status = 200, description = "On-chain milestone disputed and held", body = HoldIssueResponse),
        (status = 400, description = "Bounty is not on-chain or cannot be held", body = ErrorResponse),
        (status = 401, description = "Missing or invalid bearer token", body = ErrorResponse),
        (status = 403, description = "Caller is not a maintainer", body = ErrorResponse),
        (status = 404, description = "Issue not found", body = ErrorResponse)
    )
)]
pub async fn hold_issue(
    state: State<AppState>,
    user: AuthedUser,
    issue_id: Path<Uuid>,
) -> Result<Json<HoldIssueResponse>, AppError> {
    BountyService::hold_issue(state, user, issue_id).await
}
