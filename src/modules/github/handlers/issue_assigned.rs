use serde_json::Value;
use tracing::info;

use crate::{
    error::AppError,
    infra::queue::BountyJobData,
    modules::{
        bounty::repository::{get_issue_by_repo_and_github_id, upsert_assignment},
        contributor::repository::ensure_contributor,
        repo::repository::get_repo_by_github_id,
    },
    state::AppState,
};

/// Record the assignment and hand the issue to the automation.
///
/// This handler is a producer only: it never touches escrow. Whether the bounty
/// can be locked right now — or has to wait for a wallet — is decided by the
/// `advance-issue` worker against live state.
pub async fn handle_issue_assigned(state: &AppState, payload: &Value) -> Result<(), AppError> {
    let repository = payload
        .get("repository")
        .ok_or_else(|| AppError::webhook("issues.assigned payload missing repository"))?;
    let issue = payload
        .get("issue")
        .ok_or_else(|| AppError::webhook("issues.assigned payload missing issue"))?;
    let assignee = payload
        .get("assignee")
        .ok_or_else(|| AppError::webhook("issues.assigned payload missing assignee"))?;

    let repo_github_id = repository.get("id").and_then(|v| v.as_i64()).unwrap_or(0);
    let github_issue_id = issue.get("id").and_then(|v| v.as_i64()).unwrap_or(0);
    let assignee_id = assignee.get("id").and_then(|v| v.as_i64()).unwrap_or(0);
    let assignee_login = assignee
        .get("login")
        .and_then(|v| v.as_str())
        .unwrap_or("contributor");

    let Some(repo) = get_repo_by_github_id(state, repo_github_id).await? else {
        return Ok(());
    };

    let Some(issue_record) =
        get_issue_by_repo_and_github_id(state, repo.id, github_issue_id).await?
    else {
        return Ok(());
    };

    if issue_record.status == "completed" || issue_record.status == "cancelled" {
        return Ok(());
    }

    let contributor = ensure_contributor(state, assignee_id, assignee_login).await?;
    upsert_assignment(state, issue_record.id, contributor.id).await?;

    let outcome = state
        .queue
        .enqueue_advance_issue(BountyJobData::new(issue_record.id, "issue-assigned").notifying())
        .await?;

    info!(
        issue = issue_record.github_issue_number,
        assignee = assignee_login,
        outcome = outcome.label(),
        "contributor assigned; bounty automation queued"
    );

    Ok(())
}
