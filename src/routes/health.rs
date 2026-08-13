use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use chrono::Utc;
use serde::Serialize;
use serde_json::{json, Value};

use crate::{
    infra::redis, lifecycle, modules::escrow::trustless_work::client::health_check,
    modules::repo::repository::ping_db, state::AppState,
};

#[derive(utoipa::ToSchema, Serialize)]
pub struct ShuttingDownResponse {
    pub status: &'static str,
    pub timestamp: String,
    pub message: &'static str,
}

#[derive(utoipa::ToSchema, Serialize)]
pub struct HealthResponse {
    pub status: String,
    pub timestamp: String,
    pub env: String,
    pub version: &'static str,
    pub checks: Value,
}

#[utoipa::path(
    get,
    path = "/health",
    responses(
        (status = 200, description = "Health check succeeded", body = HealthResponse),
        (status = 503, description = "Service unavailable", body = HealthResponse),
        (status = 503, description = "Server shutting down", body = ShuttingDownResponse)
    )
)]

pub async fn health_handler(State(state): State<AppState>) -> Response {
    if lifecycle::is_shutting_down() {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(ShuttingDownResponse {
                status: "shutting_down",
                timestamp: Utc::now().to_rfc3339(),
                message: "Server is shutting down",
            }),
        )
            .into_response();
    }

    let mut checks = json!({});
    let mut is_healthy = true;

    let db_start = std::time::Instant::now();
    let db_check = match ping_db(&state).await {
        Ok(()) => json!({
            "status": "ok",
            "latency": format!("{}ms", db_start.elapsed().as_millis()),
        }),
        Err(error) => {
            is_healthy = false;
            json!({
                "status": "error",
                "latency": format!("{}ms", db_start.elapsed().as_millis()),
                "message": error.to_string(),
            })
        }
    };
    checks["database"] = db_check;

    let tw_start = std::time::Instant::now();
    checks["trustless_work"] = match health_check(&state.config, &state.http_client).await {
        Ok(()) => json!({
            "status": "ok",
            "latency": format!("{}ms", tw_start.elapsed().as_millis()),
        }),
        Err(error) => json!({
            "status": "degraded",
            "error": error.to_string(),
        }),
    };

    let redis_start = std::time::Instant::now();
    let redis_health = redis::check_health(state.redis.as_ref()).await;
    checks["redis"] = match redis_health.status {
        redis::RedisHealthStatus::Ok => json!({
            "status": "ok",
            "latency": format!("{}ms", redis_start.elapsed().as_millis()),
        }),
        redis::RedisHealthStatus::Error => json!({
            "status": "error",
            "message": "Redis unavailable",
            "latency": format!("{}ms", redis_start.elapsed().as_millis()),
        }),
    };

    let missing: Vec<&str> = [
        ("DATABASE_URL", !state.config.database_url.trim().is_empty()),
        (
            "SUPABASE_PUBLISHABLE_KEY or SUPABASE_ANON_KEY or SUPABASE_SERVICE_ROLE_KEY",
            !state.config.supabase_auth_api_key.trim().is_empty(),
        ),
        (
            "PLATFORM_STELLAR_PUBLIC_KEY",
            !state.config.platform_stellar_public_key.trim().is_empty(),
        ),
        (
            "GITHUB_WEBHOOK_SECRET",
            !state.config.github_webhook_secret.trim().is_empty(),
        ),
    ]
    .iter()
    .filter(|(_, present)| !present)
    .map(|(name, _)| *name)
    .collect();

    if !missing.is_empty() {
        is_healthy = false;
        checks["environment"] = json!({
            "status": "error",
            "missing_variables": missing,
        });
    } else {
        checks["environment"] = json!({ "status": "ok" });
    }

    let status = if is_healthy {
        "ok".to_string()
    } else {
        "unhealthy".to_string()
    };

    let status_code = if is_healthy {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };

    (
        status_code,
        Json(HealthResponse {
            status,
            timestamp: Utc::now().to_rfc3339(),
            env: state.config.node_env.clone(),
            version: "1.0.0",
            checks,
        }),
    )
        .into_response()
}

// Health Router
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/health", get(health_handler))
        .route("/api/health", get(health_handler))
}
