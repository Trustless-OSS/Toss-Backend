use axum::{
    routing::{get, post, put},
    Router,
};

use crate::{modules::repo::handlers, state::AppState};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/repos", get(handlers::list_repos))
        .route("/api/repos/connect", post(handlers::connect_repo))
        .route(
            "/api/repos/sync-installation",
            post(handlers::sync_installation),
        )
        .route("/api/repos/{repoId}/issues", get(handlers::list_issues))
        .route("/api/repos/{repoId}/rewards", put(handlers::update_rewards))
        .route(
            "/api/repos/{repoId}",
            get(handlers::repo_details).delete(handlers::delete_repo),
        )
}
