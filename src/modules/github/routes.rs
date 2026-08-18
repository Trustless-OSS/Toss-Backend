use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    routing::post,
    Router,
};
use hmac::{Hmac, Mac};
use sha2::Sha256;
use tracing::{debug, error};

use crate::infra::queue::{WebhookEnqueueOutcome, WebhookJobData};
use crate::{error::AppError, state::AppState};

/// Free-form GitHub webhook JSON payload.
#[derive(utoipa::ToSchema)]
pub struct GitHubWebhookPayload(serde_json::Value);

#[utoipa::path(
    post,
    path = "/api/webhooks/github",
    tag = "GitHub",
    params(
        ("X-Hub-Signature-256" = String, Header, description = "HMAC SHA-256 signature (`sha256=<hex>`) of the raw body using `GITHUB_WEBHOOK_SECRET`"),
        ("X-GitHub-Event" = String, Header, description = "GitHub event name, e.g. `issues`, `pull_request`, `installation`"),
        ("X-GitHub-Delivery" = Option<String>, Header, description = "Unique delivery ID used for enqueue deduplication")
    ),
    request_body(
        content = GitHubWebhookPayload,
        description = "Raw GitHub webhook JSON payload",
        content_type = "application/json"
    ),
    responses(
        (status = 202, description = "Webhook accepted (queued or processed inline)"),
        (status = 400, description = "Invalid JSON body", body = crate::error::ErrorResponse),
        (status = 401, description = "Invalid webhook signature", body = crate::error::ErrorResponse),
        (status = 500, description = "Failed to enqueue or process webhook", body = crate::error::ErrorResponse)
    )
)]
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

    if !verify_github_signature(github_secret, body.as_bytes(), signature)? {
        error!("GitHub webhook signature verification failed");
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
        attempts: 0,
    };

    if matches!(event, "installation" | "installation_repositories") {
        crate::modules::github::webhook::process_webhook_job(&state, job).await?;
        return Ok(StatusCode::ACCEPTED);
    }

    // Enqueue the webhook job
    let outcome = state
        .queue
        .enqueue_webhook(delivery_id, job.clone())
        .await
        .map_err(|e| {
            error!("Failed to enqueue webhook: {}", e);
            e
        })?;

    if outcome == WebhookEnqueueOutcome::Unavailable {
        crate::modules::github::webhook::process_webhook_job(&state, job).await?;
    }

    Ok(StatusCode::ACCEPTED)
}

fn verify_github_signature(secret: &str, body: &[u8], signature: &str) -> Result<bool, AppError> {
    type HmacSha256 = Hmac<Sha256>;
    let mut mac = HmacSha256::new_from_slice(secret.as_bytes())
        .map_err(|_| AppError::internal("Invalid HMAC key"))?;
    mac.update(body);

    let provided = signature
        .strip_prefix("sha256=")
        .and_then(|value| hex::decode(value).ok());
    Ok(provided
        .as_deref()
        .is_some_and(|provided| mac.verify_slice(provided).is_ok()))
}

pub fn router() -> Router<AppState> {
    Router::new().route("/api/webhooks/github", post(handle_github_webhook))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn signature(secret: &str, body: &[u8]) -> String {
        let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes()).unwrap();
        mac.update(body);
        format!("sha256={}", hex::encode(mac.finalize().into_bytes()))
    }

    #[test]
    fn verifies_valid_signature_and_rejects_modified_payload() {
        let secret = "webhook-secret";
        let body = br#"{"action":"opened"}"#;
        let signature = signature(secret, body);

        assert!(verify_github_signature(secret, body, &signature).unwrap());
        assert!(!verify_github_signature(secret, br#"{"action":"closed"}"#, &signature).unwrap());
    }

    #[test]
    fn rejects_missing_prefix_and_invalid_hex() {
        assert!(!verify_github_signature("secret", b"{}", "abc").unwrap());
        assert!(!verify_github_signature("secret", b"{}", "sha256=xyz").unwrap());
    }
}
