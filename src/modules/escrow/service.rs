/// All escrow business logic — deploy, fund, close, refund, milestone operations.
///
/// This module talks directly to the TrustlessWork API and the database.
/// Handlers in `handler.rs` call into these functions; they contain no HTTP
/// concerns (no Axum types, no request parsing).
use rust_decimal::{prelude::ToPrimitive, Decimal};
use serde_json::{json, Value};
use tracing::info;

use crate::{
    error::AppError,
    infra::stellar::signer::sign_and_send_transaction,
    modules::{
        escrow::trustless_work::client::tw_fetch,
        repo::repository::{get_repo_by_id, update_issue_status, update_repo_escrow_balance},
    },
    shared::models::{Issue, Repo},
    state::AppState,
};

// USDC contract address on Stellar testnet.
const TESTNET_USDC: &str = "GBBD47IF6LWK7P7MDEVSCWR7DPUWV3NY3DTQEVFL4NAT4AQH3ZLLFLA5";
const TRUSTLESS_WORK_FEE_BPS: i64 = 30;
const BASIS_POINTS: i64 = 10_000;

// ── Deploy ────────────────────────────────────────────────────────────────────

/// Build the unsigned deploy transaction for a new multi-release escrow.
pub async fn create_unsigned_escrow(
    state: &AppState,
    repo: &Repo,
    maintainer_wallet: &str,
) -> Result<String, AppError> {
    let platform_key = state.config.platform_stellar_public_key.as_str();

    let response = tw_fetch(
        state,
        "/deployer/multi-release",
        reqwest::Method::POST,
        Some(json!({
            "signer": maintainer_wallet,
            "engagementId": format!("repo-{}", chrono::Utc::now().timestamp_millis()),
            "title": format!("OSS Bounty: {}", repo.full_name),
            "description": format!("Escrow for OSS bounty rewards in {}", repo.full_name),
            "roles": {
                "approver": platform_key,
                "serviceProvider": platform_key,
                "platformAddress": platform_key,
                "releaseSigner": platform_key,
                "disputeResolver": state.config.dispute_resolver_stellar_public_key,
            },
            "platformFee": 0,
            "milestones": [{ "description": "Escrow Initialized", "amount": 0.01, "receiver": platform_key }],
            "trustline": { "address": TESTNET_USDC, "symbol": "USDC" },
        })),
    )
    .await?;

    response
        .get("unsignedTransaction")
        .and_then(|value| value.as_str())
        .map(str::to_owned)
        .ok_or_else(|| AppError::internal("TrustlessWork response missing unsignedTransaction"))
}

/// Submit the signed deploy XDR, persist the resulting contract ID.
pub async fn submit_deploy_escrow(
    state: &AppState,
    repo_id: uuid::Uuid,
    signed_xdr: &str,
) -> Result<String, AppError> {
    let result = tw_fetch(
        state,
        "/helper/send-transaction",
        reqwest::Method::POST,
        Some(json!({ "signedXdr": signed_xdr })),
    )
    .await?;

    let contract_id = result
        .get("contractId")
        .and_then(|value| value.as_str())
        .ok_or_else(|| AppError::internal("TrustlessWork response missing contractId"))?;

    crate::modules::repo::repository::update_repo_escrow_contract(state, repo_id, contract_id)
        .await?;

    Ok(contract_id.to_string())
}

// ── Fund ──────────────────────────────────────────────────────────────────────

/// Build the unsigned fund transaction.
pub async fn create_fund_unsigned(
    state: &AppState,
    repo: &Repo,
    amount: Decimal,
    funder_wallet: &str,
) -> Result<String, AppError> {
    let contract_id = repo
        .escrow_contract_id
        .as_deref()
        .ok_or_else(|| AppError::bad_request("No escrow deployed for this repository"))?;

    let response = tw_fetch(
        state,
        "/escrow/multi-release/fund-escrow",
        reqwest::Method::POST,
        Some(json!({
            "contractId": contract_id,
            "signer": funder_wallet,
            "amount": decimal_json_number(amount, "amount")?,
        })),
    )
    .await?;

    response
        .get("unsignedTransaction")
        .and_then(|value| value.as_str())
        .map(str::to_owned)
        .ok_or_else(|| AppError::internal("TrustlessWork response missing unsignedTransaction"))
}

