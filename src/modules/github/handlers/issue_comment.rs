use rust_decimal::Decimal;
use serde_json::{json, Value};
use tracing::{error, info};

use crate::{
    error::AppError,
    infra::queue::BountyJobData,
    modules::{
        bounty::repository::{
            create_issue_and_reserve_balance, get_assignment_for_issue,
            get_issue_by_repo_and_github_id, get_issue_by_repo_and_number,
            update_assignment_completion_percentage, update_assignment_payout_status,
            update_issue_status, update_pending_issue_reward,
        },
        contributor::repository::get_contributor_by_github_id,
        escrow::repository::refund_repo_balance,
        github::{
            auth::post_comment,
            handlers::helpers::{
                cancel_bounty_with_refund, dispute_milestone, extract_issue_number,
                extract_manual_amount, is_help_command, is_privileged_association,
                is_reject_command, is_retry_command, is_wallet_command, maintainer_github_id,
                refresh_repo, resolve_milestone_dispute, split_amounts, sync_repo_balance,
                work_completion_percentage,
            },
        },
        repo::repository::get_repo_by_github_id,
    },
    shared::models::Issue,
    state::AppState,
};

pub async fn handle_issue_comment_created(
    state: &AppState,
    payload: &Value,
) -> Result<(), AppError> {
    let repository = required_object(payload, "repository")?;
    let issue_payload = required_object(payload, "issue")?;
    let comment = required_object(payload, "comment")?;

    let repo_github_id = required_i64(repository, "id")?;
    let full_name = required_str(repository, "full_name")?;
    let github_issue_id = required_i64(issue_payload, "id")?;
    let issue_number = required_i64(issue_payload, "number")? as i32;
    let issue_title = issue_payload
        .get("title")
        .and_then(Value::as_str)
        .unwrap_or("Untitled");
    let body = required_str(comment, "body")?;
    let comment_user = required_object(comment, "user")?;
    let commenter_login = required_str(comment_user, "login")?;

    if comment_user.get("type").and_then(Value::as_str) == Some("Bot") {
        return Ok(());
    }

    let association = comment
        .get("author_association")
        .and_then(Value::as_str)
        .unwrap_or("");
    let privileged = is_privileged_association(association);

    if privileged && (work_completion_percentage(body).is_some() || is_reject_command(body)) {
        handle_payout_command(
            state,
            repo_github_id,
            full_name,
            issue_payload,
            issue_number,
            comment_user,
            body,
        )
        .await?;
        return Ok(());
    }

    if is_wallet_command(body) {
        let connect_url = format!(
            "{}/connect?issue={github_issue_id}&repo={repo_github_id}",
            state.config.app_url
        );
        post_comment(
            state,
            full_name,
            issue_number,
            &format!(
                "👋 Hey @{commenter_login}! You can update your wallet address here: [**Update Wallet →**]({connect_url})"
            ),
        )
        .await?;
        return Ok(());
    }

    if is_help_command(body) {
        post_comment(
            state,
            full_name,
            issue_number,
            "🤖 **Trustless-OSS Bot Commands**\n\n\
             **For Maintainers:**\n\
             - `@Trustless-OSS <amount>`: Set a manual bounty\n\
             - `@Trustless-OSS /pay <percentage>`: Split bounty on merge\n\
             - `@Trustless-OSS /reject`: Reject work and refund the escrow\n\
             - `@Trustless-OSS /retry`: Force a re-check (rarely needed — payouts continue automatically)\n\n\
             **For Contributors:**\n\
             - `@Trustless-OSS /wallet`: Connect or update your wallet\n\n\
             **General:**\n\
             - `@Trustless-OSS /help`: Show this command list",
        )
        .await?;
        return Ok(());
    }

    if privileged && is_retry_command(body) {
        retry_bounty(
            state,
            repo_github_id,
            full_name,
            github_issue_id,
            issue_number,
        )
        .await?;
        return Ok(());
    }

    if !privileged {
        return Ok(());
    }

    let Some(manual_amount) = extract_manual_amount(Some(body)) else {
        return Ok(());
    };

    create_or_update_manual_bounty(
        state,
        repo_github_id,
        full_name,
        github_issue_id,
        issue_number,
        issue_title,
        commenter_login,
        manual_amount,
    )
    .await
}

