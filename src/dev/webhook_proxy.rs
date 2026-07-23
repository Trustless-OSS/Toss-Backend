use std::time::Duration;

use axum::http::{header, HeaderMap, HeaderName, HeaderValue};
use hmac::{Hmac, Mac};
use serde_json::Value;
use sha2::Sha256;
use tracing::{info, warn};

use crate::{error::AppError, state::AppState};

const RECONNECT_DELAY: Duration = Duration::from_secs(5);

struct ForwardRequest {
    body: String,
    headers: HeaderMap,
    event: Option<String>,
    action: Option<String>,
    delivery_id: Option<String>,
}

pub fn start_if_enabled(state: AppState) {
    if !state.config.dev_webhook_proxy_enabled {
        return;
    }

    let source = state.config.smee_source_url.clone();
    let target = state.config.smee_target_url.clone();
    info!(%source, %target, "starting development Smee webhook proxy");

    tokio::spawn(async move {
        loop {
            if let Err(error) = consume_events(&state, &source, &target).await {
                warn!(%error, ?RECONNECT_DELAY, "Smee webhook proxy disconnected");
            }
            tokio::time::sleep(RECONNECT_DELAY).await;
        }
    });
}

async fn consume_events(state: &AppState, source: &str, target: &str) -> Result<(), AppError> {
    let mut response = state
        .http_client
        .get(source)
        .header(header::ACCEPT, "text/event-stream")
        .send()
        .await
        .map_err(|error| AppError::internal(format!("Smee connection failed: {error}")))?
        .error_for_status()
        .map_err(|error| AppError::internal(format!("Smee rejected connection: {error}")))?;

    info!(%source, %target, "development Smee webhook proxy connected");
    let mut buffer = Vec::new();

    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|error| AppError::internal(format!("Smee stream read failed: {error}")))?
    {
        buffer.extend_from_slice(&chunk);
        while let Some((event_end, delimiter_len)) = event_boundary(&buffer) {
            let event = buffer.drain(..event_end).collect::<Vec<_>>();
            buffer.drain(..delimiter_len);
            if let Some(data) = event_data(&event) {
                forward_event(state, target, &data).await;
            }
        }
    }

    Err(AppError::internal("Smee event stream closed"))
}

async fn forward_event(state: &AppState, target: &str, data: &str) {
    let request = match build_forward_request(data, &state.config.github_webhook_secret) {
        Ok(request) => request,
        Err(error) => {
            warn!(%error, "invalid Smee webhook event ignored");
            return;
        }
    };

    let event = request.event.as_deref().unwrap_or("unknown");
    let action = request.action.as_deref().unwrap_or("");
    let delivery_id = request.delivery_id.as_deref().unwrap_or("unknown");

    match state
        .http_client
        .post(target)
        .headers(request.headers)
        .body(request.body)
        .send()
        .await
    {
        Ok(response) if response.status().is_success() => {
            info!(
                status = %response.status(),
                %target,
                github_event = event,
                %action,
                %delivery_id,
                "Smee webhook forwarded"
            );
        }
        Ok(response) => {
            warn!(
                status = %response.status(),
                %target,
                github_event = event,
                %action,
                %delivery_id,
                "local webhook rejected Smee event"
            );
        }
        Err(error) => {
            warn!(%error, %target, "failed to forward Smee webhook");
        }
    }
}

