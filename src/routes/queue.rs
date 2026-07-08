use axum::{extract::State, routing::get, Json, Router};
use tracing::error;

use crate::{error::AppError, state::AppState};

pub async fn queue_stats_handler(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, AppError> {
    state.queue.stats().await.map(Json).map_err(|error| {
        error!(%error, "failed to get queue stats");
        error
    })
}

pub fn router() -> Router<AppState> {
    Router::new().route("/api/queue/stats", get(queue_stats_handler))
}