async fn handle_payout_command(
    state: &AppState,
    repo_github_id: i64,
    full_name: &str,
    issue_payload: &Value,
    comment_issue_number: i32,
    comment_user: &Value,
    body: &str,
) -> Result<(), AppError> {
    let Some(repo) = get_repo_by_github_id(state, repo_github_id).await? else {
        return Ok(());
    };

    let is_pr = issue_payload.get("pull_request").is_some();
    let (target_number, pr_author_id) = if is_pr {
        let Some(number) = extract_issue_number(issue_payload.get("body").and_then(Value::as_str))
        else {
            info!("PR comment command ignored because its body has no linked issue");
            return Ok(());
        };
        (
            number,
            issue_payload
                .get("user")
                .and_then(|user| user.get("id"))
                .and_then(Value::as_i64),
        )
    } else {
        (comment_issue_number, None)
    };

    let Some(issue) = get_issue_by_repo_and_number(state, repo.id, target_number).await? else {
        return Ok(());
    };

    if issue.status != "active" {
        if is_reject_command(body) && issue.status != "completed" && issue.status != "cancelled" {
            cancel_bounty_with_refund(state, &repo, issue.id, issue.reward_amount).await?;
            post_comment(
                state,
                full_name,
                target_number,
                &format!(
                    "### 🛑 Bounty Cancelled\n\nThis issue was rejected by a maintainer. The **{} USDC** bounty has been returned to the pool.",
                    issue.reward_amount
                ),
            )
            .await?;
        }
        return Ok(());
    }

    let Some((assignment, contributor)) = get_assignment_for_issue(state, issue.id).await? else {
        return Ok(());
    };
    let Some(contributor) = contributor else {
        return Ok(());
    };

    if pr_author_id.is_some_and(|author_id| author_id != contributor.github_user_id) {
        post_comment(
            state,
            full_name,
            comment_issue_number,
            &format!(
                "⚠️ The author of this PR does not match the assigned contributor for issue #{target_number}."
            ),
        )
        .await?;
        return Ok(());
    }

    let Some(milestone_index) = issue.milestone_index else {
        tracing::warn!(issue = target_number, "active issue has no milestone index");
        return Ok(());
    };

    let maintainer_id = maintainer_github_id(&repo);
    let maintainer = get_contributor_by_github_id(state, maintainer_id).await?;
    let Some(maintainer_wallet) = maintainer.and_then(|value| value.stellar_wallet) else {
        let login = comment_user
            .get("login")
            .and_then(Value::as_str)
            .unwrap_or("maintainer");
        post_comment(
            state,
            full_name,
            comment_issue_number,
            &format!(
                "### 🔑 Wallet Required\n\n@{login}, connect your Stellar wallet before using this command: [**Connect Wallet →**]({}/connect)",
                state.config.app_url
            ),
        )
        .await?;
        return Ok(());
    };

    if let Some(percentage) = work_completion_percentage(body) {
        if !(1..=99).contains(&percentage) {
            post_comment(
                state,
                full_name,
                comment_issue_number,
                "⚠️ **Invalid Split:** Percentage must be between 1 and 99.",
            )
            .await?;
            return Ok(());
        }
        if contributor.stellar_wallet.is_none() {
            post_comment(
                state,
                full_name,
                comment_issue_number,
                &format!(
                    "### 🔑 Contributor Wallet Missing\n\n@{} must connect a Stellar wallet before a split can be configured.",
                    contributor.github_username
                ),
            )
            .await?;
            return Ok(());
        }

        update_assignment_completion_percentage(state, assignment.id, Decimal::from(percentage))
            .await?;
        let (contributor_amount, maintainer_amount) =
            split_amounts(issue.reward_amount, percentage);
        post_comment(
            state,
            full_name,
            target_number,
            &format!(
                "### 📋 Payout Intent Saved ({percentage}%)\n\n\
                 When this PR is merged, the bounty will be split:\n\
                 - **{contributor_amount} USDC** → @{}\n\
                 - **{maintainer_amount} USDC** → maintainer\n\n\
                 _Update anytime with `/pay <percentage>` before merging._",
                contributor.github_username
            ),
        )
        .await?;
        return Ok(());
    }

    if is_reject_command(body) {
        let contract_id = repo
            .escrow_contract_id
            .as_deref()
            .ok_or_else(|| AppError::bad_request("No escrow deployed"))?;
        dispute_milestone(
            state,
            contract_id,
            milestone_index,
            &state.config.platform_stellar_public_key,
        )
        .await?;
        resolve_milestone_dispute(
            state,
            &repo,
            milestone_index,
            vec![json!({
                "address": maintainer_wallet,
                "amount": issue.reward_amount,
            })],
        )
        .await?;
        update_issue_status(state, issue.id, "cancelled", None).await?;
        update_assignment_payout_status(state, assignment.id, "failed").await?;
        refund_repo_balance(state, &repo, issue.reward_amount).await?;

        post_comment(
            state,
            full_name,
            target_number,
            &format!(
                "### 🛑 Bounty Rejected\n\nThe maintainer rejected the work. **{} USDC** has been returned to the maintainer's wallet.\n\n[View Escrow](https://viewer.trustlesswork.com/{contract_id})",
                issue.reward_amount
            ),
        )
        .await?;
    }

    Ok(())
}

