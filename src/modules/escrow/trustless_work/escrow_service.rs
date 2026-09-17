use reqwest::Method;
use rust_decimal::Decimal;
use serde_json::{json, Value};
use tracing::info;
use uuid::Uuid;

use crate::{
    error::AppError,
    infra::stellar::signer::sign_and_send_transaction,
    modules::{
        bounty::repository::update_issue_status,
        escrow::{
            repository::{
                clear_repo_escrow, update_repo_escrow_balance, update_repo_escrow_contract,
                update_repo_escrow_funder_wallet,
            },
            trustless_work::api_client::{decimal_json_number, tw_fetch},
        },
        repo::repository::get_repo_by_id,
    },
    shared::{
        constants::{BASIS_POINTS, TRUSTLESS_WORK_FEE_BPS},
        models::{Issue, Repo},
    },
    state::AppState,
};

pub struct TrustlessWorkAPI {
    state: AppState,
}

impl TrustlessWorkAPI {
    pub fn new(state: AppState) -> Self {
        Self { state }
    }

    pub async fn deploy(&self, repo_id: Uuid, signed_xdr: &str) -> Result<String, AppError> {
        let result = self.submit_signed_transaction(signed_xdr).await?;
        let contract_id = result
            .get("contractId")
            .and_then(Value::as_str)
            .ok_or_else(|| AppError::internal("TrustlessWork response missing contractId"))?;

        update_repo_escrow_contract(&self.state, repo_id, contract_id).await?;
        Ok(contract_id.to_string())
    }

    pub async fn fund(
        &self,
        repo_id: Uuid,
        signed_xdr: &str,
        funder_wallet: &str,
        funded_amount: Decimal,
    ) -> Result<Decimal, AppError> {
        self.submit_signed_transaction(signed_xdr).await?;

        let repo = get_repo_by_id(&self.state, repo_id)
            .await?
            .ok_or_else(|| AppError::not_found("Repo not found"))?;

        if let Some(existing_funder_wallet) = repo.escrow_funder_wallet.as_deref() {
            if existing_funder_wallet != funder_wallet {
                return Err(AppError::bad_request(
                    "This escrow must be funded from the original wallet",
                ));
            }
        } else {
            update_repo_escrow_funder_wallet(&self.state, repo_id, funder_wallet).await?;
        }

        let contract_id = repo
            .escrow_contract_id
            .as_deref()
            .ok_or_else(|| AppError::bad_request("No escrow deployed"))?;
        let new_balance = self
            .wait_for_balance_after_fund(contract_id, repo.escrow_balance, funded_amount)
            .await?;
        update_repo_escrow_balance(&self.state, repo_id, new_balance, Some(repo.github_repo_id))
            .await?;

        Ok(new_balance)
    }

    pub async fn close(&self, repo_id: Uuid, signed_xdr: &str) -> Result<(), AppError> {
        self.submit_signed_transaction(signed_xdr).await?;
        clear_repo_escrow(&self.state, repo_id).await
    }

    pub async fn sync_balance(&self, repo: &Repo) -> Result<Decimal, AppError> {
        let contract_id = repo
            .escrow_contract_id
            .as_deref()
            .filter(|id| !id.trim().is_empty())
            .ok_or_else(|| AppError::bad_request("No escrow deployed"))?;

        let on_chain_balance = self.current_balance(contract_id).await?;
        update_repo_escrow_balance(
            &self.state,
            repo.id,
            on_chain_balance,
            Some(repo.github_repo_id),
        )
        .await?;

        Ok(on_chain_balance)
    }

