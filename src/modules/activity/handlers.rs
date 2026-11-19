//! HTTP handlers for `GET /api/activity` and `GET /api/contributors/leaderboard`.

use axum::{
    extract::{rejection::QueryRejection, Query, State},
    Json,
};

use crate::{
    error::{AppError, ErrorResponse},
    middleware::auth::AuthedUser,
    state::AppState,
};

use super::{
    dto::{
        ActivityLedgerItem, ActivityQuery, ContributorLeaderboardEntry, LeaderboardQuery,
        PaginatedLedgerResponse,
    },
    repository, service,
};

#[utoipa::path(
    get,
    path = "/api/activity",
    tag = "Activity",
    security(("bearer_auth" = [])),
    params(ActivityQuery),
    responses(
        (status = 200, description = "Paginated maintainer activity ledger, newest first", body = PaginatedLedgerResponse<ActivityLedgerItem>),
        (status = 400, description = "Invalid kind, repo, page, or limit query parameter", body = ErrorResponse),
        (status = 401, description = "Missing or invalid bearer token", body = ErrorResponse),
        (status = 500, description = "Database error", body = ErrorResponse)
    )
)]
pub(crate) async fn get_activity(
    State(state): State<AppState>,
    user: AuthedUser,
    query: Result<Query<ActivityQuery>, QueryRejection>,
) -> Result<Json<PaginatedLedgerResponse<ActivityLedgerItem>>, AppError> {
    let Query(raw_query) = query.map_err(|err| AppError::bad_request(err.to_string()))?;
    let validated = raw_query.validate()?;

    let batches = repository::load_maintainer_event_batches(
        &state,
        user.github_id,
        validated.repo.as_deref(),
    )
    .await?;
    let events = service::flatten_batches(&batches);
    let response = service::query_activity(events, &validated);

    Ok(Json(response))
}

#[utoipa::path(
    get,
    path = "/api/contributors/leaderboard",
    tag = "Activity",
    security(("bearer_auth" = [])),
    params(LeaderboardQuery),
    responses(
        (status = 200, description = "Paginated maintainer-scoped contributor leaderboard", body = PaginatedLedgerResponse<ContributorLeaderboardEntry>),
        (status = 400, description = "Invalid repo, page, or limit query parameter", body = ErrorResponse),
        (status = 401, description = "Missing or invalid bearer token", body = ErrorResponse),
        (status = 500, description = "Database error", body = ErrorResponse)
    )
)]
pub(crate) async fn get_contributors_leaderboard(
    State(state): State<AppState>,
    user: AuthedUser,
    query: Result<Query<LeaderboardQuery>, QueryRejection>,
) -> Result<Json<PaginatedLedgerResponse<ContributorLeaderboardEntry>>, AppError> {
    let Query(raw_query) = query.map_err(|err| AppError::bad_request(err.to_string()))?;
    let validated = raw_query.validate()?;

    let batches = repository::load_maintainer_event_batches(
        &state,
        user.github_id,
        validated.repo.as_deref(),
    )
    .await?;
    let events = service::flatten_batches(&batches);
    let response = service::build_leaderboard(&events, &validated);

    Ok(Json(response))
}