/// Submit the signed fund XDR and update the stored balance.
pub async fn submit_fund_escrow(
    state: &AppState,
    repo_id: uuid::Uuid,
    amount: Decimal,
    signed_xdr: &str,
) -> Result<Decimal, AppError> {
    tw_fetch(
        state,
        "/helper/send-transaction",
        reqwest::Method::POST,
        Some(json!({ "signedXdr": signed_xdr })),
    )
    .await?;

    let repo = get_repo_by_id(state, repo_id)
        .await?
        .ok_or_else(|| AppError::not_found("Repo not found"))?;
    let new_balance = repo.escrow_balance + amount;
    update_repo_escrow_balance(state, repo_id, new_balance, Some(repo.github_repo_id)).await?;
    Ok(new_balance)
}

/// Re-fetch the on-chain balance and sync it to the database if it drifted.
pub async fn sync_repo_escrow_balance(state: &AppState, repo: &Repo) -> Result<Decimal, AppError> {
    let contract_id = repo
        .escrow_contract_id
        .as_deref()
        .ok_or_else(|| AppError::bad_request("No escrow deployed"))?;

    let escrow_array = tw_fetch(
        state,
        &format!("/helper/get-escrow-by-contract-ids?contractIds[]={contract_id}"),
        reqwest::Method::GET,
        None,
    )
    .await?;

    let on_chain_balance = escrow_array
        .as_array()
        .and_then(|items| items.first())
        .and_then(|item| item.get("balance"))
        .and_then(|value| value.as_f64())
        .and_then(Decimal::from_f64_retain)
        .unwrap_or(repo.escrow_balance);

    if on_chain_balance != repo.escrow_balance {
        update_repo_escrow_balance(state, repo.id, on_chain_balance, Some(repo.github_repo_id))
            .await?;
    }

    Ok(on_chain_balance)
}

// ── Close ─────────────────────────────────────────────────────────────────────

/// Build the unsigned close-escrow transaction.
pub async fn create_close_unsigned(
    state: &AppState,
    repo: &Repo,
    maintainer_wallet: &str,
) -> Result<String, AppError> {
    let contract_id = repo
        .escrow_contract_id
        .as_deref()
        .ok_or_else(|| AppError::bad_request("No escrow deployed"))?;

    let response = tw_fetch(
        state,
        "/escrow/multi-release/close-escrow",
        reqwest::Method::POST,
        Some(json!({
            "contractId": contract_id,
            "signer": maintainer_wallet,
        })),
    )
    .await?;

    response
        .get("unsignedTransaction")
        .and_then(|value| value.as_str())
        .map(str::to_owned)
        .ok_or_else(|| AppError::internal("TrustlessWork response missing unsignedTransaction"))
}

/// Submit the signed close XDR and clear the escrow record in the database.
pub async fn submit_close_escrow(
    state: &AppState,
    repo_id: uuid::Uuid,
    signed_xdr: &str,
) -> Result<(), AppError> {
    tw_fetch(
        state,
        "/helper/send-transaction",
        reqwest::Method::POST,
        Some(json!({ "signedXdr": signed_xdr })),
    )
    .await?;

    crate::modules::repo::repository::clear_repo_escrow(state, repo_id).await
}

// ── Refund ────────────────────────────────────────────────────────────────────

