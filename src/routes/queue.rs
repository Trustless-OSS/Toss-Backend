use axum::{extract::State, routing::get, Json, Router};
use serde::Serialize;
use tracing::error;
use utoipa::ToSchema;

use crate::{error::AppError, state::AppState};

/// Live BullMQ job counts for a single queue.
#[derive(Debug, Serialize, ToSchema)]
pub struct QueueCounts {
    pub waiting: u64,
    pub active: u64,
    pub completed: u64,
    pub failed: u64,
    pub delayed: u64,
}

/// Counts for every queue the backend runs.
///
/// `webhooks` is `toss-webhooks`, `escrow-operations` is the money queue
/// `toss-bounty`, and `sync` is `toss-sync`.
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
        (status = 200, description = "Live BullMQ counts per queue", body = QueueStatsResponse),
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