    pub async fn push_milestone(
        &self,
        repo: &Repo,
        issue: &Issue,
        payout_address: &str,
        payout_chain: &str,
    ) -> Result<i32, AppError> {
        let platform_key = self.state.config.platform_stellar_public_key.as_str();
        let contract_id = repo
            .escrow_contract_id
            .as_deref()
            .ok_or_else(|| AppError::bad_request("No escrow deployed"))?;
        let escrow_data = self.fetch_escrow(contract_id).await?;
        let current_milestones = escrow_data
            .get("milestones")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let receiver = build_receiver(payout_chain, payout_address)?;
        if payout_chain.eq_ignore_ascii_case("stellar") {
            crate::infra::stellar::accounts::require_usdc_payout_account(
                &self.state,
                payout_address,
            )
            .await?;
        }
        let milestone_data = json!({
            "description": format!("Issue #{}: {}", issue.github_issue_number, issue.title),
            "amount": decimal_json_number(issue.reward_amount, "milestone amount")?,
            "status": "pending",
            "evidence": "",
            "flags": { "approved": false, "released": false, "disputed": false, "resolved": false },
            "receiver": receiver,
        });

        let mut milestones = current_milestones.clone();
        let milestone_index = match issue.milestone_index {
            Some(index) if (index as usize) < milestones.len() => {
                milestones[index as usize] = milestone_data;
                index
            }
            _ => {
                let index = current_milestones.len() as i32;
                milestones.push(milestone_data);
                index
            }
        };

        let mut payload = strip_escrow_metadata(&escrow_data);
        if let Some(object) = payload.as_object_mut() {
            object.insert("milestones".to_string(), json!(milestones));
        }

        let response = tw_fetch(
            &self.state,
            "/escrow/multi-release/update-escrow",
            Method::PUT,
            Some(json!({
                "signer": platform_key,
                "contractId": contract_id,
                "escrow": payload,
            })),
        )
        .await?;
        self.sign_platform_transaction(&response).await?;

        update_issue_status(&self.state, issue.id, "active", Some(milestone_index)).await?;
        info!(
            issue = issue.github_issue_number,
            milestone_index, "issue pushed on-chain"
        );
        Ok(milestone_index)
    }

    pub async fn release_milestone(&self, repo: &Repo, issue: &Issue) -> Result<String, AppError> {
        let platform_key = self.state.config.platform_stellar_public_key.as_str();
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

        if issue.reward_amount == Decimal::ZERO {
            return Ok("success".to_string());
        }

        let mut state = self
            .require_milestone_state(contract_id, milestone_index)
            .await?;

        if state.released {
            info!(
                issue = issue.github_issue_number,
                milestone_index, "milestone already released on-chain; skipping payout"
            );
            return Ok("success".to_string());
        }

        if state.disputed && !state.resolved {
            return Err(AppError::bad_request(format!(
                "Milestone {milestone_index} on {contract_id} is disputed; resolve the dispute before release"
            )));
        }

        if let Some(receiver) = state.receiver.as_deref() {
            crate::infra::stellar::accounts::require_usdc_payout_account(&self.state, receiver)
                .await?;
        }

        let escrow_balance = self.current_balance(contract_id).await?;
        if escrow_balance <= Decimal::ZERO {
            return Err(AppError::bad_request(format!(
                "Escrow {contract_id} has balance 0; fund the escrow before releasing issue #{}",
                issue.github_issue_number
            )));
        }
        if let Some(amount) = state.amount {
            if escrow_balance < amount {
                return Err(AppError::bad_request(format!(
                    "Escrow balance {escrow_balance} USDC is below milestone amount {amount} USDC for issue #{}",
                    issue.github_issue_number
                )));
            }
        }

        let evidence = format!(
            "Trustless-OSS automatic payout: GitHub issue #{} closed after merged PR ({})",
            issue.github_issue_number, issue.title
        );

        // 1) Complete — TW approve requires a non-empty milestone status
        info!(
            issue = issue.github_issue_number,
            milestone_index, "TW step 1/3: complete milestone"
        );
        self.complete_milestone(contract_id, milestone_index, platform_key, &evidence)
            .await?;
        state = self
            .require_milestone_state(contract_id, milestone_index)
            .await?;
        if !state.is_completed() {
            return Err(AppError::internal(format!(
                "Milestone {milestone_index} on {contract_id} did not reach status=completed after change-milestone-status"
            )));
        }

        // 2) Approve — only after completed is confirmed on-chain
        info!(
            issue = issue.github_issue_number,
            milestone_index, "TW step 2/3: approve milestone"
        );
        self.approve_milestone(contract_id, milestone_index, platform_key)
            .await?;
        state = self
            .require_milestone_state(contract_id, milestone_index)
            .await?;
        if !state.approved {
            return Err(AppError::internal(format!(
                "Milestone {milestone_index} on {contract_id} was not approved after approve-milestone"
            )));
        }

        if state.released {
            if let Err(error) = self.sync_balance(repo).await {
                tracing::warn!(%error, "failed to sync escrow balance after release");
            }
            return Ok("success".to_string());
        }

        // 3) Release — only after approved is confirmed on-chain
        info!(
            issue = issue.github_issue_number,
            milestone_index, "TW step 3/3: release milestone funds"
        );
        let tx_hash = self
            .release_milestone_funds(contract_id, milestone_index, platform_key)
            .await?;

        state = self
            .require_milestone_state(contract_id, milestone_index)
            .await?;
        if !state.released {
            return Err(AppError::internal(format!(
                "Milestone {milestone_index} on {contract_id} was not marked released after release-milestone-funds"
            )));
        }

        if let Err(error) = self.sync_balance(repo).await {
            tracing::warn!(%error, "failed to sync escrow balance after release");
        }

        Ok(tx_hash)
    }

