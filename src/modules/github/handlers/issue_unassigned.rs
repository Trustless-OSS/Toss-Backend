use serde_json::Value;
use tracing::{error, info};

use crate::{
    error::AppError,
    modules::{
        bounty::repository::{
            delete_assignments_for_issue, get_issue_by_repo_and_github_id, reset_issue_to_pending,
        },
        github::{auth::post_comment, handlers::helpers::zero_milestone_on_chain},
        repo::repository::get_repo_by_github_id,
    },
    state::AppState,
};

pub async fn handle_issue_unassigned(state: &AppState, payload: &Value) -> Result<(), AppError> {
    let repository = payload
        .get("repository")
        .ok_or_else(|| AppError::webhook("issues.unassigned payload missing repository"))?;
    let issue = payload
        .get("issue")
        .ok_or_else(|| AppError::webhook("issues.unassigned payload missing issue"))?;

    let repo_github_id = repository.get("id").and_then(|v| v.as_i64()).unwrap_or(0);
    let full_name = repository
        .get("full_name")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let github_issue_id = issue.get("id").and_then(|v| v.as_i64()).unwrap_or(0);
    let issue_number = issue.get("number").and_then(|v| v.as_i64()).unwrap_or(0) as i32;

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

    delete_assignments_for_issue(state, issue_record.id).await?;

    if issue_record.status == "active" {
        if let Some(milestone_index) = issue_record.milestone_index {
            if let Err(err) = zero_milestone_on_chain(state, &repo, milestone_index).await {
                error!(%err, "failed to zero out milestone on unassign");
            }
        }
    }

    reset_issue_to_pending(state, issue_record.id).await?;

    post_comment(
        state,
        full_name,
        issue_number,
        &format!(
            "🔄 Contributor unassigned. The milestone has been closed. The bounty of **{} USDC** remains available for the next assignee.",
            issue_record.reward_amount
        ),
    )
    .await?;

    info!(issue = issue_number, "unassigned and closed milestone");
    Ok(())
}
