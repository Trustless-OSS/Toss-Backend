use serde_json::Value;
use tracing::info;

use crate::{
    error::AppError,
    infra::queue::BountyJobData,
    modules::{
        bounty::repository::get_issue_by_repo_and_github_id,
        repo::repository::get_repo_by_github_id,
    },
    state::AppState,
};

/// Hand a completed issue to the automation.
///
/// Closing an issue is one of the two events that can satisfy the release rule
/// (the other is a merged PR). It does not release anything by itself — the
/// `advance-issue` worker still has to find the milestone on-chain, unreleased,
/// and paid to the assigned contributor's wallet.
pub async fn handle_issue_closed(state: &AppState, payload: &Value) -> Result<(), AppError> {
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

    let Some(repository) = payload.get("repository") else {
        return Ok(());
    };
    let repo_github_id = repository.get("id").and_then(Value::as_i64).unwrap_or(0);
    let github_issue_id = issue.get("id").and_then(Value::as_i64).unwrap_or(0);

    let Some(repo) = get_repo_by_github_id(state, repo_github_id).await? else {
        return Ok(());
    };
    let Some(issue_record) =
        get_issue_by_repo_and_github_id(state, repo.id, github_issue_id).await?
    else {
        return Ok(());
    };

    let outcome = state
        .queue
        .enqueue_advance_issue(BountyJobData::new(issue_record.id, "issue-closed").notifying())
        .await?;

    info!(
        issue = issue_number,
        outcome = outcome.label(),
        "issue closed as completed; bounty automation queued"
    );

    Ok(())
}
