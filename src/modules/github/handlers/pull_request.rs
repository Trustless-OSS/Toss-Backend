use serde_json::Value;
use tracing::info;

use crate::{
    error::AppError,
    infra::queue::BountyJobData,
    modules::{
        bounty::repository::{
            get_assignment_for_issue, get_issue_by_repo_and_number, update_assignment_pr_merge,
        },
        github::{
            auth::post_comment,
            handlers::helpers::{extract_issue_number, has_rejected_label, labels_from_payload},
        },
        repo::repository::get_repo_by_github_id,
    },
    state::AppState,
};

pub async fn handle_pr_merged(state: &AppState, payload: &Value) -> Result<(), AppError> {
    let repository = payload
        .get("repository")
        .ok_or_else(|| AppError::webhook("pull_request payload missing repository"))?;
    let pr = payload
        .get("pull_request")
        .ok_or_else(|| AppError::webhook("pull_request payload missing pull_request"))?;

    let repo_github_id = repository
        .get("id")
        .and_then(Value::as_i64)
        .ok_or_else(|| AppError::webhook("pull_request repository.id missing"))?;
    let full_name = repository
        .get("full_name")
        .and_then(Value::as_str)
        .ok_or_else(|| AppError::webhook("pull_request repository.full_name missing"))?;
    let pr_number =
        pr.get("number")
            .and_then(Value::as_i64)
            .ok_or_else(|| AppError::webhook("pull_request.number missing"))? as i32;

    if has_rejected_label(&labels_from_payload(pr)) {
        info!(pr = pr_number, "rejected PR merged; payout skipped");
        return Ok(());
    }

    let Some(issue_number) = extract_issue_number(pr.get("body").and_then(Value::as_str)) else {
        info!(pr = pr_number, "merged PR has no linked issue in its body");
        return Ok(());
    };

    let Some(repo) = get_repo_by_github_id(state, repo_github_id).await? else {
        return Ok(());
    };
    let Some(bounty) = get_issue_by_repo_and_number(state, repo.id, issue_number).await? else {
        return Ok(());
    };

    if bounty.status == "paid" || bounty.status == "cancelled" {
        return Ok(());
    }

    let Some((assignment, assignee)) = get_assignment_for_issue(state, bounty.id).await? else {
        return Ok(());
    };

    // assignment is the Bounty itself — check it's not already paid
    if assignment.status == "paid" {
        return Ok(());
    }

    let pr_author_id = pr
        .get("user")
        .and_then(|user| user.get("id"))
        .and_then(Value::as_i64);
    let assigned_github_id = assignee.as_ref().map(|p| p.github_id);

    if pr_author_id.is_none() || pr_author_id != assigned_github_id {
        post_comment(
            state,
            full_name,
            issue_number,
            &format!(
                "⚠️ The author of this PR does not match the assigned contributor for issue #{issue_number}. Payout aborted."
            ),
        )
        .await?;
        return Ok(());
    }

    update_assignment_pr_merge(state, assignment.id, pr_number).await?;

    let outcome = state
        .queue
        .enqueue_advance_issue(BountyJobData::new(bounty.id, "pr-merged").notifying())
        .await?;

    info!(
        issue = issue_number,
        pr = pr_number,
        outcome = outcome.label(),
        "merged PR recorded; bounty automation queued"
    );

    Ok(())
}
