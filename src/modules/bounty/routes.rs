use super::handler::*;
use crate::state::AppState;
use axum::{routing::post, Router};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/v1/milestones/push", post(push_milestone))
        .route("/api/v1/issues/{issueId}/retry", post(retry_issue))
}
