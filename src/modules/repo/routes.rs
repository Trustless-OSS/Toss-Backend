use axum::{
    routing::{delete, get, post, put},
    Router,
};

use crate::{modules::repo::handlers, state::AppState};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/v1/repos", get(handlers::list_repos))
        .route("/api/v1/repos/connect", post(handlers::connect_repo))
        .route(
            "/api/v1/repos/installation-repos",
            get(handlers::list_installation_repos),
        )
        .route(
            "/api/v1/repos/sync-installation",
            post(handlers::sync_installation),
        )
        .route("/api/v1/repos/{repoId}/issues", get(handlers::list_issues))
        .route(
            "/api/v1/repos/{repoId}/rewards",
            put(handlers::update_rewards),
        )
        .route("/api/v1/repos/rewards/{repoId}", get(handlers::get_rewards))
        .route(
            "/api/v1/repos/{repoId}",
            get(handlers::repo_details).delete(handlers::delete_repo),
        )
        .route(
            "/api/v1/repos/{repoId}/github-labels",
            get(handlers::list_github_labels),
        )
        .route(
            "/api/v1/repos/{repoId}/rewards/{label}",
            delete(handlers::delete_reward),
        )
}
