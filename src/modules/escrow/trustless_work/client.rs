use reqwest::Method;
use serde_json::Value;
use tracing::debug;

use crate::{error::AppError, state::AppState};

pub async fn tw_fetch(
    state: &AppState,
    path: &str,
    method: Method,
    body: Option<Value>,
) -> Result<Value, AppError> {
    let api_key = state.config.trustless_work_api_key.as_str();
    let base_url = state.config.trustless_work_base_url.as_str();

    let url = format!("{}{}", base_url, path);

    debug!(%url, ?method, "TrustlessWork request");

    let mut request = state
        .http_client
        .request(method.clone(), &url)
        .header("Content-Type", "application/json")
        .header("x-api-key", api_key);

    if let Some(body) = body {
        request = request.json(&body);
    }

    let response = request.send().await.map_err(|error| {
        AppError::internal(format!("[Trustless-Work] : request failed: {error}"))
    })?;

    let status = response.status();
    let text = response.text().await.map_err(|error| {
        AppError::internal(format!("[Trustless-Work] : response read failed: {error}"))
    })?;

    if !status.is_success() {
        return Err(AppError::internal(format!(
            "[Trustless-Work] : {method} {path} → {status}: {text}"
        )));
    }

    if text.trim().is_empty() {
        return Ok(Value::Null);
    }

    serde_json::from_str(&text).map_err(|error| {
        AppError::internal(format!("[Trustless-Work] : JSON parse failed: {error}"))
    })
}

// [ryzen-xp] : This health check fn is checking balance of an dummy escrow .

pub async fn health_check(state: &AppState) -> Result<Value, AppError> {
    let escrow_address = "CDQ6UR6RXUNEWZTQUWUBBLSUFP3XUF2F4RWXQWDFZR632RJIDMUA7U2D";
    let path = format!(
        "/helper/get-multiple-escrow-balance?addresses[]={}",
        escrow_address
    );

    let response = tw_fetch(state, &path, Method::GET, None).await?;

    if response.is_array() {
        Ok(response)
    } else {
        Err(AppError::internal(format!(
            "TrustlessWork health check failed: {}",
            response
        )))
    }
}
