/// Thin Axum handler functions.
///
/// Each handler is responsible only for:
///   1. Extracting data from the HTTP request.
///   2. Calling the relevant `service` function.
///   3. Shaping the HTTP response.
///
/// No business logic lives here.
use axum::{extract::State, Json};
use rust_decimal::Decimal;
use tracing::error;

use crate::{
    error::AppError,
    middleware::auth::AuthedUser,
    modules::{
        escrow::{
            dto::{
                CloseEscrowBody, ContractIdResponse, CreateEscrowBody, FundEscrowBody, OkResponse,
                RefundEscrowBody, RefundResponse, SubmitCloseBody, SubmitDeployBody,
                SubmitFundBody, SubmitFundResponse, UnsignedTransactionResponse,
            },
            service,
        },
        github::auth::post_comment,
        repo::repository::{
            get_contributor_by_github_id, get_repo_by_id, is_maintainer, list_issues_to_cancel,
            update_repo_escrow_balance,
        },
    },
    state::AppState,
};

pub async fn create_escrow_unsigned(
    State(state): State<AppState>,
    user: AuthedUser,
    Json(body): Json<CreateEscrowBody>,
) -> Result<Json<UnsignedTransactionResponse>, AppError> {
    let repo = get_repo_by_id(&state, body.repo_id)
        .await?
        .ok_or_else(|| AppError::not_found("Repo not found"))?;

    if !is_maintainer(&state, user.github_id, repo.id).await? {
        return Err(AppError::forbidden(
            "Forbidden: Only maintainers can perform this action",
        ));
    }

    let unsigned_transaction =
        service::create_unsigned_escrow(&state, &repo, &body.maintainer_wallet).await?;

    Ok(Json(UnsignedTransactionResponse {
        unsigned_transaction,
    }))
}

pub async fn submit_deploy(
    State(state): State<AppState>,
    user: AuthedUser,
    Json(body): Json<SubmitDeployBody>,
) -> Result<Json<ContractIdResponse>, AppError> {
    if !is_maintainer(&state, user.github_id, body.repo_id).await? {
        return Err(AppError::forbidden(
            "Forbidden: Only maintainers can perform this action",
        ));
    }

    let contract_id = service::submit_deploy_escrow(&state, body.repo_id, &body.signed_xdr).await?;

    Ok(Json(ContractIdResponse { contract_id }))
}

pub async fn fund_unsigned(
    State(state): State<AppState>,
    user: AuthedUser,
    Json(body): Json<FundEscrowBody>,
) -> Result<Json<UnsignedTransactionResponse>, AppError> {
    if body.amount <= Decimal::ZERO || body.funder_wallet.is_empty() {
        return Err(AppError::bad_request("Invalid amount or funder wallet"));
    }

    let repo = get_repo_by_id(&state, body.repo_id)
        .await?
        .ok_or_else(|| AppError::bad_request("Repo Not Found"))?;

    if repo.escrow_contract_id.is_none() {
        return Err(AppError::bad_request(
            "No escrow deployed for this repository",
        ));
    }

    if !is_maintainer(&state, user.github_id, repo.id).await? {
        return Err(AppError::forbidden(
            "Forbidden: Only maintainers can fund the escrow",
        ));
    }

    let unsigned_transaction =
        service::create_fund_unsigned(&state, &repo, body.amount, &body.funder_wallet).await?;

    Ok(Json(UnsignedTransactionResponse {
        unsigned_transaction,
    }))
}

