use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    routing::post,
    Router,
};
use tracing::{debug, error};

use crate::infra::queue::WebhookJobData;
use crate::{error::AppError, state::AppState};

pub async fn handle_github_webhook(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: String,
) -> Result<StatusCode, AppError> {
    // Verify GitHub signature
    let signature = headers
        .get("x-hub-signature-256")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    let github_secret = &state.config.github_webhook_secret;

    // Verify the signature using HMAC-SHA256
    use hmac::{Hmac, Mac};
    use sha2::Sha256;

    type HmacSha256 = Hmac<Sha256>;
    let mut mac = HmacSha256::new_from_slice(github_secret.as_bytes())
        .map_err(|_| AppError::internal("Invalid HMAC key"))?;
    mac.update(body.as_bytes());

    let expected = format!("sha256={}", hex::encode(mac.finalize().into_bytes()));

    if signature != expected {
        error!(provided = %signature, expected = %expected, "GitHub webhook signature verification failed");
        return Err(AppError::unauthorized("Invalid webhook signature"));
    }

    debug!("GitHub webhook signature verified");

    // Parse webhook payload
    let payload: serde_json::Value = serde_json::from_str(&body)
        .map_err(|e| AppError::bad_request(format!("Invalid JSON: {}", e)))?;

    let event = headers
        .get("x-github-event")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("unknown");

    let delivery_id = headers
        .get("x-github-delivery")
        .and_then(|v| v.to_str().ok());

    let action = payload
        .get("action")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    debug!(
        event = %event,
        action = ?action,
        delivery_id = ?delivery_id,
        "received GitHub webhook"
    );

    let job = WebhookJobData {
        event: event.to_string(),
        action,
        payload,
    };

    if matches!(event, "installation" | "installation_repositories") {
        crate::modules::github::webhook::process_webhook_job(&state, job).await?;
        return Ok(StatusCode::ACCEPTED);
    }

    // Enqueue the webhook job
    state
        .queue
        .enqueue_webhook(delivery_id, job)
        .await
        .map_err(|e| {
            error!("Failed to enqueue webhook: {}", e);
            e
        })?;

    Ok(StatusCode::ACCEPTED)
}

pub fn router() -> Router<AppState> {
    Router::new().route("/api/webhooks/github", post(handle_github_webhook))
}
