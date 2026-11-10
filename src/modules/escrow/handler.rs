use axum::{extract::State, Json};
use rust_decimal::Decimal;
use tracing::error;

use crate::{
    error::{AppError, ErrorResponse},
    middleware::auth::AuthedUser,
    middleware::rate_limit::check_user_limit,
    modules::{
        escrow::{
            dto::{
                CloseEscrow, ContractIdResponse, CreateEscrow, FundEscrow, OkResponse,
                RefundEscrow, RefundResponse, SubmitClose, SubmitDeploy, SubmitFund,
                SubmitFundResponse, UnsignedTransactionResponse,
            },
            trustless_work::{escrow_service::TrustlessWorkAPI, tx_builder::TxBuilder},
        },
        github::{auth::post_comment, comments, labels::ensure_default_repository_labels},
        notification::{
            kinds::{Kind, Notify},
            service::Service,
        },
        repo::repository::{get_repo_by_id, is_maintainer},
    },
    state::AppState,
};

#[utoipa::path(
    post,
    path = "/api/v1/escrow/create-unsigned",
    tag = "Escrow",
    security(("bearer_auth" = [])),
    request_body = CreateEscrow,
    responses(
        (status = 200, description = "Unsigned escrow deploy transaction", body = UnsignedTransactionResponse),
        (status = 401, description = "Missing or invalid bearer token", body = ErrorResponse),
        (status = 429, description = "Per-user rate limit exceeded", body = ErrorResponse),
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
    check_user_limit(&state, "escrow", user.github_id, 5, 60, false).await?;

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
    path = "/api/v1/escrow/submit-deploy",
    tag = "Escrow",
    security(("bearer_auth" = [])),
    request_body = SubmitDeploy,
    responses(
        (status = 200, description = "Escrow contract deployed", body = ContractIdResponse),
        (status = 401, description = "Missing or invalid bearer token", body = ErrorResponse),
        (status = 429, description = "Per-user rate limit exceeded", body = ErrorResponse),
        (status = 403, description = "Caller is not a maintainer", body = ErrorResponse),
        (status = 500, description = "Failed to submit deploy transaction", body = ErrorResponse)
    )
)]
pub async fn submit_deploy(
    State(state): State<AppState>,
    user: AuthedUser,
    Json(body): Json<SubmitDeploy>,
) -> Result<Json<ContractIdResponse>, AppError> {
    check_user_limit(&state, "escrow", user.github_id, 5, 60, false).await?;

    if !is_maintainer(&state, user.github_id, body.repo_id).await? {
        return Err(AppError::forbidden(
            "Forbidden: Only maintainers can perform this action",
        ));
    }

    let contract_id = TrustlessWorkAPI::new(state.clone())
        .deploy(body.repo_id, &body.signed_xdr)
        .await?;

    if let Some(repo) = get_repo_by_id(&state, body.repo_id).await? {
        if let Err(error) =
            ensure_default_repository_labels(&state, repo.github_repo_id, &repo.full_name).await
        {
            error!(%error, repo = %repo.full_name, "failed to create default labels after escrow deploy");
        }
    }

    Ok(Json(ContractIdResponse { contract_id }))
}

#[utoipa::path(
    post,
    path = "/api/v1/escrow/fund-unsigned",
    tag = "Escrow",
    security(("bearer_auth" = [])),
    request_body = FundEscrow,
    responses(
        (status = 200, description = "Unsigned escrow fund transaction", body = UnsignedTransactionResponse),
        (status = 400, description = "Invalid amount, wallet, or escrow state", body = ErrorResponse),
        (status = 401, description = "Missing or invalid bearer token", body = ErrorResponse),
        (status = 429, description = "Per-user rate limit exceeded", body = ErrorResponse),
        (status = 403, description = "Caller is not a maintainer", body = ErrorResponse),
        (status = 500, description = "Failed to create unsigned fund transaction", body = ErrorResponse)
    )
)]
pub async fn fund_unsigned(
    State(state): State<AppState>,
    user: AuthedUser,
    Json(body): Json<FundEscrow>,
) -> Result<Json<UnsignedTransactionResponse>, AppError> {
    check_user_limit(&state, "escrow", user.github_id, 5, 60, false).await?;

    if body.amount <= Decimal::ZERO || body.funder_wallet.is_empty() {
        return Err(AppError::bad_request("Invalid amount or funder wallet"));
    }

    let repo = get_repo_by_id(&state, body.repo_id)
        .await?
        .ok_or_else(|| AppError::bad_request("Repo Not Found"))?;

    if !is_maintainer(&state, user.github_id, repo.id).await? {
        return Err(AppError::forbidden(
            "Forbidden: Only maintainers can fund the escrow",
        ));
    }

    if repo.escrow_contract_id.is_none() {
        return Err(AppError::bad_request(
            "No escrow deployed for this repository",
        ));
    }

    let unsigned_transaction =
        TxBuilder::fund_escrow(&state, &repo, body.amount, &body.funder_wallet).await?;

    Ok(Json(UnsignedTransactionResponse {
        unsigned_transaction,
    }))
}

