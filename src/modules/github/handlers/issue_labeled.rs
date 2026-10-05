use rust_decimal::Decimal;
use serde_json::Value;
use tracing::{info, warn};

use crate::{
    error::AppError,
    modules::{
        bounty::labels::{difficulty_label, get_reward_amount, parse_labels},
        bounty::repository::{
            cancel_issue, create_bounty_and_reserve_balance, create_issue_and_reserve_balance,
            delete_assignments_for_issue, get_issue_by_repo_and_github_id,
            update_pending_issue_reward,
        },
        escrow::repository::refund_repo_balance,
        github::{
            auth::post_comment,
            comments,
            handlers::helpers::{extract_manual_amount, labels_from_payload, sync_repo_balance},
        },
        notification::{
            kinds::{Kind, Notify},
            service::Service,
        },
        repo::repository::get_repo_by_github_id,
    },
    shared::models::Difficulty,
    state::AppState,
};

pub async fn handle_issue_labeled(state: &AppState, payload: &Value) -> Result<(), AppError> {
    let repository = payload
        .get("repository")
        .ok_or_else(|| AppError::webhook("issues.labeled payload missing repository"))?;

    let issue = payload
        .get("issue")
        .ok_or_else(|| AppError::webhook("issues.labeled payload missing issue"))?;

    let repo_github_id = repository
        .get("id")
        .and_then(|v| v.as_i64())
        .ok_or_else(|| AppError::webhook("repository.id missing"))?;

    let full_name = repository
        .get("full_name")
        .and_then(|v| v.as_str())
        .ok_or_else(|| AppError::webhook("repository.full_name missing"))?;

    let event_label = payload
        .get("label")
        .and_then(|label| label.get("name"))
        .and_then(|name| name.as_str())
        .map(str::to_ascii_lowercase);

    let difficulty_labels = ["low", "medium", "high", "bonus", "manual"];

    let is_opened = payload.get("action").and_then(Value::as_str) == Some("opened");
    let is_trigger = event_label.as_ref().is_some_and(|label| {
        label == "rewarded" || difficulty_labels.contains(&label.as_str()) || label == "rejected"
    }) || is_opened;

    if !is_trigger {
        info!(
            action = payload.get("action").and_then(|v| v.as_str()).unwrap_or(""),
            label = event_label.as_deref().unwrap_or("none"),
            "issue event does not contain a bounty-triggering label"
        );
        return Ok(());
    }

    let Some(mut repo) = get_repo_by_github_id(state, repo_github_id).await? else {
        warn!(
            github_repo_id = repo_github_id,
            repo = full_name,
            "bounty label ignored because repository is not connected"
        );
        return Ok(());
    };

    if repo.escrow_contract_id.is_none() {
        warn!(
            repo = full_name,
            "bounty label ignored because repository escrow is not deployed"
        );
        return Ok(());
    }

    let github_issue_id = issue.get("id").and_then(|v| v.as_i64()).unwrap_or(0);
    let issue_number = issue.get("number").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
    let title = issue
        .get("title")
        .and_then(|v| v.as_str())
        .unwrap_or("Untitled");

    let existing = get_issue_by_repo_and_github_id(state, repo.id, github_issue_id).await?;

    if event_label.as_deref() == Some("rejected") {
        if let Some(ref existing) = existing {
            if existing.status != "paid" && existing.status != "cancelled" {
                // Send BountyRejected notification to assignee
                if let Some(assignee_id) = existing.assignee_id {
                    let n = Notify {
                        recipient: assignee_id,
                        kind: Kind::BountyRejected,
                        title: format!("Bounty Rejected: #{}", issue_number),
                        body: format!("Your work on issue #{} has been rejected", issue_number),
                        ref_id: Some(existing.id),
                        data: serde_json::json!({
                            "repoId": repo.id,
                            "issueNumber": issue_number,
                        }),
                        dedupe_key: format!("rejected:{}", existing.id),
                        actor: None,
                    };
                    Service::notify_contributor_quiet(state, n).await;
                }

                cancel_issue(state, existing.id).await?;
                delete_assignments_for_issue(state, existing.id).await?;
                refund_repo_balance(state, &repo, existing.reward_amount).await?;
                let reward = existing.reward_amount.unwrap_or_default();
                post_comment(
                    state,
                    full_name,
                    issue_number,
                    &comments::bounty_cancelled(reward),
                )
                .await?;
            }
        }
        return Ok(());
    }

    let parsed = parse_labels(&labels_from_payload(issue));
    if !parsed.is_rewarded {
        info!(
            repo = full_name,
            issue = issue_number,
            "bounty label ignored; issue is missing `rewarded`"
        );
        return Ok(());
    }

    if let Some(ref existing) = existing {
        // Once a milestone is locked, changing the reward would leave the database
        // and escrow out of sync. Before that point, a maintainer may still choose
        // or change the amount, including after assigning a contributor.
        if existing.status != "open"
            && !(existing.status == "assigned" && existing.milestone_index.is_none())
        {
            return Ok(());
        }
    }

    let Some(difficulty) = parsed.difficulty else {
        if existing.is_some() {
            return Ok(());
        }

        let Some(created) = create_bounty_and_reserve_balance(
            state,
            &repo,
            github_issue_id,
            issue_number,
            title,
            Decimal::ZERO,
            None,
        )
        .await?
        else {
            return Ok(());
        };

        post_comment(
            state,
            full_name,
            issue_number,
            comments::UNPRICED_BOUNTY_CREATED,
        )
        .await?;
        info!(repo = full_name, issue = issue_number, bounty = %created.id, "unpriced bounty issue created");
        return Ok(());
    };

    let manual_amount = if difficulty == Difficulty::Manual {
        let body = issue.get("body").and_then(|v| v.as_str());
        extract_manual_amount(body)
    } else {
        None
    };

    let reward_amount = get_reward_amount(Some(difficulty), &repo, manual_amount);
    let diff_label = difficulty_label(difficulty);

    if difficulty == Difficulty::Manual && manual_amount.is_none() {
        if existing.is_none() {
            // Create the bounty in an unpriced state; the amount can be supplied
            // later from an issue comment without requiring a new label event.
            create_bounty_and_reserve_balance(
                state,
                &repo,
                github_issue_id,
                issue_number,
                title,
                Decimal::ZERO,
                None,
            )
            .await?;
        }
        post_comment(
            state,
            full_name,
            issue_number,
            comments::MANUAL_AMOUNT_NEEDED,
        )
        .await?;
        return Ok(());
    }

    if let Some(ref existing) = existing {
        if !update_pending_issue_reward(state, &repo, existing.id, reward_amount, diff_label)
            .await?
        {
            // Send InsufficientFunds notification to maintainers
            let n = Notify {
                recipient: repo.id, // Placeholder; Service::notify_maintainers will set actual recipients
                kind: Kind::InsufficientFunds,
                title: "Insufficient Escrow Balance".to_string(),
                body: format!(
                    "Cannot update bounty for issue #{} due to insufficient escrow balance",
                    issue_number
                ),
                ref_id: Some(existing.id),
                data: serde_json::json!({
                    "repoId": repo.id,
                    "issueNumber": issue_number,
                }),
                dedupe_key: format!("insufficient:{}:{}", github_issue_id, reward_amount),
                actor: None,
            };
            Service::notify_maintainers_quiet(state, repo.id, n).await;

            post_comment(
                state,
                full_name,
                issue_number,
                comments::BOUNTY_UPDATE_FAILED,
            )
            .await?;
            return Ok(());
        }
        info!(repo = full_name, issue = issue_number, %reward_amount, "bounty amount updated");

        // Send AmountUpdated notification to assignee
        if let Some(assignee_id) = existing.assignee_id {
            let n = Notify {
                recipient: assignee_id,
                kind: Kind::AmountUpdated,
                title: format!("Bounty Amount Updated: #{}", issue_number),
                body: format!(
                    "The reward for issue #{} has been updated to {} USDC",
                    issue_number, reward_amount
                ),
                ref_id: Some(existing.id),
                data: serde_json::json!({
                    "repoId": repo.id,
                    "issueNumber": issue_number,
                    "amount": reward_amount.to_string(),
                }),
                dedupe_key: format!("amount:{}:{}", existing.id, reward_amount),
                actor: None,
            };
            Service::notify_contributor_quiet(state, n).await;
        }

        post_comment(
            state,
            full_name,
            issue_number,
            &comments::labeled_bounty_updated(reward_amount, diff_label),
        )
        .await?;
        if existing.status == "assigned" && existing.milestone_index.is_none() {
            state
                .queue
                .enqueue_advance_issue(
                    crate::infra::queue::BountyJobData::new(existing.id, "reward-configured")
                        .notifying(),
                )
                .await?;
        }
        return Ok(());
    }

    sync_repo_balance(state, &mut repo).await?;

    let balance = repo.escrow_balance.unwrap_or_default();
    if reward_amount > Decimal::ZERO && balance < reward_amount {
        warn!(
            repo = full_name,
            issue = issue_number,
            available_balance = %balance,
            required_reward = %reward_amount,
            "bounty not created because escrow balance is insufficient"
        );

        // Send InsufficientFunds notification to maintainers
        let n = Notify {
            recipient: repo.id, // Placeholder; Service::notify_maintainers will set actual recipients
            kind: Kind::InsufficientFunds,
            title: "Insufficient Escrow Balance".to_string(),
            body: format!(
                "Cannot create bounty for issue #{}: balance {} < required {}",
                issue_number, balance, reward_amount
            ),
            ref_id: None,
            data: serde_json::json!({
                "repoId": repo.id,
                "issueNumber": issue_number,
            }),
            dedupe_key: format!("insufficient:{}:{}", github_issue_id, reward_amount),
            actor: None,
        };
        Service::notify_maintainers_quiet(state, repo.id, n).await;

        post_comment(
            state,
            full_name,
            issue_number,
            &comments::labeled_insufficient_balance(balance, reward_amount, &state.config.app_url),
        )
        .await?;
        return Ok(());
    }

    if create_issue_and_reserve_balance(
        state,
        &repo,
        github_issue_id,
        issue_number,
        title,
        reward_amount,
        diff_label,
    )
    .await?
    .is_none()
    {
        return Ok(());
    }

    let contract_id = repo.escrow_contract_id.as_deref().unwrap_or("");
    post_comment(
        state,
        full_name,
        issue_number,
        &comments::labeled_bounty_created(reward_amount, diff_label, contract_id),
    )
    .await?;

    info!(repo = full_name, issue = issue_number, %reward_amount, "bounty issue created");
    Ok(())
}
