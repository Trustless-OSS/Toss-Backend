use rust_decimal::prelude::ToPrimitive;
use serde_json::{json, Value};
use tracing::{error, info, warn};

use crate::{
    error::AppError,
    modules::{
        bounty::repository::{
            get_assignment_for_issue, get_issue_by_repo_and_github_id,
            get_issue_by_repo_and_number, update_assignment_payout_status,
            update_assignment_pr_merge, update_issue_status,
        },
        contributor::repository::get_contributor_by_github_id,
        escrow::service::{push_milestone_on_chain, release_escrow_milestone},
        github::{
            auth::post_comment,
            handlers::helpers::{
                dispute_milestone, explorer_tx_url, extract_issue_number, has_rejected_label,
                labels_from_payload, maintainer_github_id, resolve_milestone_dispute,
                split_amounts,
            },
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
    let Some(mut issue) = get_issue_by_repo_and_number(state, repo.id, issue_number).await? else {
        return Ok(());
    };
    if issue.status != "pending" && issue.status != "active" {
        return Ok(());
    }

    let Some((assignment, contributor)) = get_assignment_for_issue(state, issue.id).await? else {
        return Ok(());
    };
    if assignment.payout_status == "released" {
        return Ok(());
    }
    let pr_author_id = pr
        .get("user")
        .and_then(|user| user.get("id"))
        .and_then(Value::as_i64);
    let assigned_github_id = contributor.as_ref().map(|value| value.github_user_id);
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

    if issue.status == "pending" {
        let Some(contributor) = contributor.as_ref() else {
            return Ok(());
        };
        let Some(payout_address) = contributor
            .payout_address
            .as_deref()
            .or(contributor.stellar_wallet.as_deref())
        else {
            warn!(
                issue = issue_number,
                "merged PR contributor has no payout wallet"
            );
            post_comment(
                state,
                full_name,
                issue_number,
                &format!(
                    "⚠️ Payout is waiting for @{} to connect a wallet. Connect it and use `@Trustless-OSS /retry`.",
                    contributor.github_username
                ),
            )
            .await?;
            return Ok(());
        };

        push_milestone_on_chain(
            state,
            &repo,
            &issue,
            payout_address,
            contributor.payout_chain.as_deref().unwrap_or("stellar"),
        )
        .await?;
        let Some(updated) =
            get_issue_by_repo_and_github_id(state, repo.id, issue.github_issue_id).await?
        else {
            return Err(AppError::database(
                "Issue disappeared after pushing its milestone",
            ));
        };
        issue = updated;
    }

    update_assignment_pr_merge(state, assignment.id, pr_number).await?;

    if let Some(percentage) = assignment
        .completion_percentage
        .filter(|value| *value > rust_decimal::Decimal::ZERO)
        .filter(|value| *value < rust_decimal::Decimal::from(100))
        .and_then(|value| value.to_i32())
    {
        release_split_payout(
            state,
            full_name,
            issue_number,
            &repo,
            &issue,
            &assignment,
            contributor.as_ref(),
            percentage,
        )
        .await?;
        return Ok(());
    }

    match release_escrow_milestone(state, &repo, &issue).await {
        Ok(tx_hash) => {
            update_assignment_payout_status(state, assignment.id, "released").await?;
            update_issue_status(state, issue.id, "completed", None).await?;
            let username = contributor
                .as_ref()
                .map(|value| value.github_username.as_str())
                .unwrap_or("contributor");
            let contract_id = repo.escrow_contract_id.as_deref().unwrap_or("");
            let explorer_url = explorer_tx_url(state, &tx_hash, contract_id);
            post_comment(
                state,
                full_name,
                issue_number,
                &format!(
                    "### 🎉 Bounty Released!\n\n\
                     **{} USDC** has been sent to @{username}.\n\n\
                     | Recipient | Amount | Status |\n\
                     | :--- | :--- | :--- |\n\
                     | @{username} | {} USDC | [View Transaction]({explorer_url}) |\n\n\
                     Thanks for your contribution! 🚀",
                    issue.reward_amount, issue.reward_amount
                ),
            )
            .await?;
        }
        Err(release_error) => {
            error!(%release_error, issue = issue_number, "bounty release failed after PR merge");
            update_assignment_payout_status(state, assignment.id, "failed").await?;
            post_comment(
                state,
                full_name,
                issue_number,
                &format!(
                    "⚠️ Bounty release failed: {release_error}\n\nUse the dashboard retry button or `@Trustless-OSS /retry`."
                ),
            )
            .await?;
        }
    }

    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn release_split_payout(
    state: &AppState,
    full_name: &str,
    issue_number: i32,
    repo: &crate::shared::models::Repo,
    issue: &crate::shared::models::Issue,
    assignment: &crate::shared::models::Assignment,
    contributor: Option<&crate::shared::models::Contributor>,
    percentage: i32,
) -> Result<(), AppError> {
    let Some(contributor) = contributor else {
        return Ok(());
    };
    let maintainer_id = maintainer_github_id(repo);
    let maintainer = get_contributor_by_github_id(state, maintainer_id).await?;
    let Some(maintainer_wallet) = maintainer.and_then(|value| value.stellar_wallet) else {
        post_comment(
            state,
            full_name,
            issue_number,
            &format!(
                "⚠️ Partial payment is waiting for the maintainer to connect a Stellar wallet. [Connect here →]({}/connect)",
                state.config.app_url
            ),
        )
        .await?;
        return Ok(());
    };
    let Some(contributor_wallet) = contributor.stellar_wallet.as_deref() else {
        post_comment(
            state,
            full_name,
            issue_number,
            &format!(
                "⚠️ Partial payment is waiting for @{} to connect a Stellar wallet.",
                contributor.github_username
            ),
        )
        .await?;
        return Ok(());
    };
    let Some(milestone_index) = issue.milestone_index else {
        return Err(AppError::internal(
            "Cannot release split payout without a milestone index",
        ));
    };
    let contract_id = repo
        .escrow_contract_id
        .as_deref()
        .ok_or_else(|| AppError::bad_request("No escrow deployed"))?;
    let (contributor_amount, maintainer_amount) = split_amounts(issue.reward_amount, percentage);

    let payout_result = async {
        dispute_milestone(
            state,
            contract_id,
            milestone_index,
            &state.config.platform_stellar_public_key,
        )
        .await?;
        let distributions = if contributor_wallet == maintainer_wallet {
            vec![json!({ "address": maintainer_wallet, "amount": issue.reward_amount })]
        } else {
            vec![
                json!({ "address": contributor_wallet, "amount": contributor_amount }),
                json!({ "address": maintainer_wallet, "amount": maintainer_amount }),
            ]
        };
        resolve_milestone_dispute(state, repo, milestone_index, distributions).await
    }
    .await;

    if let Err(release_error) = payout_result {
        error!(%release_error, issue = issue_number, "partial payout failed after PR merge");
        update_assignment_payout_status(state, assignment.id, "failed").await?;
        post_comment(
            state,
            full_name,
            issue_number,
            &format!(
                "⚠️ Partial payment failed: {release_error}\n\nUse the dashboard retry button."
            ),
        )
        .await?;
        return Ok(());
    }

    update_assignment_payout_status(state, assignment.id, "released").await?;
    update_issue_status(state, issue.id, "completed", None).await?;
    post_comment(
        state,
        full_name,
        issue_number,
        &format!(
            "### ✅ Payout Released ({percentage}%)\n\n\
             | Recipient | Amount | Role |\n\
             | :--- | :--- | :--- |\n\
             | @{} | **{contributor_amount} USDC** | Contributor |\n\
             | Maintainer | **{maintainer_amount} USDC** | Refund |\n\n\
             [View Escrow](https://viewer.trustlesswork.com/{contract_id})",
            contributor.github_username
        ),
    )
    .await?;

    Ok(())
}
