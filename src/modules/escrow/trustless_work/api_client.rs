use reqwest::Method;
use rust_decimal::{prelude::ToPrimitive, Decimal};
use serde_json::{Number, Value};
use tracing::debug;

use crate::{error::AppError, state::AppState};

pub(crate) fn decimal_json_number(value: Decimal, field_name: &str) -> Result<Value, AppError> {
    let number = value
        .to_f64()
        .and_then(Number::from_f64)
        .ok_or_else(|| AppError::internal(format!("Invalid {field_name}")))?;

    Ok(Value::Number(number))
}

pub(crate) fn decimal_json_string(value: Decimal, field_name: &str) -> Result<Value, AppError> {
    if value <= Decimal::ZERO {
        return Err(AppError::bad_request(format!(
            "{field_name} must be greater than zero"
        )));
    }
    Ok(Value::String(value.normalize().to_string()))
}

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
        let message = format!("[Trustless-Work] : {method} {path} → {status}: {text}");
        if status.is_client_error() {
            return Err(AppError::bad_request(message));
        }
        return Err(AppError::internal(message));
    }

    if text.trim().is_empty() {
        return Ok(Value::Null);
    }

    serde_json::from_str(&text).map_err(|error| {
        AppError::internal(format!("[Trustless-Work] : JSON parse failed: {error}"))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal::Decimal;

    #[test]
    fn decimal_json_string_formats_fund_amount() {
        let value = decimal_json_string(Decimal::new(1, 2), "amount").unwrap();
        assert_eq!(value, Value::String("0.01".into()));
    }
}

pub async fn health_check(state: &AppState) -> Result<Value, AppError> {
    let escrow_address = state.config.trustless_work_health_escrow_contract.as_str();
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
