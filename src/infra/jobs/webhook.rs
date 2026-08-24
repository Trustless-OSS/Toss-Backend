//! The `github-webhook` processor.

use crate::{
    error::AppError,
    infra::{jobs::payload, queue::WebhookJobData},
    state::AppState,
};

/// Process one signed GitHub delivery.
pub(crate) async fn run(
    state: &AppState,
    job: &bullmq::Job,
) -> Result<serde_json::Value, AppError> {
    let data: WebhookJobData = payload(job)?;
    let event = data.event.clone();
    let action = data.action.clone();

    crate::modules::github::webhook::process_webhook_job(state, data).await?;

    Ok(serde_json::json!({
        "event": event,
        "action": action,
    }))
}