    async fn require_milestone_state(
        &self,
        contract_id: &str,
        milestone_index: i32,
    ) -> Result<MilestoneChainState, AppError> {
        self.fetch_milestone_state(contract_id, milestone_index)
            .await?
            .ok_or_else(|| {
                AppError::bad_request(format!(
                    "milestone {milestone_index} does not exist on escrow {contract_id}"
                ))
            })
    }

    async fn complete_milestone(
        &self,
        contract_id: &str,
        milestone_index: i32,
        platform_key: &str,
        evidence: &str,
    ) -> Result<(), AppError> {
        let state = self
            .require_milestone_state(contract_id, milestone_index)
            .await?;
        if state.is_completed() {
            return Ok(());
        }

        let response = tw_fetch(
            &self.state,
            "/escrow/multi-release/change-milestone-status",
            Method::POST,
            Some(json!({
                "contractId": contract_id,
                "milestoneIndex": milestone_index.to_string(),
                "newStatus": "completed",
                "newEvidence": evidence,
                "serviceProvider": platform_key,
            })),
        )
        .await?;
        self.sign_platform_transaction(&response).await?;
        Ok(())
    }

    async fn approve_milestone(
        &self,
        contract_id: &str,
        milestone_index: i32,
        platform_key: &str,
    ) -> Result<(), AppError> {
        let state = self
            .require_milestone_state(contract_id, milestone_index)
            .await?;
        if state.approved {
            return Ok(());
        }

        match tw_fetch(
            &self.state,
            "/escrow/multi-release/approve-milestone",
            Method::POST,
            Some(json!({
                "approver": platform_key,
                "contractId": contract_id,
                "milestoneIndex": milestone_index.to_string(),
            })),
        )
        .await
        {
            Ok(response) => self.sign_platform_transaction(&response).await?,
            Err(error) => {
                if self
                    .require_milestone_state(contract_id, milestone_index)
                    .await?
                    .approved
                {
                    return Ok(());
                }
                return Err(error);
            }
        }

        Ok(())
    }

    async fn release_milestone_funds(
        &self,
        contract_id: &str,
        milestone_index: i32,
        platform_key: &str,
    ) -> Result<String, AppError> {
        match tw_fetch(
            &self.state,
            "/escrow/multi-release/release-milestone-funds",
            Method::POST,
            Some(json!({
                "releaseSigner": platform_key,
                "contractId": contract_id,
                "milestoneIndex": milestone_index.to_string(),
            })),
        )
        .await
        {
            Ok(response) => {
                let unsigned = unsigned_transaction(&response)?;
                let result = sign_and_send_transaction(&self.state, unsigned, None).await?;
                Ok(result
                    .get("hash")
                    .or_else(|| result.get("transactionHash"))
                    .and_then(Value::as_str)
                    .unwrap_or("success")
                    .to_string())
            }
            Err(error) => {
                if self
                    .require_milestone_state(contract_id, milestone_index)
                    .await?
                    .released
                {
                    Ok("success".to_string())
                } else if error.to_string().contains("Escrow not found") {
                    Err(AppError::bad_request(format!(
                        "Trustless Work could not release milestone {milestone_index} on {contract_id} \
                         (TW returned Escrow not found). Most often the milestone receiver account \
                         does not exist on Stellar or has no USDC trustline, or escrow balance is too low. \
                         TW detail: {error}"
                    )))
                } else {
                    Err(error)
                }
            }
        }
    }