#[utoipa::path(
    post,
    path = "/api/v1/escrow/submit-fund",
    tag = "Escrow",
    security(("bearer_auth" = [])),
    request_body = SubmitFund,
    responses(
        (status = 200, description = "Fund transaction submitted and local balance updated", body = SubmitFundResponse),
        (status = 400, description = "Invalid amount, wallet, or escrow state", body = ErrorResponse),
        (status = 401, description = "Missing or invalid bearer token", body = ErrorResponse),
        (status = 429, description = "Per-user rate limit exceeded", body = ErrorResponse),
        (status = 403, description = "Caller is not a maintainer", body = ErrorResponse),
        (status = 500, description = "Failed to submit fund transaction", body = ErrorResponse)
    )
)]
pub async fn submit_fund(
    State(state): State<AppState>,
    user: AuthedUser,
    Json(body): Json<SubmitFund>,
) -> Result<Json<SubmitFundResponse>, AppError> {
    check_user_limit(&state, "escrow", user.github_id, 5, 60, false).await?;

    if body.amount <= Decimal::ZERO || body.funder_wallet.is_empty() {
        return Err(AppError::bad_request("Invalid amount or funder wallet"));
    }

    if !is_maintainer(&state, user.github_id, body.repo_id).await? {
        return Err(AppError::forbidden(
            "Forbidden: Only maintainers can fund the escrow",
        ));
    }

    let new_balance = TrustlessWorkAPI::new(state.clone())
        .fund(
            body.repo_id,
            &body.signed_xdr,
            &body.funder_wallet,
            body.amount,
        )
        .await?;

    // Funding succeeded even if Redis is temporarily unavailable. The durable
    // scanner will retry a failed wake-up without asking the maintainer to fund twice.
    if let Err(error) = crate::infra::recovery::wake_repo(&state, body.repo_id).await {
        tracing::warn!(%error, "escrow funded; failed to wake pending bounty requests");
    }

    // Send EscrowFunded notification to maintainers
    let n = Notify {
        recipient: body.repo_id, // Placeholder; Service::notify_maintainers will set actual recipients
        kind: Kind::EscrowFunded,
        title: "Escrow Funded".to_string(),
        body: format!("Escrow has been funded with {} USDC", body.amount),
        ref_id: None,
        data: serde_json::json!({
            "repoId": body.repo_id,
            "amount": body.amount.to_string(),
        }),
        dedupe_key: format!("escrow-funded:{}:{}", body.repo_id, body.amount),
        actor: None,
    };
    Service::notify_maintainers_quiet(&state, body.repo_id, n).await;

    Ok(Json(SubmitFundResponse {
        ok: true,
        new_balance: Some(new_balance),
    }))
}