/// Emergency `@Trustless-OSS /retry` command.
///
/// Automation no longer depends on this: every documented step advances on its
/// own and transient failures retry with backoff. The command survives as a
/// maintainer escape hatch, and all it does is ask the state machine to run now
/// against live rules.
async fn retry_bounty(
    state: &AppState,
    repo_github_id: i64,
    full_name: &str,
    github_issue_id: i64,
    issue_number: i32,
) -> Result<(), AppError> {
    let Some(repo) = get_repo_by_github_id(state, repo_github_id).await? else {
        return Ok(());
    };
    let Some(issue) = get_issue_by_repo_and_github_id(state, repo.id, github_issue_id).await?
    else {
        return Ok(());
    };

    let outcome = state
        .queue
        .enqueue_advance_issue(BountyJobData::new(issue.id, "comment-retry").notifying())
        .await?;

    info!(
        issue = issue_number,
        outcome = outcome.label(),
        "manual retry command received"
    );

    post_comment(
        state,
        full_name,
        issue_number,
        "🔄 Re-checking this bounty now. Note that you should not normally need this — \
         the payout continues by itself once its conditions are met.",
    )
    .await?;

    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn create_or_update_manual_bounty(
    state: &AppState,
    repo_github_id: i64,
    full_name: &str,
    github_issue_id: i64,
    issue_number: i32,
    issue_title: &str,
    commenter_login: &str,
    manual_amount: Decimal,
) -> Result<(), AppError> {
    let Some(mut repo) = get_repo_by_github_id(state, repo_github_id).await? else {
        return Ok(());
    };
    if repo.escrow_contract_id.is_none() {
        return Ok(());
    }

    let existing = get_issue_by_repo_and_github_id(state, repo.id, github_issue_id).await?;
    if let Some(existing) = existing {
        if existing.status == "pending" {
            if !update_pending_issue_reward(state, &repo, existing.id, manual_amount, "manual")
                .await?
            {
                post_comment(
                    state,
                    full_name,
                    issue_number,
                    "⚠️ The bounty could not be updated because the available escrow balance is too low.",
                )
                .await?;
                return Ok(());
            }
            post_comment(
                state,
                full_name,
                issue_number,
                &format!("🔄 Bounty updated to **{manual_amount} USDC**!"),
            )
            .await?;
        }
        return Ok(());
    }

    if let Err(error) = sync_repo_balance(state, &mut repo).await {
        error!(%error, "failed to sync escrow balance before manual bounty creation");
    }
    if repo.escrow_balance < manual_amount {
        post_comment(
            state,
            full_name,
            issue_number,
            &format!(
                "⚠️ Insufficient escrow balance (**{} USDC**). Need **{manual_amount} USDC**.\n\n[Top up your escrow →]({}/dashboard)",
                repo.escrow_balance, state.config.app_url
            ),
        )
        .await?;
        return Ok(());
    }

    if create_issue_and_reserve_balance(
        state,
        &repo,
        github_issue_id,
        issue_number,
        issue_title,
        manual_amount,
        "manual",
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
        &format!(
            "🎯 Bounty of **{manual_amount} USDC** created by @{commenter_login}!\n\n\
             | Detail | Value |\n|---|---|\n\
             | 💰 Reward | **{manual_amount} USDC** |\n\
             | 📊 Level | `manual` |\n\
             | 📋 Escrow | [View on-chain →](https://viewer.trustlesswork.com/{contract_id}) |\n\n\
             Assign a contributor to get started."
        ),
    )
    .await?;

    Ok(())
}

async fn refresh_repo_issue(state: &AppState, issue: &Issue) -> Result<Option<Issue>, AppError> {
    let Some(repo) = refresh_repo(state, issue.repo_id).await? else {
        return Ok(None);
    };
    get_issue_by_repo_and_github_id(state, repo.id, issue.github_issue_id).await
}

fn required_object<'a>(value: &'a Value, key: &str) -> Result<&'a Value, AppError> {
    value
        .get(key)
        .ok_or_else(|| AppError::webhook(format!("issue_comment payload missing {key}")))
}

fn required_i64(value: &Value, key: &str) -> Result<i64, AppError> {
    value
        .get(key)
        .and_then(Value::as_i64)
        .ok_or_else(|| AppError::webhook(format!("issue_comment payload missing {key}")))
}

fn required_str<'a>(value: &'a Value, key: &str) -> Result<&'a str, AppError> {
    value
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| AppError::webhook(format!("issue_comment payload missing {key}")))
}