pub async fn submit_fund(
    State(state): State<AppState>,
    _user: AuthedUser,
    Json(body): Json<SubmitFundBody>,
) -> Result<Json<SubmitFundResponse>, AppError> {
    use crate::modules::escrow::trustless_work::client::tw_fetch;

    tw_fetch(
        &state,
        "/helper/send-transaction",
        reqwest::Method::POST,
        Some(serde_json::json!({ "signedXdr": body.signed_xdr })),
    )
    .await?;

    if let Some(repo) = get_repo_by_id(&state, body.repo_id).await? {
        let new_balance = repo.escrow_balance + body.amount;
        update_repo_escrow_balance(&state, body.repo_id, new_balance, Some(repo.github_repo_id))
            .await?;
        Ok(Json(SubmitFundResponse {
            ok: true,
            new_balance: Some(new_balance),
        }))
    } else {
        Ok(Json(SubmitFundResponse {
            ok: true,
            new_balance: None,
        }))
    }
}

pub async fn refund(
    State(state): State<AppState>,
    user: AuthedUser,
    Json(body): Json<RefundEscrowBody>,
) -> Result<Json<RefundResponse>, AppError> {
    if !is_maintainer(&state, user.github_id, body.repo_id).await? {
        return Err(AppError::forbidden(
            "Forbidden: Only maintainers can refund funds",
        ));
    }

    let maintainer = get_contributor_by_github_id(&state, user.github_id)
        .await?
        .ok_or_else(|| {
            AppError::bad_request("You must link your Stellar wallet before refunding")
        })?;

    let maintainer_wallet = maintainer
        .stellar_wallet
        .as_deref()
        .or(maintainer.payout_address.as_deref())
        .ok_or_else(|| {
            AppError::bad_request("You must link your Stellar wallet before refunding")
        })?;

    let repo = get_repo_by_id(&state, body.repo_id)
        .await?
        .ok_or_else(|| AppError::not_found("Repo or escrow not found"))?;

    if repo.escrow_contract_id.is_none() {
        return Err(AppError::not_found("Repo or escrow not found"));
    }

    let issues_to_cancel = list_issues_to_cancel(&state, repo.id).await?;
    let contract_id = repo.escrow_contract_id.clone().unwrap_or_default();

    let (refunded_amount, cancelled_issues) =
        service::refund_escrow(&state, &repo, maintainer_wallet).await?;

    for issue in &issues_to_cancel {
        let comment = format!(
            "🚫 **Bounty Cancelled.**\n\nThe maintainer has withdrawn funds from the escrow. This bounty is now cancelled.\n\n[View Escrow Contract](https://viewer.trustlesswork.com/{contract_id})"
        );
        if let Err(error) =
            post_comment(&state, &repo.full_name, issue.github_issue_number, &comment).await
        {
            error!(%error, issue = issue.github_issue_number, "failed to post refund cancellation comment");
        }
    }

    Ok(Json(RefundResponse {
        refunded_amount,
        cancelled_issues,
    }))
}

pub async fn close_unsigned(
    State(state): State<AppState>,
    user: AuthedUser,
    Json(body): Json<CloseEscrowBody>,
) -> Result<Json<UnsignedTransactionResponse>, AppError> {
    let repo = get_repo_by_id(&state, body.repo_id)
        .await?
        .ok_or_else(|| AppError::bad_request("No escrow deployed"))?;

    if repo.escrow_contract_id.is_none() {
        return Err(AppError::bad_request("No escrow deployed"));
    }

    if !is_maintainer(&state, user.github_id, repo.id).await? {
        return Err(AppError::forbidden(
            "Forbidden: Only maintainers can close the escrow",
        ));
    }

    let unsigned_transaction =
        service::create_close_unsigned(&state, &repo, &body.maintainer_wallet).await?;

    Ok(Json(UnsignedTransactionResponse {
        unsigned_transaction,
    }))
}

pub async fn submit_close(
    State(state): State<AppState>,
    user: AuthedUser,
    Json(body): Json<SubmitCloseBody>,
) -> Result<Json<OkResponse>, AppError> {
    if !is_maintainer(&state, user.github_id, body.repo_id).await? {
        return Err(AppError::forbidden(
            "Forbidden: Only maintainers can perform this action",
        ));
    }

    service::submit_close_escrow(&state, body.repo_id, &body.signed_xdr).await?;

    Ok(Json(OkResponse { ok: true }))
}
