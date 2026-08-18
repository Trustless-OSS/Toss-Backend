use axum::{extract::State, routing::get, Json, Router};
use serde::Serialize;
use tracing::error;
use utoipa::ToSchema;

use crate::{error::AppError, state::AppState};

#[derive(Debug, Serialize, ToSchema)]
pub struct QueueCounts {
    pub waiting: i64,
    pub active: i64,
    pub completed: i64,
    pub failed: i64,
    pub delayed: i64,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct QueueStatsResponse {
    pub webhooks: QueueCounts,
    #[serde(rename = "escrow-operations")]
    pub escrow_operations: QueueCounts,
    pub sync: QueueCounts,
}

#[utoipa::path(
    get,
    path = "/api/queue/stats",
    tag = "Queue",
    responses(
        (status = 200, description = "Current background queue depths", body = QueueStatsResponse),
        (status = 500, description = "Failed to read queue stats", body = crate::error::ErrorResponse)
    )
)]
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
