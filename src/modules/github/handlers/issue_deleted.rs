use serde_json::Value;
use tracing::info;

use crate::{
    error::AppError,
    modules::{
        github::handlers::helpers::{cancel_bounty_with_refund, zero_milestone_on_chain},
        repo::repository::{get_issue_by_repo_and_github_id, get_repo_by_github_id},
    },
    state::AppState,
};

pub async fn handle_issue_deleted(state: &AppState, payload: &Value) -> Result<(), AppError> {
    let repository = payload
        .get("repository")
        .ok_or_else(|| AppError::webhook("issues.deleted payload missing repository"))?;
    let issue = payload
        .get("issue")
        .ok_or_else(|| AppError::webhook("issues.deleted payload missing issue"))?;
    let repo_github_id = repository
        .get("id")
        .and_then(Value::as_i64)
        .ok_or_else(|| AppError::webhook("issues.deleted repository.id missing"))?;
    let github_issue_id = issue
        .get("id")
        .and_then(Value::as_i64)
        .ok_or_else(|| AppError::webhook("issues.deleted issue.id missing"))?;
    let issue_number = issue.get("number").and_then(Value::as_i64).unwrap_or(0);

    let Some(repo) = get_repo_by_github_id(state, repo_github_id).await? else {
        return Ok(());
    };
    let Some(issue_record) =
        get_issue_by_repo_and_github_id(state, repo.id, github_issue_id).await?
    else {
        info!(
            issue = issue_number,
            "deleted GitHub issue had no bounty record"
        );
        return Ok(());
    };
    if issue_record.status == "completed" || issue_record.status == "cancelled" {
        return Ok(());
    }

    if issue_record.status == "active" {
        if let Some(milestone_index) = issue_record.milestone_index {
            zero_milestone_on_chain(state, &repo, milestone_index).await?;
        }
    }

    cancel_bounty_with_refund(state, &repo, issue_record.id, issue_record.reward_amount).await?;
    info!(
        issue = issue_number,
        reward = %issue_record.reward_amount,
        "deleted issue bounty cancelled and reserved balance restored"
    );
    Ok(())
}
