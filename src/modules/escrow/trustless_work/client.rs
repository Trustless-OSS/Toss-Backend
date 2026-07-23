use reqwest::Method;
use serde_json::Value;
use tracing::debug;

use crate::{config::Config, error::AppError, state::AppState};

pub async fn tw_fetch(
    state: &AppState,
    path: &str,
    method: Method,
    body: Option<Value>,
) -> Result<Value, AppError> {
    let api_key = state.config.trustless_work_api_key.as_str();

    let url = format!("{}{}", state.config.trustless_work_base_url, path);
    debug!(%url, ?method, "TrustlessWork request");

    let mut request = state
        .http_client
        .request(method.clone(), &url)
        .header("Content-Type", "application/json")
        .header("x-api-key", api_key);

    if let Some(body) = body {
        request = request.json(&body);
    }

    let response = request
        .send()
        .await
        .map_err(|error| AppError::internal(format!("TrustlessWork request failed: {error}")))?;

    let status = response.status();
    let text = response.text().await.map_err(|error| {
        AppError::internal(format!("TrustlessWork response read failed: {error}"))
    })?;

    if !status.is_success() {
        return Err(AppError::internal(format!(
            "[TrustlessWork] {method} {path} → {status}: {text}"
        )));
    }

    if text.trim().is_empty() {
        return Ok(Value::Null);
    }

    serde_json::from_str(&text)
        .map_err(|error| AppError::internal(format!("TrustlessWork JSON parse failed: {error}")))
}

pub async fn health_check(config: &Config, client: &reqwest::Client) -> Result<(), AppError> {
    let url = format!(
        "{}/docs",
        config.trustless_work_base_url.trim_end_matches('/')
    );
    let response = client
        .get(url)
        .send()
        .await
        .map_err(|error| AppError::internal(error.to_string()))?;

    if response.status().is_success() {
        Ok(())
    } else {
        Err(AppError::internal(format!(
            "TrustlessWork health check failed: {}",
            response.status()
        )))
    }
}
