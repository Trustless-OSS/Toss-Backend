use axum::{
    extract::{Path, State},
    routing::post,
    Json, Router,
};
use uuid::Uuid;

use crate::{
    error::{AppError, ErrorResponse},
    middleware::auth::AuthedUser,
    modules::bounty::{
        model::{Milestone, MilestoneResponse, RetryIssueResponse},
        service::BountyService,
    },
    state::AppState,
};

#[utoipa::path(
    post,
    path = "/api/milestones/push",
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
    path = "/api/issues/{issueId}/retry",
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

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/milestones/push", post(push_milestone))
        .route("/api/issues/{issueId}/retry", post(retry_issue))
}