    pub async fn fetch_milestone_state(
        &self,
        contract_id: &str,
        milestone_index: i32,
    ) -> Result<Option<MilestoneChainState>, AppError> {
        let escrow = self.fetch_escrow(contract_id).await?;
        let Some(milestone) =
            escrow
                .get("milestones")
                .and_then(Value::as_array)
                .and_then(|milestones| {
                    usize::try_from(milestone_index)
                        .ok()
                        .and_then(|index| milestones.get(index))
                })
        else {
            return Ok(None);
        };

        let flag = |name: &str| {
            milestone
                .get("flags")
                .and_then(|flags| flags.get(name))
                .and_then(Value::as_bool)
                .unwrap_or(false)
        };
        let status = milestone
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let status_released = status.eq_ignore_ascii_case("released");

        Ok(Some(MilestoneChainState {
            index: milestone_index,
            status,
            released: flag("released") || status_released,
            approved: flag("approved"),
            disputed: flag("disputed"),
            resolved: flag("resolved"),
            receiver: milestone
                .get("receiver")
                .and_then(Value::as_str)
                .map(str::to_string),
            amount: milestone.get("amount").and_then(decimal_from_value),
        }))
    }

    pub async fn refund(
        &self,
        repo: &Repo,
        maintainer_wallet: &str,
    ) -> Result<(Decimal, i64), AppError> {
        let platform_key = self.state.config.platform_stellar_public_key.as_str();
        let contract_id = repo
            .escrow_contract_id
            .as_deref()
            .ok_or_else(|| AppError::not_found("Repo or escrow not found"))?;
        let resolver_public_key = self
            .state
            .config
            .dispute_resolver_stellar_public_key
            .clone();
        let resolver_secret_key = self
            .state
            .config
            .dispute_resolver_stellar_secret_key
            .clone();
        let escrow_data = self.fetch_escrow(contract_id).await?;
        let milestones = escrow_data
            .get("milestones")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let is_dual_wallet = escrow_data
            .get("roles")
            .and_then(|roles| roles.get("disputeResolver"))
            .and_then(Value::as_str)
            == Some(resolver_public_key.as_str());

        let mut total_refunded = Decimal::ZERO;
        let mut remaining_balance = self.current_balance(contract_id).await?;

        if is_dual_wallet {
            for (index, milestone) in milestones.iter().enumerate() {
                if milestone_flag(milestone, "released") || milestone_flag(milestone, "resolved") {
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

                remaining_balance = remaining_balance.min(self.current_balance(contract_id).await?);
                let distribution_amount = resolution_distribution_amount(
                    milestone_amount,
                    remaining_balance,
                    self.state.config.is_mainnet(),
                );
                if distribution_amount <= Decimal::ZERO {
                    return Err(AppError::bad_request(format!(
                        "Cannot resolve milestone {index}: escrow has no available balance"
                    )));
                }

                let dispute = tw_fetch(
                    &self.state,
                    "/escrow/multi-release/dispute-milestone",
                    Method::POST,
                    Some(json!({
                        "signer": platform_key,
                        "contractId": contract_id,
                        "milestoneIndex": index.to_string(),
                    })),
                )
                .await;
                if let Ok(response) = dispute {
                    self.sign_platform_transaction_if_present(&response).await?;
                }

                let resolve = tw_fetch(
                    &self.state,
                    "/escrow/multi-release/resolve-milestone-dispute",
                    Method::POST,
                    Some(json!({
                        "disputeResolver": resolver_public_key,
                        "contractId": contract_id,
                        "milestoneIndex": index.to_string(),
                        "distributions": [{
                            "address": maintainer_wallet,
                            "amount": decimal_json_number(distribution_amount, "distribution amount")?,
                        }],
                    })),
                )
                .await?;
                if let Some(unsigned) = resolve.get("unsignedTransaction").and_then(Value::as_str) {
                    sign_and_send_transaction(&self.state, unsigned, Some(&resolver_secret_key))
                        .await?;
                    remaining_balance = (remaining_balance
                        - consumed_balance_amount(
                            distribution_amount,
                            self.state.config.is_mainnet(),
                        ))
                    .max(Decimal::ZERO);
                    total_refunded += distribution_amount;
                }
            }
        } else {
            let new_milestones: Vec<Value> = milestones
                .iter()
                .map(|milestone| {
                    if milestone_flag(milestone, "released") || milestone_flag(milestone, "resolved") {
                        return milestone.clone();
                    }

                    json!({
                        "description": format!(
                            "Refund: {}",
                            milestone.get("description").and_then(Value::as_str).unwrap_or("")
                        ),
                        "amount": milestone.get("amount").cloned().unwrap_or(json!(0)),
                        "receiver": maintainer_wallet,
                        "status": "pending",
                        "evidence": milestone.get("evidence").cloned().unwrap_or(json!("")),
                        "flags": { "approved": false, "released": false, "disputed": false, "resolved": false },
                    })
                })
                .collect();

            let mut payload = strip_escrow_metadata(&escrow_data);
            if let Some(object) = payload.as_object_mut() {
                object.insert("milestones".to_string(), json!(new_milestones));
                object.insert("isActive".to_string(), json!(true));
            }
            let update = tw_fetch(
                &self.state,
                "/escrow/multi-release/update-escrow",
                Method::PUT,
                Some(json!({
                    "signer": platform_key,
                    "contractId": contract_id,
                    "escrow": payload,
                })),
            )
            .await?;
            self.sign_platform_transaction_if_present(&update).await?;

            for (index, milestone) in new_milestones.iter().enumerate() {
                let description = milestone
                    .get("description")
                    .and_then(Value::as_str)
                    .unwrap_or("");
                if !description.starts_with("Refund:") {
                    continue;
                }
                let amount = milestone
                    .get("amount")
                    .and_then(decimal_from_value)
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
                    let response = tw_fetch(&self.state, path, Method::POST, Some(body)).await?;
                    self.sign_platform_transaction_if_present(&response).await?;
                }
                total_refunded += amount;
            }
        }

        let observed_balance = self.current_balance(contract_id).await?;
        let current_balance = if is_dual_wallet {
            remaining_balance.min(observed_balance)
        } else {
            observed_balance
        };
        if current_balance > Decimal::ZERO {
            let distribution_amount = resolution_distribution_amount(
                current_balance,
                current_balance,
                self.state.config.is_mainnet(),
            );
            let withdrawal = tw_fetch(
                &self.state,
                "/escrow/multi-release/withdraw-remaining-funds",
                Method::POST,
                Some(json!({
                    "contractId": contract_id,
                    "disputeResolver": if is_dual_wallet { resolver_public_key.clone() } else { platform_key.to_string() },
                    "distributions": [{
                        "address": maintainer_wallet,
                        "amount": decimal_json_number(distribution_amount, "distribution amount")?,
                    }],
                })),
            )
            .await?;
            if let Some(unsigned) = withdrawal
                .get("unsignedTransaction")
                .and_then(Value::as_str)
            {
                sign_and_send_transaction(
                    &self.state,
                    unsigned,
                    if is_dual_wallet {
                        Some(&resolver_secret_key)
                    } else {
                        None
                    },
                )
                .await?;
                total_refunded += distribution_amount;
            }
        }

        let issues =
            crate::modules::bounty::repository::list_issues_to_cancel(&self.state, repo.id).await?;
        let issue_ids: Vec<_> = issues.iter().map(|issue| issue.id).collect();
        crate::modules::bounty::repository::fail_assignments_for_issues(&self.state, &issue_ids)
            .await?;
        for issue in &issues {
            update_issue_status(&self.state, issue.id, "cancelled", None).await?;
        }
        update_repo_escrow_balance(
            &self.state,
            repo.id,
            Decimal::ZERO,
            Some(repo.github_repo_id),
        )
        .await?;

        Ok((total_refunded, issues.len() as i64))
    }

