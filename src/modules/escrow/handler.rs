use axum::{extract::State, Json};
use rust_decimal::Decimal;
use tracing::error;

use crate::{
    error::{AppError, ErrorResponse},
    middleware::auth::AuthedUser,
    modules::{
        bounty::repository::list_issues_to_cancel,
        escrow::repository::update_repo_escrow_funder_wallet,
        escrow::{
            dto::{
                CloseEscrow, ContractIdResponse, CreateEscrow, FundEscrow, OkResponse,
                RefundEscrow, RefundResponse, SubmitClose, SubmitDeploy, SubmitFund,
                SubmitFundResponse, UnsignedTransactionResponse,
            },
            trustless_work::{escrow_service::TrustlessWorkAPI, tx_builder::TxBuilder},
        },
        github::auth::post_comment,
        repo::repository::{get_repo_by_id, is_maintainer},
    },
    state::AppState,
};

#[utoipa::path(
    post,
    path = "/api/escrow/create-unsigned",
    tag = "Escrow",
    security(("bearer_auth" = [])),
    request_body = CreateEscrow,
    responses(
        (status = 200, description = "Unsigned escrow deploy transaction", body = UnsignedTransactionResponse),
        (status = 401, description = "Missing or invalid bearer token", body = ErrorResponse),
        (status = 403, description = "Caller is not a maintainer", body = ErrorResponse),
        (status = 404, description = "Repository not found", body = ErrorResponse),
        (status = 500, description = "Failed to create unsigned deploy transaction", body = ErrorResponse)
    )
)]
pub async fn create_escrow_unsigned(
    State(state): State<AppState>,
    user: AuthedUser,
    Json(body): Json<CreateEscrow>,
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
        TxBuilder::create_escrow(&state, &repo, &body.maintainer_wallet).await?;

    Ok(Json(UnsignedTransactionResponse {
        unsigned_transaction,
    }))
}

#[utoipa::path(
    post,
    path = "/api/escrow/submit-deploy",
    tag = "Escrow",
    security(("bearer_auth" = [])),
    request_body = SubmitDeploy,
    responses(
        (status = 200, description = "Escrow contract deployed", body = ContractIdResponse),
        (status = 401, description = "Missing or invalid bearer token", body = ErrorResponse),
        (status = 403, description = "Caller is not a maintainer", body = ErrorResponse),
        (status = 500, description = "Failed to submit deploy transaction", body = ErrorResponse)
    )
)]
pub async fn submit_deploy(
    State(state): State<AppState>,
    user: AuthedUser,
    Json(body): Json<SubmitDeploy>,
) -> Result<Json<ContractIdResponse>, AppError> {
    if !is_maintainer(&state, user.github_id, body.repo_id).await? {
        return Err(AppError::forbidden(
            "Forbidden: Only maintainers can perform this action",
        ));
    }

    let contract_id = TrustlessWorkAPI::new(state.clone())
        .deploy(body.repo_id, &body.signed_xdr)
        .await?;

    Ok(Json(ContractIdResponse { contract_id }))
}

#[utoipa::path(
    post,
    path = "/api/escrow/fund-unsigned",
    tag = "Escrow",
    security(("bearer_auth" = [])),
    request_body = FundEscrow,
    responses(
        (status = 200, description = "Unsigned escrow fund transaction", body = UnsignedTransactionResponse),
        (status = 400, description = "Invalid amount, wallet, or escrow state", body = ErrorResponse),
        (status = 401, description = "Missing or invalid bearer token", body = ErrorResponse),
        (status = 403, description = "Caller is not a maintainer", body = ErrorResponse),
        (status = 500, description = "Failed to create unsigned fund transaction", body = ErrorResponse)
    )
)]
pub async fn fund_unsigned(
    State(state): State<AppState>,
    user: AuthedUser,
    Json(body): Json<FundEscrow>,
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

    if let Some(existing_funder_wallet) = repo.escrow_funder_wallet.as_deref() {
        if existing_funder_wallet != body.funder_wallet {
            return Err(AppError::bad_request(
                "This escrow must be funded from the original wallet",
            ));
        }
    }

    let unsigned_transaction =
        TxBuilder::fund_escrow(&state, &repo, body.amount, &body.funder_wallet).await?;

    update_repo_escrow_funder_wallet(&state, repo.id, &body.funder_wallet).await?;

    Ok(Json(UnsignedTransactionResponse {
        unsigned_transaction,
    }))
}

#[utoipa::path(
    post,
    path = "/api/escrow/submit-fund",
    tag = "Escrow",
    security(("bearer_auth" = [])),
    request_body = SubmitFund,
    responses(
        (status = 200, description = "Fund transaction submitted and local balance updated", body = SubmitFundResponse),
        (status = 401, description = "Missing or invalid bearer token", body = ErrorResponse),
        (status = 500, description = "Failed to submit fund transaction", body = ErrorResponse)
    )
)]
pub async fn submit_fund(
    State(state): State<AppState>,
    _user: AuthedUser,
    Json(body): Json<SubmitFund>,
) -> Result<Json<SubmitFundResponse>, AppError> {
    let new_balance = TrustlessWorkAPI::new(state.clone())
        .fund(body.repo_id, body.amount, &body.signed_xdr)
        .await?;

    Ok(Json(SubmitFundResponse {
        ok: true,
        new_balance: Some(new_balance),
    }))
}

