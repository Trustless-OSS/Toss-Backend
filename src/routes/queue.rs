use axum::{
    extract::{FromRequestParts, Path, State},
    http::{request::Parts, StatusCode},
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use tracing::error;

use crate::{error::AppError, state::AppState};

const ADMIN_TOKEN_HEADER: &str = "x-queue-admin-token";

/// Shared-secret guard for the operator replay/inspection endpoints. Kept
/// deliberately simple (a static token, not a full user session) since
/// these are meant to be called by an operator or a runbook script.
pub struct QueueAdmin;

impl FromRequestParts<AppState> for QueueAdmin {
    type Rejection = AppError;

    fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> impl std::future::Future<Output = Result<Self, Self::Rejection>> + Send {
        let expected = state.config.queue_admin_token.clone();
        let provided = parts
            .headers
            .get(ADMIN_TOKEN_HEADER)
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned);

        async move {
            let Some(expected) = expected else {
                return Err(AppError::forbidden("Queue admin API is not configured"));
            };
            let provided = provided.unwrap_or_default();

            if !constant_time_eq(expected.as_bytes(), provided.as_bytes()) {
                return Err(AppError::unauthorized("Invalid queue admin token"));
            }
            Ok(Self)
        }
    }
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

pub async fn queue_stats_handler(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, AppError> {
    state.queue.stats().await.map(Json).map_err(|error| {
        error!(%error, "failed to get queue stats");
        error
    })
}

pub async fn list_dead_letters_handler(
    _admin: QueueAdmin,
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, AppError> {
    let entries = state.queue.list_dead_letters(200).await?;
    Ok(Json(
        serde_json::json!({ "count": entries.len(), "jobs": entries }),
    ))
}

#[derive(Debug, Deserialize)]
pub struct ReplayRequest {
    /// Identity of the operator triggering the replay, for the audit trail.
    pub replayed_by: String,
}

pub async fn replay_one_handler(
    _admin: QueueAdmin,
    State(state): State<AppState>,
    Path(job_id): Path<String>,
    Json(body): Json<ReplayRequest>,
) -> Result<StatusCode, AppError> {
    let replayed = state
        .queue
        .replay_dead_letter(&job_id, &body.replayed_by)
        .await?;
    if replayed {
        Ok(StatusCode::ACCEPTED)
    } else {
        Err(AppError::not_found("Dead-letter job not found"))
    }
}

#[derive(Debug, Deserialize)]
pub struct BatchReplayRequest {
    pub replayed_by: String,
    #[serde(default)]
    pub event: Option<String>,
    #[serde(default)]
    pub action: Option<String>,
    #[serde(default = "default_batch_limit")]
    pub limit: usize,
}

fn default_batch_limit() -> usize {
    50
}

#[derive(Debug, Serialize)]
pub struct BatchReplayResponse {
    pub replayed: usize,
}

pub async fn replay_batch_handler(
    _admin: QueueAdmin,
    State(state): State<AppState>,
    Json(body): Json<BatchReplayRequest>,
) -> Result<Json<BatchReplayResponse>, AppError> {
    let replayed = state
        .queue
        .replay_dead_letter_batch(
            body.event.as_deref(),
            body.action.as_deref(),
            body.limit,
            &body.replayed_by,
        )
        .await?;
    Ok(Json(BatchReplayResponse { replayed }))
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/queue/stats", get(queue_stats_handler))
        .route(
            "/api/queue/webhooks/dead-letter",
            get(list_dead_letters_handler).post(replay_batch_handler),
        )
        .route(
            "/api/queue/webhooks/dead-letter/{job_id}/replay",
            post(replay_one_handler),
        )
}
