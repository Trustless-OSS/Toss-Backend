use serde_json::Value;
use tracing::info;

use crate::{error::AppError, state::AppState};

pub async fn handle_issue_closed(state: &AppState, payload: &Value) -> Result<(), AppError> {
    let _ = state;
    let issue = payload
        .get("issue")
        .ok_or_else(|| AppError::webhook("issues.closed payload missing issue"))?;

    let state_reason = issue
        .get("state_reason")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    if state_reason != "completed" {
        return Ok(());
    }

    let issue_number = issue.get("number").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
    info!(
        issue = issue_number,
        "issue closed as completed — skipping automated payout"
    );
    Ok(())
}
