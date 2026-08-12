use crate::modules::bounty::service::BountyService;
use axum::{routing::post, Router};

use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/milestones/push", post(BountyService::push_milestone))
        .route(
            "/api/issues/{issueId}/retry",
            post(BountyService::retry_issue),
        )
}