/// Full refund flow: dispute/resolve every unreleased milestone, withdraw
/// remaining balance, cancel all open issues.
///
/// Returns `(total_refunded, cancelled_issue_count)`.
pub async fn refund_escrow(
    state: &AppState,
    repo: &Repo,
    maintainer_wallet: &str,
) -> Result<(Decimal, i64), AppError> {
    let platform_key = state.config.platform_stellar_public_key.as_str();
    let contract_id = repo
        .escrow_contract_id
        .as_deref()
        .ok_or_else(|| AppError::not_found("Repo or escrow not found"))?;
    let resolver_pub = state.config.dispute_resolver_stellar_public_key.clone();
    let resolver_secret = state.config.dispute_resolver_stellar_secret_key.clone();

    let escrow_array = tw_fetch(
        state,
        &format!("/helper/get-escrow-by-contract-ids?contractIds[]={contract_id}"),
        reqwest::Method::GET,
        None,
    )
    .await?;
    let escrow_data = escrow_array
        .as_array()
        .and_then(|items| items.first())
        .cloned()
        .ok_or_else(|| AppError::internal("Escrow not found on Trustless Work"))?;

    let milestones = escrow_data
        .get("milestones")
        .and_then(|value| value.as_array())
        .cloned()
        .unwrap_or_default();

    let is_dual_wallet = escrow_data
        .get("roles")
        .and_then(|roles| roles.get("disputeResolver"))
        .and_then(|value| value.as_str())
        == Some(resolver_pub.as_str());

    let mut total_refunded = Decimal::ZERO;
    let mut remaining_balance = current_escrow_balance(state, contract_id).await?;

    if is_dual_wallet {
        for (index, milestone) in milestones.iter().enumerate() {
            if milestone
                .get("flags")
                .and_then(|flags| flags.get("released"))
                .and_then(|value| value.as_bool())
                .unwrap_or(false)
                || milestone
                    .get("flags")
                    .and_then(|flags| flags.get("resolved"))
                    .and_then(|value| value.as_bool())
                    .unwrap_or(false)
            {
                continue;
            }

            let milestone_amount = milestone
                .get("amount")
                .and_then(decimal_from_value)
                .unwrap_or(Decimal::ZERO);
            if milestone_amount <= Decimal::ZERO {
                return Err(AppError::bad_request(format!(
                    "Cannot resolve milestone {index}: its amount is not positive"
                )));
            }

            let observed_balance = current_escrow_balance(state, contract_id).await?;
            remaining_balance = remaining_balance.min(observed_balance);
            let distribution_amount = resolution_distribution_amount(
                milestone_amount,
                remaining_balance,
                state.config.is_mainnet(),
            );
            if distribution_amount <= Decimal::ZERO {
                return Err(AppError::bad_request(format!(
                    "Cannot resolve milestone {index}: escrow has no available balance"
                )));
            }

            let dispute_res = tw_fetch(
                state,
                "/escrow/multi-release/dispute-milestone",
                reqwest::Method::POST,
                Some(json!({
                    "signer": platform_key,
                    "contractId": contract_id,
                    "milestoneIndex": index.to_string(),
                })),
            )
            .await;

            if let Ok(response) = dispute_res {
                if let Some(unsigned) = response.get("unsignedTransaction").and_then(|v| v.as_str())
                {
                    sign_and_send_transaction(state, unsigned, None).await?;
                }
            }

            let resolve_res = tw_fetch(
                state,
                "/escrow/multi-release/resolve-milestone-dispute",
                reqwest::Method::POST,
                Some(json!({
                    "disputeResolver": resolver_pub,
                    "contractId": contract_id,
                    "milestoneIndex": index.to_string(),
                    "distributions": [{
                        "address": maintainer_wallet,
                        "amount": decimal_json_number(
                            distribution_amount,
                            "distribution amount",
                        )?,
                    }],
                })),
            )
            .await?;
            if let Some(unsigned) = resolve_res
                .get("unsignedTransaction")
                .and_then(|value| value.as_str())
            {
                sign_and_send_transaction(state, unsigned, Some(&resolver_secret)).await?;
                remaining_balance = (remaining_balance
                    - consumed_balance_amount(distribution_amount, state.config.is_mainnet()))
                .max(Decimal::ZERO);
                total_refunded += distribution_amount;
            }
        }
    } else {
        let new_milestones: Vec<Value> = milestones
            .iter()
            .map(|milestone| {
                let released = milestone
                    .get("flags")
                    .and_then(|flags| flags.get("released"))
                    .and_then(|value| value.as_bool())
                    .unwrap_or(false);
                let resolved = milestone
                    .get("flags")
                    .and_then(|flags| flags.get("resolved"))
                    .and_then(|value| value.as_bool())
                    .unwrap_or(false);
                if released || resolved {
                    return milestone.clone();
                }
                json!({
                    "description": format!("Refund: {}", milestone.get("description").and_then(|v| v.as_str()).unwrap_or("")),
                    "amount": milestone.get("amount").cloned().unwrap_or(json!(0)),
                    "receiver": maintainer_wallet,
                    "status": "pending",
                    "evidence": milestone.get("evidence").cloned().unwrap_or(json!("")),
                    "flags": { "approved": false, "released": false, "disputed": false, "resolved": false },
                })
            })
            .collect();

        let mut payload = strip_escrow_metadata(&escrow_data);
        if let Some(obj) = payload.as_object_mut() {
            obj.insert("milestones".to_string(), json!(new_milestones));
            obj.insert("isActive".to_string(), json!(true));
        }

        let update_res = tw_fetch(
            state,
            "/escrow/multi-release/update-escrow",
            reqwest::Method::PUT,
            Some(json!({
                "signer": platform_key,
                "contractId": contract_id,
                "escrow": payload,
            })),
        )
        .await?;
        if let Some(unsigned) = update_res
            .get("unsignedTransaction")
            .and_then(|value| value.as_str())
        {
            sign_and_send_transaction(state, unsigned, None).await?;
        }

        for (index, milestone) in new_milestones.iter().enumerate() {
            let description = milestone
                .get("description")
                .and_then(|value| value.as_str())
                .unwrap_or("");
            if !description.starts_with("Refund:") {
                continue;
            }
            let amount = milestone
                .get("amount")
                .and_then(|value| value.as_f64())
                .and_then(Decimal::from_f64_retain)
                .unwrap_or(Decimal::ZERO);

            for (path, body) in [
                (
                    "/escrow/multi-release/approve-milestone",
                    json!({
                        "approver": platform_key,
                        "contractId": contract_id,
                        "milestoneIndex": index.to_string(),
                    }),
                ),
                (
                    "/escrow/multi-release/release-milestone-funds",
                    json!({
                        "releaseSigner": platform_key,
                        "contractId": contract_id,
                        "milestoneIndex": index.to_string(),
                    }),
                ),
            ] {
                let response = tw_fetch(state, path, reqwest::Method::POST, Some(body)).await?;
                if let Some(unsigned) = response.get("unsignedTransaction").and_then(|v| v.as_str())
                {
                    sign_and_send_transaction(state, unsigned, None).await?;
                }
            }
            total_refunded += amount;
        }
    }

    let observed_balance = current_escrow_balance(state, contract_id).await?;
    let current_balance = if is_dual_wallet {
        remaining_balance.min(observed_balance)
    } else {
        observed_balance
    };

    if current_balance > Decimal::ZERO {
        let distribution_amount = resolution_distribution_amount(
            current_balance,
            current_balance,
            state.config.is_mainnet(),
        );
        let withdraw_res = tw_fetch(
            state,
            "/escrow/multi-release/withdraw-remaining-funds",
            reqwest::Method::POST,
            Some(json!({
                "contractId": contract_id,
                "disputeResolver": if is_dual_wallet { resolver_pub.clone() } else { platform_key.to_string() },
                "distributions": [{
                    "address": maintainer_wallet,
                    "amount": decimal_json_number(distribution_amount, "distribution amount")?,
                }],
            })),
        )
        .await?;
        if let Some(unsigned) = withdraw_res
            .get("unsignedTransaction")
            .and_then(|value| value.as_str())
        {
            sign_and_send_transaction(
                state,
                unsigned,
                if is_dual_wallet {
                    Some(&resolver_secret)
                } else {
                    None
                },
            )
            .await?;
            total_refunded += distribution_amount;
        }
    }

    let issues = crate::modules::repo::repository::list_issues_to_cancel(state, repo.id).await?;
    let issue_ids: Vec<_> = issues.iter().map(|issue| issue.id).collect();
    crate::modules::repo::repository::fail_assignments_for_issues(state, &issue_ids).await?;

    let mut cancelled_count = 0_i64;
    for issue in &issues {
        update_issue_status(state, issue.id, "cancelled", None).await?;
        cancelled_count += 1;
    }

    update_repo_escrow_balance(state, repo.id, Decimal::ZERO, Some(repo.github_repo_id)).await?;
    Ok((total_refunded, cancelled_count))
}

