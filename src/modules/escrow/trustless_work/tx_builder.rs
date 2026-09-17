use reqwest::Method;
use rust_decimal::Decimal;
use serde_json::{json, Value};

use crate::{
    error::AppError,
    modules::escrow::trustless_work::api_client::{decimal_json_number, tw_fetch},
    shared::{constants::ESCROW_INIT_MILESTONE_USDC, models::Repo},
    state::AppState,
};

pub struct TxBuilder;

impl TxBuilder {
    // [ryzen-xp] : Deploy with platformFee 0 and 0.01 init milestone (not a 5 USDC fee row)
    pub async fn create_escrow(
        state: &AppState,
        repo: &Repo,
        maintainer_address: &str,
    ) -> Result<String, AppError> {
        let repo_name = &repo.full_name;
        let platform = &state.config.platform_stellar_public_key;
        let dispute_resolver = &state.config.dispute_resolver_stellar_public_key;
        let token_address = &state.config.token_address;
        let path = "/deployer/multi-release";

        let body = Some(json!({
            "signer": maintainer_address,
            "engagementId": format!("repo-{}", chrono::Utc::now().timestamp_millis()),
            "title": format!("Trustless-OSS Bounty for: {}", repo.full_name),
            "description": format!("Trustless-OSS bounty rewards in {repo_name}"),
            "roles": {
                "approver": platform,
                "serviceProvider": platform,
                "platformAddress": platform,
                "releaseSigner": platform,
                "disputeResolver": dispute_resolver,
            },
            "platformFee": 0,
            "milestones": [{
                "description": "Escrow Initialized",
                "amount": ESCROW_INIT_MILESTONE_USDC,
                "receiver": platform,
            }],
            "trustline": {
                "symbol": "USDC",
                "address": token_address,
            },
        }));

        let response = tw_fetch(state, path, Method::POST, body).await?;

        Self::unsigned_transaction(response)
    }

    pub async fn fund_escrow(
        state: &AppState,
        repo: &Repo,
        amount: Decimal,
        funder_address: &str,
    ) -> Result<String, AppError> {
        let contract_id = repo
            .escrow_contract_id
            .as_deref()
            .ok_or_else(|| AppError::bad_request("No escrow deployed for this repository"))?;

        // [ryzen-xp] : fund-escrow amount is a JSON number (TW class-validator)
        let response = tw_fetch(
            state,
            "/escrow/multi-release/fund-escrow",
            Method::POST,
            Some(json!({
                "contractId": contract_id,
                "signer": funder_address,
                "amount": decimal_json_number(amount, "amount")?,
            })),
        )
        .await?;

        Self::unsigned_transaction(response)
    }

    pub async fn close_escrow(
        state: &AppState,
        repo: &Repo,
        maintainer_address: &str,
    ) -> Result<String, AppError> {
        let contract_id = repo
            .escrow_contract_id
            .as_deref()
            .ok_or_else(|| AppError::bad_request("No escrow deployed"))?;

        let response = tw_fetch(
            state,
            "/escrow/multi-release/close-escrow",
            Method::POST,
            Some(json!({
                "contractId": contract_id,
                "signer": maintainer_address,
            })),
        )
        .await?;

        Self::unsigned_transaction(response)
    }

    fn unsigned_transaction(response: Value) -> Result<String, AppError> {
        response
            .get("unsignedTransaction")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .ok_or_else(|| AppError::internal("TrustlessWork response missing unsignedTransaction"))
    }
}
