use reqwest::Method;
use resend_rs::json;

use crate::shared::constants::PLATFORM_FEES;
use crate::{
    error::AppError, modules::escrow::trustless_work::client::tw_fetch, shared::models::Repo,
    state::AppState,
};

pub struct UnsignedTx;

impl UnsignedTx {
    pub async fn create_escrow(
        state: &AppState,
        repo: &Repo,
        maintainer_address: &str,
    ) -> Result<String, AppError> {
        let repo_name = &repo.full_name;
        let platform = &state.config.platform_stellar_public_key;
        let dipute_resolver = &state.config.dispute_resolver_stellar_public_key;
        let token_address = &state.config.token_address;
        let path = "/deployer/multi-release";

        let body = Some(json!({
          "signer": maintainer_address,
            "engagementId": format!("repo-{}", chrono::Utc::now().timestamp_millis()),
            "title": format!("Trustless-OSS Bounty for : {}", repo.full_name),
            "description": format!("Trustelss-OSS bounty rewards in {}", repo_name),
          "roles": {
            "approver": platform,
            "serviceProvider": platform,
            "platformAddress": platform,
            "releaseSigner": platform,
            "disputeResolver": dipute_resolver
          },
          "platformFee":PLATFORM_FEES,
          "milestones": [
            {
              "description": format!("Trustless_OSS platform fees milestone Created for {}" , repo_name),
              "amount": PLATFORM_FEES,
              "receiver": platform
            }
          ],
          "trustline": {
            "symbol": "USDC",
            "address": token_address
          }
        }

        ));

        let response = tw_fetch(state, path, Method::POST, body).await?;

        response
            .get("unsignedTransaction")
            .and_then(|value| value.as_str())
            .map(str::to_owned)
            .ok_or_else(|| AppError::internal("[TrustlessWork]:response missing unsignedTx"))
    }

    pub async fn update_escrow() {
        todo!()
    }

    pub async fn approve_milestone() {
        todo!()
    }

    pub async fn release_milestone() {
        todo!()
    }

    pub async fn refund() {
        todo!()
    }

    pub async fn close_escrow() {
        todo!()
    }
}