fn build_forward_request(data: &str, webhook_secret: &str) -> Result<ForwardRequest, AppError> {
    let mut event: Value = serde_json::from_str(data)
        .map_err(|error| AppError::bad_request(format!("Invalid Smee event JSON: {error}")))?;
    let object = event
        .as_object_mut()
        .ok_or_else(|| AppError::bad_request("Smee event must be a JSON object"))?;
    let body = object
        .remove("body")
        .ok_or_else(|| AppError::bad_request("Smee event is missing body"))?;
    object.remove("query");

    let body = serde_json::to_string(&body)
        .map_err(|error| AppError::internal(format!("Failed to encode webhook body: {error}")))?;
    let mut headers = HeaderMap::new();

    for (name, value) in object.iter() {
        if matches!(
            name.to_ascii_lowercase().as_str(),
            "host" | "content-length" | "connection" | "transfer-encoding" | "x-hub-signature-256"
        ) {
            continue;
        }
        let Ok(name) = HeaderName::from_bytes(name.as_bytes()) else {
            continue;
        };
        let value = value
            .as_str()
            .map(str::to_owned)
            .unwrap_or_else(|| value.to_string());
        if let Ok(value) = HeaderValue::from_str(&value) {
            headers.insert(name, value);
        }
    }

    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );
    let signature = webhook_signature(webhook_secret, body.as_bytes())?;
    headers.insert(
        HeaderName::from_static("x-hub-signature-256"),
        HeaderValue::from_str(&signature)
            .map_err(|error| AppError::internal(format!("Invalid webhook signature: {error}")))?,
    );

    let event = headers
        .get("x-github-event")
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    let delivery_id = headers
        .get("x-github-delivery")
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    let action = serde_json::from_str::<Value>(&body).ok().and_then(|value| {
        value
            .get("action")
            .and_then(Value::as_str)
            .map(str::to_owned)
    });

    Ok(ForwardRequest {
        body,
        headers,
        event,
        action,
        delivery_id,
    })
}

fn webhook_signature(secret: &str, body: &[u8]) -> Result<String, AppError> {
    let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes())
        .map_err(|_| AppError::internal("Invalid webhook secret"))?;
    mac.update(body);
    Ok(format!(
        "sha256={}",
        hex::encode(mac.finalize().into_bytes())
    ))
}

fn event_boundary(buffer: &[u8]) -> Option<(usize, usize)> {
    buffer
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .map(|index| (index, 4))
        .or_else(|| {
            buffer
                .windows(2)
                .position(|window| window == b"\n\n")
                .map(|index| (index, 2))
        })
}

fn event_data(event: &[u8]) -> Option<String> {
    let event = std::str::from_utf8(event).ok()?;
    if event.lines().any(|line| {
        line.strip_prefix("event:")
            .is_some_and(|name| name.trim() != "message")
    }) {
        return None;
    }
    let data = event
        .lines()
        .filter_map(|line| line.strip_prefix("data:"))
        .map(|line| line.strip_prefix(' ').unwrap_or(line))
        .collect::<Vec<_>>()
        .join("\n");
    (!data.is_empty()).then_some(data)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_sse_data_with_both_line_endings() {
        assert_eq!(event_boundary(b"data: {}\r\n\r\nnext"), Some((8, 4)));
        assert_eq!(event_boundary(b"data: {}\n\nnext"), Some((8, 2)));
        assert_eq!(
            event_data(b"event: message\ndata: {\"ok\":true}"),
            Some("{\"ok\":true}".to_string())
        );
        assert_eq!(event_data(b"event: ready\ndata: {}"), None);
        assert_eq!(event_data(b"event: ping\ndata: {}"), None);
    }

    #[test]
    fn builds_forward_request_with_github_headers_and_new_signature() {
        let data = serde_json::json!({
            "x-github-event": "issues",
            "x-github-delivery": "delivery-1",
            "x-hub-signature-256": "sha256=original",
            "host": "smee.io",
            "body": { "action": "opened" },
            "query": { "ignored": "true" }
        })
        .to_string();

        let request = build_forward_request(&data, "secret").unwrap();
        assert_eq!(request.body, "{\"action\":\"opened\"}");
        assert_eq!(request.headers["x-github-event"], "issues");
        assert_eq!(request.headers["x-github-delivery"], "delivery-1");
        assert_eq!(request.event.as_deref(), Some("issues"));
        assert_eq!(request.action.as_deref(), Some("opened"));
        assert_eq!(request.delivery_id.as_deref(), Some("delivery-1"));
        assert!(!request.headers.contains_key("host"));
        assert_eq!(
            request.headers["x-hub-signature-256"],
            webhook_signature("secret", request.body.as_bytes()).unwrap()
        );
    }
}