    async fn submit_signed_transaction(&self, signed_xdr: &str) -> Result<Value, AppError> {
        tw_fetch(
            &self.state,
            "/helper/send-transaction",
            Method::POST,
            Some(json!({ "signedXdr": signed_xdr })),
        )
        .await
    }

    async fn fetch_escrow(&self, contract_id: &str) -> Result<Value, AppError> {
        let response = tw_fetch(
            &self.state,
            &format!("/helper/get-escrow-by-contract-ids?contractIds[]={contract_id}"),
            Method::GET,
            None,
        )
        .await?;

        response
            .as_array()
            .and_then(|items| items.first())
            .cloned()
            .ok_or_else(|| AppError::internal(format!("Escrow not found: {contract_id}")))
    }

    async fn sign_platform_transaction(&self, response: &Value) -> Result<(), AppError> {
        sign_and_send_transaction(&self.state, unsigned_transaction(response)?, None).await?;
        Ok(())
    }

    async fn sign_platform_transaction_if_present(&self, response: &Value) -> Result<(), AppError> {
        let Some(unsigned) = response.get("unsignedTransaction").and_then(Value::as_str) else {
            return Ok(());
        };

        sign_and_send_transaction(&self.state, unsigned, None).await?;
        Ok(())
    }

    pub async fn current_balance(&self, contract_id: &str) -> Result<Decimal, AppError> {
        let contract_id = contract_id.trim();
        if contract_id.is_empty() {
            return Err(AppError::bad_request("Escrow contract id is empty"));
        }

        let response = tw_fetch(
            &self.state,
            &format!("/helper/get-multiple-escrow-balance?addresses[]={contract_id}"),
            Method::GET,
            None,
        )
        .await?;

        response
            .as_array()
            .and_then(|items| items.first())
            .and_then(|item| item.get("balance"))
            .and_then(decimal_from_value)
            .ok_or_else(|| {
                AppError::internal(format!(
                    "TrustlessWork get-multiple-escrow-balance missing balance for {contract_id}"
                ))
            })
    }

