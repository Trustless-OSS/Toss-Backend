use axum::{routing::get, Router};

use crate::state::AppState;

use super::handlers::{get_activity, get_contributors_leaderboard};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/activity", get(get_activity))
        .route(
            "/api/contributors/leaderboard",
            get(get_contributors_leaderboard),
        )
}