// ── Milestone operations ──────────────────────────────────────────────────────

/// Add or update a milestone on-chain for the given issue, then mark it active.
pub async fn push_milestone_on_chain(
    state: &AppState,
    repo: &Repo,
    issue: &Issue,
    payout_address: &str,
    payout_chain: &str,
) -> Result<i32, AppError> {
    let platform_key = state.config.platform_stellar_public_key.as_str();
    let contract_id = repo
        .escrow_contract_id
        .as_deref()
        .ok_or_else(|| AppError::bad_request("No escrow deployed"))?;

    let escrow_array = tw_fetch(
        state,
        &format!("/helper/get-escrow-by-contract-ids?contractIds[]={contract_id}"),
        reqwest::Method::GET,
        None,
    )
    .await?;

    let escrow_data = escrow_array
        .as_array()
        .and_then(|items| items.first())
        .cloned()
        .ok_or_else(|| AppError::internal(format!("Escrow not found: {contract_id}")))?;

    let current_milestones = escrow_data
        .get("milestones")
        .and_then(|value| value.as_array())
        .cloned()
        .unwrap_or_default();

    let receiver = build_receiver(payout_chain, payout_address)?;
    let milestone_data = json!({
        "description": format!("Issue #{}: {}", issue.github_issue_number, issue.title),
        "amount": decimal_json_number(issue.reward_amount, "milestone amount")?,
        "status": "pending",
        "evidence": "",
        "flags": { "approved": false, "released": false, "disputed": false, "resolved": false },
        "receiver": receiver,
    });

    let mut new_milestones = current_milestones.clone();
    let milestone_index = if let Some(index) = issue.milestone_index {
        if (index as usize) < new_milestones.len() {
            new_milestones[index as usize] = milestone_data.clone();
            index
        } else {
            let index = current_milestones.len() as i32;
            new_milestones.push(milestone_data);
            index
        }
    } else {
        let index = current_milestones.len() as i32;
        new_milestones.push(milestone_data);
        index
    };

    let escrow_payload = strip_escrow_metadata(&escrow_data);
    let mut payload = escrow_payload;
    if let Some(obj) = payload.as_object_mut() {
        obj.insert("milestones".to_string(), json!(new_milestones));
    }

    let update_res = tw_fetch(
        state,
        "/escrow/multi-release/update-escrow",
        reqwest::Method::PUT,
        Some(json!({
            "signer": platform_key,
            "contractId": contract_id,
            "escrow": payload,
        })),
    )
    .await?;

    let unsigned = update_res
        .get("unsignedTransaction")
        .and_then(|value| value.as_str())
        .ok_or_else(|| AppError::internal("missing unsignedTransaction"))?;

    sign_and_send_transaction(state, unsigned, None).await?;
    update_issue_status(state, issue.id, "active", Some(milestone_index)).await?;
    info!(
        issue = issue.github_issue_number,
        milestone_index, "issue pushed on-chain"
    );
    Ok(milestone_index)
}