#[utoipa::path(
    post,
    path = "/api/escrow/refund",
    tag = "Escrow",
    security(("bearer_auth" = [])),
    request_body = RefundEscrow,
    responses(
        (status = 200, description = "Escrow refunded and open bounties cancelled", body = RefundResponse),
        (status = 400, description = "Escrow has no recorded funding wallet", body = ErrorResponse),
        (status = 401, description = "Missing or invalid bearer token", body = ErrorResponse),
        (status = 403, description = "Caller is not a maintainer", body = ErrorResponse),
        (status = 404, description = "Repository or escrow not found", body = ErrorResponse),
        (status = 500, description = "Failed to refund escrow", body = ErrorResponse)
    )
)]
pub async fn refund(
    State(state): State<AppState>,
    user: AuthedUser,
    Json(body): Json<RefundEscrow>,
) -> Result<Json<RefundResponse>, AppError> {
    if !is_maintainer(&state, user.github_id, body.repo_id).await? {
        return Err(AppError::forbidden(
            "Forbidden: Only maintainers can refund funds",
        ));
    }

    let repo = get_repo_by_id(&state, body.repo_id)
        .await?
        .ok_or_else(|| AppError::not_found("Repo or escrow not found"))?;

    if repo.escrow_contract_id.is_none() {
        return Err(AppError::not_found("Repo or escrow not found"));
    }

    let funder_wallet = repo
        .escrow_funder_wallet
        .as_deref()
        .ok_or_else(|| AppError::bad_request("This escrow has no recorded funding wallet"))?;

    let issues_to_cancel = list_issues_to_cancel(&state, repo.id).await?;
    let contract_id = repo.escrow_contract_id.clone().unwrap_or_default();

    let (refunded_amount, cancelled_issues) = TrustlessWorkAPI::new(state.clone())
        .refund(&repo, funder_wallet)
        .await?;

    for issue in &issues_to_cancel {
        let comment = format!(
            "## 🚫 Bounty Cancelled\n\n\
             > **Status:** Cancelled  \n\
             > **Reason:** A repository maintainer withdrew the escrowed funds.\n\n\
             ### What this means\n\n\
             - No further work or pull requests for this issue are eligible for a payout.\n\
             - The bounty will not be reopened unless the repository maintainer creates a new one.\n\n\
             ### Escrow\n\n\
             [View escrow contract →](https://viewer.trustlesswork.com/{contract_id})"
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

#[utoipa::path(
    post,
    path = "/api/escrow/close-unsigned",
    tag = "Escrow",
    security(("bearer_auth" = [])),
    request_body = CloseEscrow,
    responses(
        (status = 200, description = "Unsigned escrow close transaction", body = UnsignedTransactionResponse),
        (status = 400, description = "No escrow deployed for this repository", body = ErrorResponse),
        (status = 401, description = "Missing or invalid bearer token", body = ErrorResponse),
        (status = 403, description = "Caller is not a maintainer", body = ErrorResponse),
        (status = 500, description = "Failed to create unsigned close transaction", body = ErrorResponse)
    )
)]
pub async fn close_unsigned(
    State(state): State<AppState>,
    user: AuthedUser,
    Json(body): Json<CloseEscrow>,
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
        TxBuilder::close_escrow(&state, &repo, &body.maintainer_wallet).await?;

    Ok(Json(UnsignedTransactionResponse {
        unsigned_transaction,
    }))
}

#[utoipa::path(
    post,
    path = "/api/escrow/submit-close",
    tag = "Escrow",
    security(("bearer_auth" = [])),
    request_body = SubmitClose,
    responses(
        (status = 200, description = "Escrow close transaction submitted", body = OkResponse),
        (status = 401, description = "Missing or invalid bearer token", body = ErrorResponse),
        (status = 403, description = "Caller is not a maintainer", body = ErrorResponse),
        (status = 500, description = "Failed to submit close transaction", body = ErrorResponse)
    )
)]
pub async fn submit_close(
    State(state): State<AppState>,
    user: AuthedUser,
    Json(body): Json<SubmitClose>,
) -> Result<Json<OkResponse>, AppError> {
    if !is_maintainer(&state, user.github_id, body.repo_id).await? {
        return Err(AppError::forbidden(
            "Forbidden: Only maintainers can perform this action",
        ));
    }

    TrustlessWorkAPI::new(state.clone())
        .close(body.repo_id, &body.signed_xdr)
        .await?;

    Ok(Json(OkResponse { ok: true }))
}
