use rust_decimal::Decimal;
use serde_json::Value;
use tracing::info;

use crate::{
    error::AppError,
    modules::{
        bounty::labels::{difficulty_label, get_reward_amount, parse_labels},
        github::{
            auth::post_comment,
            handlers::helpers::{extract_custom_amount, labels_from_payload, sync_repo_balance},
        },
        repo::repository::{
            cancel_issue, delete_assignments_for_issue, get_issue_by_repo_and_github_id,
            get_repo_by_github_id, refund_repo_balance, reserve_repo_balance, try_insert_issue,
            update_issue_reward,
        },
    },
    shared::models::{Difficulty, Repo},
    state::AppState,
};

pub async fn handle_issue_labeled(state: &AppState, payload: &Value) -> Result<(), AppError> {
    let repository = payload.get("repository").ok_or_else(|| {
        AppError::webhook("issues.labeled payload missing repository")
    })?;
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

    let Some(event_label) = event_label else {
        return Ok(());
    };

    let difficulty_labels = ["low", "medium", "high", "custom"];
    let is_trigger = event_label == "rewarded"
        || difficulty_labels.contains(&event_label.as_str())
        || event_label == "rejected";

    if !is_trigger {
        return Ok(());
    }

    let Some(mut repo) = get_repo_by_github_id(state, repo_github_id).await? else {
        return Ok(());
    };

    if repo.escrow_contract_id.is_none() {
        return Ok(());
    }

    let github_issue_id = issue.get("id").and_then(|v| v.as_i64()).unwrap_or(0);
    let issue_number = issue.get("number").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
    let title = issue
        .get("title")
        .and_then(|v| v.as_str())
        .unwrap_or("Untitled");

    let existing = get_issue_by_repo_and_github_id(state, repo.id, github_issue_id).await?;

    if event_label == "rejected" {
        if let Some(ref existing) = existing {
            if existing.status != "completed" && existing.status != "cancelled" {
                cancel_issue(state, existing.id).await?;
                delete_assignments_for_issue(state, existing.id).await?;
                refund_repo_balance(state, &repo, existing.reward_amount).await?;
                post_comment(
                    state,
                    full_name,
                    issue_number,
                    &format!(
                        "### 🛑 Bounty Cancelled\n\nThis issue was rejected by a maintainer. The **{} USDC** bounty has been returned to the pool.",
                        existing.reward_amount
                    ),
                )
                .await?;
            }
        }
        return Ok(());
    }

    let parsed = parse_labels(&labels_from_payload(issue));
    if !parsed.is_rewarded || parsed.difficulty.is_none() {
        return Ok(());
    }

    let difficulty = parsed.difficulty.unwrap();

    if let Some(ref existing) = existing {
        if existing.status != "pending" {
            return Ok(());
        }
    }

    let custom_amount = if difficulty == Difficulty::Custom {
        let body = issue.get("body").and_then(|v| v.as_str());
        match extract_custom_amount(body) {
            Some(amount) => Some(amount),
            None => {
                if existing.is_none() {
                    post_comment(
                        state,
                        full_name,
                        issue_number,
                        "### ⚠️ Missing Amount\n\n\
                         Custom bounties require an amount. Please comment with `@Trustless-OSS <amount>` to set it.",
                    )
                    .await?;
                }
                return Ok(());
            }
        }
    } else {
        None
    };

    let reward_amount = get_reward_amount(Some(difficulty), &repo, custom_amount);
    let diff_label = difficulty_label(difficulty);

    if let Some(ref existing) = existing {
        update_issue_reward(state, existing.id, reward_amount, diff_label).await?;
        info!(
            repo = full_name,
            issue = issue_number,
            %reward_amount,
            "bounty amount updated"
        );
        post_comment(
            state,
            full_name,
            issue_number,
            &format!(
                "🔄 **Bounty Updated:** **{} USDC** (`{}`)",
                reward_amount, diff_label
            ),
        )
        .await?;
        return Ok(());
    }

    sync_repo_balance(state, &mut repo).await?;

    if reward_amount > Decimal::ZERO && repo.escrow_balance < reward_amount {
        post_comment(
            state,
            full_name,
            issue_number,
            &format!(
                "### ⚠️ Insufficient Balance\n\n\
                 Escrow balance (**{} USDC**) is too low for this **{} USDC** bounty.\n\n\
                 [**Top Up Escrow →**]({}/dashboard)",
                repo.escrow_balance, reward_amount, state.config.app_url
            ),
        )
        .await?;
        return Ok(());
    }

    if try_insert_issue(
        state,
        repo.id,
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

    reserve_repo_balance(state, &repo, reward_amount).await?;

    let contract_id = repo.escrow_contract_id.as_deref().unwrap_or("");
    post_comment(
        state,
        full_name,
        issue_number,
        &format!(
            "### 💰 Bounty Created!\n\n\
             | Reward | Level | Escrow |\n\
             | :--- | :--- | :--- |\n\
             | **{} USDC** | `{}` | [View On-Chain →](https://viewer.trustlesswork.com/{}) |\n\n\
             Assign a contributor to lock the funds.",
            reward_amount, diff_label, contract_id
        ),
    )
    .await?;

    info!(
        repo = full_name,
        issue = issue_number,
        %reward_amount,
        "bounty issue created"
    );

    Ok(())
}
