use rust_decimal::Decimal;
use serde_json::Value;
use tracing::info;

use crate::{
    error::AppError,
    infra::queue::BountyJobData,
    modules::{
        bounty::repository::{get_issue_by_repo_and_github_id, upsert_assignment},
        contributor::repository::ensure_contributor,
        github::{auth::post_comment, comments},
        notification::{
            kinds::{Kind, Notify},
            service::Service,
        },
        repo::repository::get_repo_by_github_id,
    },
    state::AppState,
};

pub async fn handle_issue_assigned(state: &AppState, payload: &Value) -> Result<(), AppError> {
    let repository = payload
        .get("repository")
        .ok_or_else(|| AppError::webhook("[Github]:payload missing repository"))?;

    let issue = payload
        .get("issue")
        .ok_or_else(|| AppError::webhook("issues.assigned payload missing issue"))?;

    let assignee = payload
        .get("assignee")
        .ok_or_else(|| AppError::webhook("issues.assigned payload missing assignee"))?;

    let repo_github_id = repository.get("id").and_then(|v| v.as_i64()).unwrap_or(0);
    let github_issue_id = issue.get("id").and_then(|v| v.as_i64()).unwrap_or(0);
    let assignee_github_id = assignee.get("id").and_then(|v| v.as_i64()).unwrap_or(0);
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

    if issue_record.status == "paid" || issue_record.status == "cancelled" {
        return Ok(());
    }

    let profile = ensure_contributor(state, assignee_github_id, assignee_login).await?;
    upsert_assignment(state, issue_record.id, profile.id).await?;

    // Send BountyAssigned notification
    let now_unix = jiff::Timestamp::now().as_second();
    let dedup_suffix = now_unix / 600; // 10-minute window for deduping
    let n = Notify {
        recipient: profile.id,
        kind: Kind::BountyAssigned,
        title: format!("Bounty Assigned: #{}", issue_record.github_issue_number),
        body: format!("You have been assigned to a bounty on {}", repo.full_name),
        ref_id: Some(issue_record.id),
        data: serde_json::json!({
            "repoId": repo.id,
            "issueNumber": issue_record.github_issue_number,
            "amount": issue_record.reward_amount.map(|d| d.to_string()),
        }),
        dedupe_key: format!(
            "assigned:{}:{}:{}",
            issue_record.id, profile.id, dedup_suffix
        ),
        actor: None,
    };
    Service::notify_contributor_quiet(state, n).await;

    if issue_record.reward_amount.unwrap_or(Decimal::ZERO) <= Decimal::ZERO {
        // Send AmountMissing notification to maintainers and assignee
        let n_missing = Notify {
            recipient: profile.id, // Will be ignored for maintainers due to No delivery
            kind: Kind::AmountMissing,
            title: "Bounty Amount Not Set".to_string(),
            body: format!(
                "Contributor assigned to issue #{} but bounty amount is not configured",
                issue_record.github_issue_number
            ),
            ref_id: Some(issue_record.id),
            data: serde_json::json!({
                "repoId": repo.id,
                "issueNumber": issue_record.github_issue_number,
            }),
            dedupe_key: format!("amount-missing:{}", issue_record.id),
            actor: None,
        };

        // Notify assignee (Contributor role, gets BellOnly)
        Service::notify_contributor_quiet(state, n_missing.clone()).await;

        // Notify maintainers (gets BellAndEmail)
        Service::notify_maintainers_quiet(state, repo.id, n_missing).await;

        post_comment(
            state,
            &repo.full_name,
            issue_record.github_issue_number,
            &comments::contributor_assigned_without_amount(assignee_login),
        )
        .await?;
        info!(
            issue = issue_record.github_issue_number,
            assignee = assignee_login,
            "contributor assigned while bounty amount is unconfigured"
        );
        return Ok(());
    }

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
