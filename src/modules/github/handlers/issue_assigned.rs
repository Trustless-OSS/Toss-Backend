use serde_json::Value;

use crate::{
    error::AppError,
    modules::{
        escrow::trustless_work::milestone::push_milestone_on_chain,
        github::auth::post_comment,
        repo::repository::{
            ensure_contributor, get_issue_by_repo_and_github_id, get_repo_by_github_id,
            upsert_assignment,
        },
    },
    state::AppState,
};

pub async fn handle_issue_assigned(state: &AppState, payload: &Value) -> Result<(), AppError> {
    let repository = payload.get("repository").ok_or_else(|| {
        AppError::webhook("issues.assigned payload missing repository")
    })?;
    let issue = payload
        .get("issue")
        .ok_or_else(|| AppError::webhook("issues.assigned payload missing issue"))?;
    let assignee = payload
        .get("assignee")
        .ok_or_else(|| AppError::webhook("issues.assigned payload missing assignee"))?;

    let repo_github_id = repository.get("id").and_then(|v| v.as_i64()).unwrap_or(0);
    let full_name = repository
        .get("full_name")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let github_issue_id = issue.get("id").and_then(|v| v.as_i64()).unwrap_or(0);
    let issue_number = issue.get("number").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
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

    let payout_address = contributor
        .payout_address
        .as_deref()
        .or(contributor.stellar_wallet.as_deref());

    if let Some(payout_address) = payout_address {
        let payout_chain = contributor.payout_chain.as_deref().unwrap_or("stellar");
        push_milestone_on_chain(
            state,
            &repo,
            &issue_record,
            payout_address,
            payout_chain,
        )
        .await?;

        let contract_id = repo.escrow_contract_id.as_deref().unwrap_or("");
        post_comment(
            state,
            full_name,
            issue_number,
            &format!(
                "### 🚀 Contributor Assigned\n\n\
                 **{} USDC** has been locked for @{}.\n\n\
                 | Status | Reward | Escrow |\n\
                 | :--- | :--- | :--- |\n\
                 | 🔒 **Locked** | {} USDC | [View On-Chain →](https://viewer.trustlesswork.com/{}) |\n\n\
                 Merge the PR to release funds.",
                issue_record.reward_amount, assignee_login, issue_record.reward_amount, contract_id
            ),
        )
        .await?;
    } else {
        let connect_url = format!(
            "{}/connect?issue={}&repo={}",
            state.config.app_url, github_issue_id, repo_github_id
        );
        post_comment(
            state,
            full_name,
            issue_number,
            &format!(
                "### 🔑 Wallet Required\n\n\
                 Hey @{}, you've been assigned this **{} USDC** bounty!\n\n\
                 To lock the funds, please connect your Stellar wallet:\n\
                 [**Connect Wallet →**]({})",
                assignee_login, issue_record.reward_amount, connect_url
            ),
        )
        .await?;
    }

    Ok(())
}
