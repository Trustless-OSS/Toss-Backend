use axum::{routing::get, Router};

use crate::state::AppState;

use super::handlers::*;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/v1/dashboard/activity", get(activity))
        .route("/api/v1/dashboard/summary", get(summary))
        .route("/api/v1/dashboard/funds-movement", get(funds_movement))
        .route("/api/v1/dashboard/leaderboard", get(maintainer_leaderboard))
        // Public, unauthenticated, cached.
        .route("/api/leaderboard", get(public_leaderboard))
}