/// Approve and release funds for a completed milestone.
pub async fn release_escrow_milestone(
    state: &AppState,
    repo: &Repo,
    issue: &Issue,
) -> Result<String, AppError> {
    let platform_key = state.config.platform_stellar_public_key.as_str();
    let contract_id = repo
        .escrow_contract_id
        .as_deref()
        .ok_or_else(|| AppError::bad_request("No escrow deployed"))?;
    let milestone_index = issue.milestone_index.ok_or_else(|| {
        AppError::bad_request(format!(
            "milestone_index is null for issue #{}",
            issue.github_issue_number
        ))
    })?;

    let approve_res = tw_fetch(
        state,
        "/escrow/multi-release/approve-milestone",
        reqwest::Method::POST,
        Some(json!({
            "approver": platform_key,
            "contractId": contract_id,
            "milestoneIndex": milestone_index.to_string(),
        })),
    )
    .await;

    match approve_res {
        Ok(response) => {
            let unsigned = response
                .get("unsignedTransaction")
                .and_then(|value| value.as_str())
                .ok_or_else(|| AppError::internal("missing unsignedTransaction"))?;
            sign_and_send_transaction(state, unsigned, None).await?;
        }
        Err(error)
            if error
                .to_string()
                .contains("already been approved previously") => {}
        Err(error) => return Err(error),
    }

    let release_res = tw_fetch(
        state,
        "/escrow/multi-release/release-milestone-funds",
        reqwest::Method::POST,
        Some(json!({
            "releaseSigner": platform_key,
            "contractId": contract_id,
            "milestoneIndex": milestone_index.to_string(),
        })),
    )
    .await;

    match release_res {
        Ok(response) => {
            let unsigned = response
                .get("unsignedTransaction")
                .and_then(|value| value.as_str())
                .ok_or_else(|| AppError::internal("missing unsignedTransaction"))?;
            let result = sign_and_send_transaction(state, unsigned, None).await?;
            Ok(result
                .get("hash")
                .or_else(|| result.get("transactionHash"))
                .and_then(|value| value.as_str())
                .unwrap_or("success")
                .to_string())
        }
        Err(error) => {
            let message = error.to_string();
            if message.contains("already been released previously")
                || message.contains("already been paid")
                || (message.contains("Only the dispute resolver can execute this function")
                    && issue.reward_amount == Decimal::ZERO)
            {
                Ok("success".to_string())
            } else {
                Err(error)
            }
        }
    }
}

