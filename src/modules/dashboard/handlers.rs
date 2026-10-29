//! Dashboard & leaderboard HTTP handlers.
//!
//! The four `dashboard/*` endpoints are maintainer-scoped (via `AuthedUser` →
//! `RepoMaintainer`). `/api/leaderboard` is public, cached, and paginated.

use axum::{
    extract::{Query, State},
    Json,
};
use serde::Deserialize;
use utoipa::IntoParams;

use crate::{
    error::{AppError, ErrorResponse},
    middleware::auth::AuthedUser,
    state::AppState,
};

use super::{
    model::{ActivityRow, DashboardSummary, FundsMovementPoint, LeaderboardPage},
    repository, service,
};

#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct LeaderboardParams {
    /// 1-based page index. Out-of-range clamps to the last page.
    pub page: Option<i64>,
    /// Rows per page. Home-page preview uses 5.
    pub page_size: Option<i64>,
}

impl LeaderboardParams {
    fn resolve(&self) -> (i64, i64) {
        let page = self.page.unwrap_or(1).max(1);
        let page_size = self.page_size.unwrap_or(20).clamp(1, 100);
        (page, page_size)
    }
}

#[utoipa::path(
    get,
    path = "/api/v1/dashboard/activity",
    tag = "Dashboard",
    security(("bearer_auth" = [])),
    responses(
        (status = 200, description = "Maintainer activity feed, newest first", body = Vec<ActivityRow>),
        (status = 401, description = "Missing or invalid bearer token", body = ErrorResponse),
        (status = 500, description = "Database error", body = ErrorResponse)
    )
)]
pub(crate) async fn activity(
    State(state): State<AppState>,
    user: AuthedUser,
) -> Result<Json<Vec<ActivityRow>>, AppError> {
    let repo_ids = repository::maintainer_repo_ids(&state, user.github_id).await?;
    let bounties = repository::enriched_bounties_for_repos(&state, &repo_ids).await?;
    Ok(Json(service::activity_feed(&bounties)))
}

#[utoipa::path(
    get,
    path = "/api/v1/dashboard/summary",
    tag = "Dashboard",
    security(("bearer_auth" = [])),
    responses(
        (status = 200, description = "Maintainer dashboard summary metrics", body = DashboardSummary),
        (status = 401, description = "Missing or invalid bearer token", body = ErrorResponse),
        (status = 500, description = "Database error", body = ErrorResponse)
    )
)]
pub(crate) async fn summary(
    State(state): State<AppState>,
    user: AuthedUser,
) -> Result<Json<DashboardSummary>, AppError> {
    let repo_ids = repository::maintainer_repo_ids(&state, user.github_id).await?;
    let bounties = repository::enriched_bounties_for_repos(&state, &repo_ids).await?;
    let feed = service::activity_feed(&bounties);
    Ok(Json(service::summary(&feed)))
}

#[utoipa::path(
    get,
    path = "/api/v1/dashboard/funds-movement",
    tag = "Dashboard",
    security(("bearer_auth" = [])),
    responses(
        (status = 200, description = "Monthly cumulative TVL vs payouts", body = Vec<FundsMovementPoint>),
        (status = 401, description = "Missing or invalid bearer token", body = ErrorResponse),
        (status = 500, description = "Database error", body = ErrorResponse)
    )
)]
pub(crate) async fn funds_movement(
    State(state): State<AppState>,
    user: AuthedUser,
) -> Result<Json<Vec<FundsMovementPoint>>, AppError> {
    let repo_ids = repository::maintainer_repo_ids(&state, user.github_id).await?;
    let bounties = repository::enriched_bounties_for_repos(&state, &repo_ids).await?;
    let feed = service::activity_feed(&bounties);
    Ok(Json(service::funds_movement(&feed)))
}

#[utoipa::path(
    get,
    path = "/api/v1/dashboard/leaderboard",
    tag = "Dashboard",
    security(("bearer_auth" = [])),
    params(LeaderboardParams),
    responses(
        (status = 200, description = "Maintainer-scoped leaderboard (paginated)", body = LeaderboardPage),
        (status = 401, description = "Missing or invalid bearer token", body = ErrorResponse),
        (status = 500, description = "Database error", body = ErrorResponse)
    )
)]
pub(crate) async fn maintainer_leaderboard(
    State(state): State<AppState>,
    user: AuthedUser,
    Query(params): Query<LeaderboardParams>,
) -> Result<Json<LeaderboardPage>, AppError> {
    let (page, page_size) = params.resolve();
    let repo_ids = repository::maintainer_repo_ids(&state, user.github_id).await?;
    let bounties = repository::enriched_bounties_for_repos(&state, &repo_ids).await?;
    Ok(Json(service::leaderboard(&bounties, page, page_size)))
}

#[utoipa::path(
    get,
    path = "/api/leaderboard",
    tag = "Dashboard",
    params(LeaderboardParams),
    responses(
        (status = 200, description = "Public platform-wide leaderboard (paginated)", body = LeaderboardPage),
        (status = 500, description = "Database error", body = ErrorResponse)
    )
)]
pub(crate) async fn public_leaderboard(
    State(state): State<AppState>,
    Query(params): Query<LeaderboardParams>,
) -> Result<Json<LeaderboardPage>, AppError> {
    let (page, page_size) = params.resolve();
    let cache_key = format!("leaderboard:public:{page}:{page_size}");

    if let Some(cached) = state.cache.get::<LeaderboardPage>(&cache_key).await {
        return Ok(Json(cached));
    }

    let bounties = repository::all_enriched_bounties(&state).await?;
    let page_data = service::leaderboard(&bounties, page, page_size);

    // Public, aggregate-only data — cache a few minutes so crawling paginated URLs stays cheap.
    state.cache.set(&cache_key, &page_data, Some(180)).await;
    Ok(Json(page_data))
}