    async fn wait_for_balance_after_fund(
        &self,
        contract_id: &str,
        previous_balance: Decimal,
        funded_amount: Decimal,
    ) -> Result<Decimal, AppError> {
        let expected = (previous_balance + funded_amount).max(Decimal::ZERO);
        let mut last = previous_balance;

        for attempt in 0..10u32 {
            if attempt > 0 {
                tokio::time::sleep(std::time::Duration::from_millis(
                    350 + u64::from(attempt) * 150,
                ))
                .await;
            }
            last = self.current_balance(contract_id).await?;
            if last > previous_balance || last >= expected {
                return Ok(last);
            }
        }

        if last <= previous_balance {
            tracing::warn!(
                %contract_id,
                %previous_balance,
                %funded_amount,
                %last,
                "TW balance still flat after fund; applying funded amount locally"
            );
            return Ok(expected);
        }

        Ok(last)
    }
}

#[derive(Debug, Clone)]
pub struct MilestoneChainState {
    pub index: i32,
    pub status: String,
    pub released: bool,
    pub approved: bool,
    pub disputed: bool,
    pub resolved: bool,
    pub receiver: Option<String>,
    pub amount: Option<Decimal>,
}

impl MilestoneChainState {
    fn is_completed(&self) -> bool {
        self.status.eq_ignore_ascii_case("completed")
            || self.status.eq_ignore_ascii_case("released")
            || self.released
    }
}

fn unsigned_transaction(response: &Value) -> Result<&str, AppError> {
    response
        .get("unsignedTransaction")
        .and_then(Value::as_str)
        .ok_or_else(|| AppError::internal("TrustlessWork response missing unsignedTransaction"))
}

fn milestone_flag(milestone: &Value, name: &str) -> bool {
    milestone
        .get("flags")
        .and_then(|flags| flags.get(name))
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

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

fn strip_escrow_metadata(escrow_data: &Value) -> Value {
    let mut payload = escrow_data.clone();
    if let Some(object) = payload.as_object_mut() {
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
            object.remove(key);
        }
    }
    payload
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
    use crate::modules::escrow::trustless_work::api_client::decimal_json_number;

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

    #[test]
    fn completed_status_covers_completed_and_released() {
        let completed = MilestoneChainState {
            index: 1,
            status: "completed".into(),
            released: false,
            approved: false,
            disputed: false,
            resolved: false,
            receiver: None,
            amount: None,
        };
        let released = MilestoneChainState {
            index: 1,
            status: "pending".into(),
            released: true,
            approved: true,
            disputed: false,
            resolved: false,
            receiver: None,
            amount: None,
        };
        let pending = MilestoneChainState {
            index: 1,
            status: "pending".into(),
            released: false,
            approved: false,
            disputed: false,
            resolved: false,
            receiver: None,
            amount: None,
        };

        assert!(completed.is_completed());
        assert!(released.is_completed());
        assert!(!pending.is_completed());
    }
}