// ── Private helpers ───────────────────────────────────────────────────────────

/// Build the receiver accepted by Trustless Work multi-release escrows.
fn build_receiver(payout_chain: &str, payout_address: &str) -> Result<Value, AppError> {
    if payout_address.trim().is_empty() {
        return Err(AppError::bad_request("Payout address cannot be empty"));
    }

    if payout_chain.eq_ignore_ascii_case("stellar") {
        return Ok(json!(payout_address));
    }

    Err(AppError::bad_request(format!(
        "Trustless Work multi-release escrows currently support Stellar payout addresses only; unsupported payout chain: {payout_chain}"
    )))
}

/// Remove read-only metadata fields before sending an escrow payload to the
/// update endpoint.
fn strip_escrow_metadata(escrow_data: &Value) -> Value {
    let mut payload = escrow_data.clone();
    if let Some(obj) = payload.as_object_mut() {
        for key in [
            "type",
            "createdAt",
            "updatedAt",
            "balance",
            "inconsistencies",
            "contractBaseId",
            "receiverMemo",
            "contractId",
            "signer",
        ] {
            obj.remove(key);
        }
    }
    payload
}

fn decimal_json_number(value: Decimal, field_name: &str) -> Result<Value, AppError> {
    let number = value
        .to_f64()
        .and_then(serde_json::Number::from_f64)
        .ok_or_else(|| AppError::internal(format!("Invalid {field_name}")))?;

    Ok(Value::Number(number))
}

