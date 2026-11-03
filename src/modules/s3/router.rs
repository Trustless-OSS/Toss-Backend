use super::handler::upload;
use crate::state::AppState;

pub fn router() -> axum::Router<AppState> {
    use axum::routing::post;

    axum::Router::new().route("/api/v1/upload", post(upload))
}
