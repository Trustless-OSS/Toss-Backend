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
        contributor::repository::{get_profile_by_github_id, get_wallets_for_profile},
        escrow::repository::refund_repo_balance,
        github::{
            auth::post_comment,
            comments,
            handlers::helpers::{
                cancel_bounty_with_refund, dispute_milestone, extract_issue_number,
                extract_manual_amount, is_help_command, is_privileged_association,
                is_reject_command, is_retry_command, is_wallet_command, resolve_milestone_dispute,
                split_amounts, sync_repo_balance, work_completion_percentage,
            },
        },
        repo::repository::get_repo_by_github_id,
    },
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
            &comments::wallet_update(commenter_login, &connect_url),
        )
        .await?;
        return Ok(());
    }

    if is_help_command(body) {
        post_comment(state, full_name, issue_number, comments::HELP).await?;
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

    let reward = issue.reward_amount.unwrap_or_default();

    if issue.status != "assigned" {
        if is_reject_command(body) && issue.status != "paid" && issue.status != "cancelled" {
            cancel_bounty_with_refund(
                state,
                &repo,
                issue.id,
                issue.reward_amount.unwrap_or_default(),
            )
            .await?;
            post_comment(
                state,
                full_name,
                target_number,
                &comments::bounty_cancelled(reward),
            )
            .await?;
        }
        return Ok(());
    }

    let Some((_assignment, contributor)) = get_assignment_for_issue(state, issue.id).await? else {
        return Ok(());
    };
    let Some(contributor) = contributor else {
        return Ok(());
    };

    if pr_author_id.is_some_and(|author_id| author_id != contributor.github_id) {
        post_comment(
            state,
            full_name,
            comment_issue_number,
            &comments::pr_author_mismatch(target_number),
        )
        .await?;
        return Ok(());
    }

    let Some(milestone_index) = issue.milestone_index else {
        if is_reject_command(body) {
            cancel_bounty_with_refund(state, &repo, issue.id, reward).await?;
            post_comment(
                state,
                full_name,
                target_number,
                &comments::bounty_cancelled(reward),
            )
            .await?;
        } else {
            tracing::warn!(issue = target_number, "active issue has no milestone index");
        }
        return Ok(());
    };

    // The maintainer issuing the command is the wallet recipient. GitHub includes
    // the comment author's numeric ID in every issue-comment webhook.
    let maintainer_id = comment_user.get("id").and_then(Value::as_i64);
    let maintainer_wallet = if let Some(maintainer_id) = maintainer_id {
        if let Some(profile) = get_profile_by_github_id(state, maintainer_id).await? {
            let wallets = get_wallets_for_profile(state, profile.id).await?;
            wallets
                .into_iter()
                .find(|w| w.chain == "stellar")
                .map(|w| w.address)
        } else {
            None
        }
    } else {
        None
    };

    let Some(maintainer_wallet) = maintainer_wallet else {
        let login = comment_user
            .get("login")
            .and_then(Value::as_str)
            .unwrap_or("maintainer");
        post_comment(
            state,
            full_name,
            comment_issue_number,
            &comments::maintainer_wallet_required(login, &state.config.app_url),
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
                comments::INVALID_SPLIT,
            )
            .await?;
            return Ok(());
        }

        let contributor_wallets = get_wallets_for_profile(state, contributor.id).await?;
        let contributor_stellar = contributor_wallets
            .iter()
            .find(|w| w.chain == "stellar")
            .map(|w| w.address.clone());

        if contributor_stellar.is_none() {
            post_comment(
                state,
                full_name,
                comment_issue_number,
                &comments::contributor_wallet_missing(&contributor.username),
            )
            .await?;
            return Ok(());
        }

        update_assignment_completion_percentage(state, issue.id, Decimal::from(percentage)).await?;
        let (contributor_amount, maintainer_amount) = split_amounts(reward, percentage);
        post_comment(
            state,
            full_name,
            target_number,
            &comments::payout_intent_saved(
                percentage,
                contributor_amount,
                maintainer_amount,
                &contributor.username,
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
            vec![json!({ "address": maintainer_wallet, "amount": reward })],
        )
        .await?;
        update_issue_status(state, issue.id, "cancelled", None).await?;
        update_assignment_payout_status(state, issue.id, "failed").await?;
        refund_repo_balance(state, &repo, issue.reward_amount).await?;

        post_comment(
            state,
            full_name,
            target_number,
            &comments::bounty_rejected(reward, contract_id),
        )
        .await?;
    }

    Ok(())
}

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

    post_comment(state, full_name, issue_number, comments::RETRYING_BOUNTY).await?;

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
        let can_update = existing.status == "open"
            || (existing.status == "assigned" && existing.milestone_index.is_none());
        if can_update {
            if !update_pending_issue_reward(state, &repo, existing.id, manual_amount, "manual")
                .await?
            {
                post_comment(
                    state,
                    full_name,
                    issue_number,
                    comments::BOUNTY_UPDATE_FAILED,
                )
                .await?;
                return Ok(());
            }
            post_comment(
                state,
                full_name,
                issue_number,
                &comments::manual_bounty_updated(manual_amount),
            )
            .await?;
            if existing.status == "assigned" && existing.milestone_index.is_none() {
                state
                    .queue
                    .enqueue_advance_issue(
                        BountyJobData::new(existing.id, "reward-configured").notifying(),
                    )
                    .await?;
            }
        }
        return Ok(());
    }

    if let Err(error) = sync_repo_balance(state, &mut repo).await {
        error!(%error, "failed to sync escrow balance before manual bounty creation");
    }
    let balance = repo.escrow_balance.unwrap_or_default();
    if balance < manual_amount {
        post_comment(
            state,
            full_name,
            issue_number,
            &comments::manual_insufficient_balance(balance, manual_amount, &state.config.app_url),
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
        &comments::manual_bounty_created(manual_amount, contract_id, commenter_login),
    )
    .await?;

    Ok(())
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