async fn current_escrow_balance(state: &AppState, contract_id: &str) -> Result<Decimal, AppError> {
    let response = tw_fetch(
        state,
        &format!("/helper/get-multiple-escrow-balance?addresses[]={contract_id}"),
        reqwest::Method::GET,
        None,
    )
    .await?;

    response
        .as_array()
        .and_then(|items| items.first())
        .and_then(|item| item.get("balance"))
        .and_then(decimal_from_value)
        .ok_or_else(|| AppError::internal("TrustlessWork response missing escrow balance"))
}

fn decimal_from_value(value: &Value) -> Option<Decimal> {
    match value {
        Value::Number(number) => number.to_string().parse().ok(),
        Value::String(string) => string.parse().ok(),
        _ => None,
    }
}

fn resolution_distribution_amount(
    milestone_amount: Decimal,
    current_balance: Decimal,
    is_mainnet: bool,
) -> Decimal {
    let available = current_balance.max(Decimal::ZERO);
    let target = milestone_amount.min(available).max(Decimal::ZERO);

    if !is_mainnet {
        return target.round_dp(7);
    }

    let fee_multiplier =
        Decimal::from(BASIS_POINTS - TRUSTLESS_WORK_FEE_BPS) / Decimal::from(BASIS_POINTS);
    (target * fee_multiplier).round_dp(7)
}

fn consumed_balance_amount(distribution_amount: Decimal, is_mainnet: bool) -> Decimal {
    if !is_mainnet {
        return distribution_amount;
    }

    let fee_multiplier =
        Decimal::from(BASIS_POINTS - TRUSTLESS_WORK_FEE_BPS) / Decimal::from(BASIS_POINTS);
    (distribution_amount / fee_multiplier).round_dp(7)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decimal_json_number_serializes_as_number() {
        let value = decimal_json_number(Decimal::new(125, 1), "amount").unwrap();

        assert!(value.is_number());
        assert_eq!(value, json!(12.5));
    }

    #[test]
    fn stellar_milestone_receiver_is_a_plain_address() {
        let address = "GCPZCXSEWARYFZAJQEAJORUHZNGJNMQCDYYCDTYTUDD65H4TKKDF65HS";

        assert_eq!(build_receiver("stellar", address).unwrap(), json!(address));
    }

    #[test]
    fn non_stellar_milestone_receiver_is_rejected_before_api_call() {
        let error = build_receiver("base", "0x1234").unwrap_err();

        assert!(matches!(error, AppError::BadRequest { .. }));
        assert!(error.to_string().contains("Stellar payout addresses only"));
    }

    #[test]
    fn update_payload_removes_indexer_metadata_but_keeps_active_state() {
        let payload = strip_escrow_metadata(&json!({
            "contractId": "C123",
            "signer": "G123",
            "balance": 10,
            "type": "multi-release",
            "createdAt": "now",
            "updatedAt": "now",
            "inconsistencies": [],
            "contractBaseId": "base",
            "receiverMemo": 12,
            "isActive": true,
            "title": "Escrow",
        }));

        assert_eq!(payload, json!({ "isActive": true, "title": "Escrow" }));
    }

    #[test]
    fn caps_resolution_at_live_balance() {
        assert_eq!(
            resolution_distribution_amount(Decimal::new(10001, 2), Decimal::from(100), false),
            Decimal::from(100)
        );
    }

    #[test]
    fn subtracts_mainnet_protocol_fee_from_resolution() {
        assert_eq!(
            resolution_distribution_amount(Decimal::from(100), Decimal::from(100), true),
            Decimal::new(997, 1)
        );
    }
}