#[utoipa::path(
    post,
    path = "/api/v1/escrow/refund",
    tag = "Escrow",
    security(("bearer_auth" = [])),
    request_body = RefundEscrow,
    responses(
        (status = 200, description = "Escrow refunded and open bounties cancelled", body = RefundResponse),
        (status = 400, description = "Escrow has no recorded funding wallet", body = ErrorResponse),
        (status = 401, description = "Missing or invalid bearer token", body = ErrorResponse),
        (status = 429, description = "Per-user rate limit exceeded", body = ErrorResponse),
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
    check_user_limit(&state, "escrow", user.github_id, 5, 60, false).await?;

    let (maintainer, repo) = tokio::try_join!(
        is_maintainer(&state, user.github_id, body.repo_id),
        get_repo_by_id(&state, body.repo_id)
    )?;
    if !maintainer {
        return Err(AppError::forbidden(
            "Forbidden: Only maintainers can refund funds",
        ));
    }
    let repo = repo.ok_or_else(|| AppError::not_found("Repo or escrow not found"))?;

    if repo.escrow_contract_id.is_none() {
        return Err(AppError::not_found("Repo or escrow not found"));
    }

    let funder_wallet = validate_refund_wallet(&body.maintainer_wallet)?;
    crate::modules::github::authorization::require_current_repo_maintainer(
        &state,
        &repo,
        user.github_id,
    )
    .await?;
    let contract_id = repo.escrow_contract_id.clone().unwrap_or_default();

    let (refunded_amount, issues_to_cancel) = TrustlessWorkAPI::new(state.clone())
        .refund_with_issues(&repo, funder_wallet)
        .await?;
    let cancelled_issues = issues_to_cancel.len() as i64;

    // Send EscrowRefunded notification to maintainers
    let n = Notify {
        recipient: repo.id, // Placeholder; Service::notify_maintainers will set actual recipients
        kind: Kind::EscrowRefunded,
        title: "Escrow Refunded".to_string(),
        body: format!("Escrow has been refunded with {} USDC", refunded_amount),
        ref_id: None,
        data: serde_json::json!({
            "repoId": repo.id,
            "amount": refunded_amount.to_string(),
        }),
        dedupe_key: format!("escrow-refunded:{}:{}", repo.id, refunded_amount),
        actor: None,
    };
    Service::notify_maintainers_quiet(&state, repo.id, n).await;

    let mut deliveries = tokio::task::JoinSet::new();
    for issue in issues_to_cancel {
        let state = state.clone();
        let repo_name = repo.full_name.clone();
        let repo_id = repo.id;
        let comment = comments::bounty_cancelled_after_refund(&contract_id);
        deliveries.spawn(async move {
            // Send BountyCancelled notification to assignee
            if let Some(assignee_id) = issue.assignee_id {
                let n = Notify {
                    recipient: assignee_id,
                    kind: Kind::BountyCancelled,
                    title: format!("Bounty Cancelled: #{}", issue.github_issue_number),
                    body: "A repository maintainer withdrew the escrowed funds".to_string(),
                    ref_id: Some(issue.id),
                    data: serde_json::json!({
                        "repoId": repo_id,
                        "issueNumber": issue.github_issue_number,
                    }),
                    dedupe_key: format!("cancelled:{}:refund", issue.id),
                    actor: None,
                };
                Service::notify_contributor_quiet(&state, n).await;
            }
            if let Err(error) =
                post_comment(&state, &repo_name, issue.github_issue_number, &comment).await
            {
                error!(%error, issue = issue.github_issue_number, "failed to post refund cancellation comment");
            }
        });
        if deliveries.len() >= 4 {
            if let Some(Err(error)) = deliveries.join_next().await {
                error!(%error, "refund notification task failed");
            }
        }
    }
    while let Some(result) = deliveries.join_next().await {
        if let Err(error) = result {
            error!(%error, "refund notification task failed");
        }
    }

    Ok(Json(RefundResponse {
        refunded_amount,
        cancelled_issues,
    }))
}

fn validate_refund_wallet(value: &str) -> Result<&str, AppError> {
    let wallet = value.trim();
    stellar_base::crypto::PublicKey::from_account_id(wallet)
        .map_err(|_| AppError::bad_request("Invalid Stellar refund wallet"))?;
    Ok(wallet)
}

#[cfg(test)]
mod refund_tests {
    use super::validate_refund_wallet;

    #[test]
    fn refund_wallet_requires_a_valid_stellar_account() {
        assert!(
            validate_refund_wallet("GDTNILC7HOK4QGBBUPYLWFDOCW66H5BAIL374PJFIS7SZKHR4QJCH43D")
                .is_ok()
        );
        assert!(validate_refund_wallet("").is_err());
        assert!(
            validate_refund_wallet("GDTNILC7HOK4QGBBUPYLWFDOCW66H5BAIL374PJFIS7SZKHR4QJCH43E")
                .is_err()
        );
    }
}

#[utoipa::path(
    post,
    path = "/api/v1/escrow/close-unsigned",
    tag = "Escrow",
    security(("bearer_auth" = [])),
    request_body = CloseEscrow,
    responses(
        (status = 200, description = "Unsigned escrow close transaction", body = UnsignedTransactionResponse),
        (status = 400, description = "No escrow deployed for this repository", body = ErrorResponse),
        (status = 401, description = "Missing or invalid bearer token", body = ErrorResponse),
        (status = 429, description = "Per-user rate limit exceeded", body = ErrorResponse),
        (status = 403, description = "Caller is not a maintainer", body = ErrorResponse),
        (status = 500, description = "Failed to create unsigned close transaction", body = ErrorResponse)
    )
)]
pub async fn close_unsigned(
    State(state): State<AppState>,
    user: AuthedUser,
    Json(body): Json<CloseEscrow>,
) -> Result<Json<UnsignedTransactionResponse>, AppError> {
    check_user_limit(&state, "escrow", user.github_id, 5, 60, false).await?;

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
    path = "/api/v1/escrow/submit-close",
    tag = "Escrow",
    security(("bearer_auth" = [])),
    request_body = SubmitClose,
    responses(
        (status = 200, description = "Escrow close transaction submitted", body = OkResponse),
        (status = 401, description = "Missing or invalid bearer token", body = ErrorResponse),
        (status = 429, description = "Per-user rate limit exceeded", body = ErrorResponse),
        (status = 403, description = "Caller is not a maintainer", body = ErrorResponse),
        (status = 500, description = "Failed to submit close transaction", body = ErrorResponse)
    )
)]
pub async fn submit_close(
    State(state): State<AppState>,
    user: AuthedUser,
    Json(body): Json<SubmitClose>,
) -> Result<Json<OkResponse>, AppError> {
    check_user_limit(&state, "escrow", user.github_id, 5, 60, false).await?;

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
