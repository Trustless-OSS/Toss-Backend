use std::sync::LazyLock;

use regex::Regex;
use reqwest::Method;
use rust_decimal::Decimal;
use serde_json::{json, Value};
use tracing::warn;

use crate::{
    error::AppError,
    infra::stellar::signer::sign_and_send_transaction,
    modules::{
        escrow::trustless_work::client::tw_fetch,
        repo::repository::{get_repo_by_id, update_repo_escrow_balance},
    },
    shared::models::Repo,
    state::AppState,
};

static ISSUE_NUMBER_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)(?:close|closes|closed|fix|fixes|fixed|resolve|resolves|resolved)[:\s]*#\s*(\d+)",
    )
    .expect("valid issue number regex")
});

static MANUAL_AMOUNT_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)@Trustless-OSS\s+([\d.]+)").expect("valid manual amount regex")
});

static WORK_COMPLETION_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)@Trustless-OSS\s+/(pay|split|work|work-completion)\s+(\d+)")
        .expect("valid work completion regex")
});

static REJECTED_CMD_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)@Trustless-OSS\s+/(reject|rejected|no)").expect("valid reject regex")
});

static WALLET_CMD_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)@Trustless-OSS\s+/(wallet|address|connect|change-address)")
        .expect("valid wallet regex")
});

static HELP_CMD_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)@Trustless-OSS\s+/help").expect("valid help regex"));

pub fn extract_issue_number(body: Option<&str>) -> Option<i32> {
    let body = body?;
    ISSUE_NUMBER_RE
        .captures(body)
        .and_then(|caps| caps.get(1))
        .and_then(|m| m.as_str().parse().ok())
}

pub fn extract_manual_amount(body: Option<&str>) -> Option<Decimal> {
    let body = body?;
    let amount_str = MANUAL_AMOUNT_RE.captures(body)?.get(1)?.as_str();
    let amount: Decimal = amount_str.parse().ok()?;
    if amount <= Decimal::ZERO {
        return None;
    }
    Some(amount)
}

pub fn has_rejected_label(labels: &[Value]) -> bool {
    labels.iter().any(|label| {
        label
            .get("name")
            .and_then(|name| name.as_str())
            .is_some_and(|name| name.eq_ignore_ascii_case("rejected"))
    })
}

pub fn is_privileged_association(association: &str) -> bool {
    matches!(association, "OWNER" | "MEMBER" | "COLLABORATOR")
}

pub fn work_completion_percentage(body: &str) -> Option<i32> {
    WORK_COMPLETION_RE
        .captures(body)
        .and_then(|caps| caps.get(2))
        .and_then(|m| m.as_str().parse().ok())
}

pub fn is_reject_command(body: &str) -> bool {
    REJECTED_CMD_RE.is_match(body)
}

pub fn is_wallet_command(body: &str) -> bool {
    WALLET_CMD_RE.is_match(body)
}

pub fn is_help_command(body: &str) -> bool {
    HELP_CMD_RE.is_match(body)
}

pub fn is_retry_command(body: &str) -> bool {
    body.contains("@Trustless-OSS /retry")
}

pub fn labels_from_payload(issue: &Value) -> Vec<Value> {
    issue
        .get("labels")
        .and_then(|labels| labels.as_array())
        .cloned()
        .unwrap_or_default()
}

pub fn explorer_tx_url(state: &AppState, tx_hash: &str, contract_id: &str) -> String {
    let network = if state.config.is_mainnet() {
        "public"
    } else {
        "testnet"
    };
    if tx_hash != "success" {
        format!("https://stellar.expert/explorer/{network}/tx/{tx_hash}")
    } else {
        format!("https://stellar.expert/explorer/{network}/contract/{contract_id}")
    }
}

pub async fn sync_repo_balance(state: &AppState, repo: &mut Repo) -> Result<(), AppError> {
    let contract_id = match repo.escrow_contract_id.as_deref() {
        Some(id) => id,
        None => return Ok(()),
    };

    let escrow_array = tw_fetch(
        state,
        &format!("/helper/get-escrow-by-contract-ids?contractIds[]={contract_id}"),
        Method::GET,
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
        repo.escrow_balance = on_chain_balance;
    }

    Ok(())
}

pub async fn dispute_milestone(
    state: &AppState,
    contract_id: &str,
    milestone_index: i32,
    platform_key: &str,
) -> Result<(), AppError> {
    let result = tw_fetch(
        state,
        "/escrow/multi-release/dispute-milestone",
        Method::POST,
        Some(json!({
            "signer": platform_key,
            "contractId": contract_id,
            "milestoneIndex": milestone_index.to_string(),
        })),
    )
    .await;

    match result {
        Ok(response) => {
            if let Some(unsigned) = response.get("unsignedTransaction").and_then(|v| v.as_str()) {
                sign_and_send_transaction(state, unsigned, None).await?;
            }
            Ok(())
        }
        Err(error) if error.to_string().contains("already in dispute") => Ok(()),
        Err(error) => Err(error),
    }
}

pub async fn resolve_milestone_dispute(
    state: &AppState,
    repo: &Repo,
    milestone_index: i32,
    distributions: Vec<Value>,
) -> Result<(), AppError> {
    let platform_key = state.config.platform_stellar_public_key.as_str();
    let contract_id = repo
        .escrow_contract_id
        .as_deref()
        .ok_or_else(|| AppError::bad_request("No escrow deployed"))?;
    let resolver_pub = &state.config.dispute_resolver_stellar_public_key;
    let resolver_secret = &state.config.dispute_resolver_stellar_secret_key;

    let escrow_array = tw_fetch(
        state,
        &format!("/helper/get-escrow-by-contract-ids?contractIds[]={contract_id}"),
        Method::GET,
        None,
    )
    .await?;

    let is_dual_wallet = escrow_array
        .as_array()
        .and_then(|items| items.first())
        .and_then(|item| item.get("roles"))
        .and_then(|roles| roles.get("disputeResolver"))
        .and_then(|value| value.as_str())
        == Some(resolver_pub.as_str());

    let result = tw_fetch(
        state,
        "/escrow/multi-release/resolve-milestone-dispute",
        Method::POST,
        Some(json!({
            "disputeResolver": if is_dual_wallet { resolver_pub.clone() } else { platform_key.to_string() },
            "contractId": contract_id,
            "milestoneIndex": milestone_index.to_string(),
            "distributions": distributions,
        })),
    )
    .await;

    match result {
        Ok(response) => {
            if let Some(unsigned) = response.get("unsignedTransaction").and_then(|v| v.as_str()) {
                sign_and_send_transaction(
                    state,
                    unsigned,
                    if is_dual_wallet {
                        Some(resolver_secret.as_str())
                    } else {
                        None
                    },
                )
                .await?;
            }
            Ok(())
        }
        Err(error) => {
            let message = error.to_string();
            if message.contains("already resolved") || message.contains("already released") {
                Ok(())
            } else {
                Err(error)
            }
        }
    }
}

pub fn strip_escrow_metadata(escrow_data: &Value) -> Value {
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

pub async fn zero_milestone_on_chain(
    state: &AppState,
    repo: &Repo,
    milestone_index: i32,
) -> Result<(), AppError> {
    let platform_key = state.config.platform_stellar_public_key.as_str();
    let contract_id = repo
        .escrow_contract_id
        .as_deref()
        .ok_or_else(|| AppError::bad_request("No escrow deployed"))?;

    let escrow_array = tw_fetch(
        state,
        &format!("/helper/get-escrow-by-contract-ids?contractIds[]={contract_id}"),
        Method::GET,
        None,
    )
    .await?;

    let escrow_data = escrow_array
        .as_array()
        .and_then(|items| items.first())
        .cloned()
        .ok_or_else(|| AppError::internal("Escrow not found on-chain"))?;

    let mut new_milestones = escrow_data
        .get("milestones")
        .and_then(|value| value.as_array())
        .cloned()
        .unwrap_or_default();

    let index = milestone_index as usize;
    if index >= new_milestones.len() {
        return Ok(());
    }

    let mut milestone = new_milestones[index].clone();
    if let Some(obj) = milestone.as_object_mut() {
        obj.insert("amount".to_string(), json!(0));
        obj.insert("receiver".to_string(), json!(platform_key));
    }
    new_milestones[index] = milestone;

    let mut payload = strip_escrow_metadata(&escrow_data);
    if let Some(obj) = payload.as_object_mut() {
        obj.insert("milestones".to_string(), json!(new_milestones));
    }

    let update_res = tw_fetch(
        state,
        "/escrow/multi-release/update-escrow",
        Method::PUT,
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

    Ok(())
}

pub async fn refresh_repo(state: &AppState, repo_id: uuid::Uuid) -> Result<Option<Repo>, AppError> {
    get_repo_by_id(state, repo_id).await
}

pub fn split_amounts(reward: Decimal, percentage: i32) -> (Decimal, Decimal) {
    let pct = Decimal::from(percentage);
    let hundred = Decimal::from(100);
    let contributor = (reward * pct / hundred).round_dp(7);
    let maintainer = (reward - contributor).round_dp(7);
    (contributor, maintainer)
}

pub fn maintainer_github_id(repo: &Repo) -> i64 {
    repo.installer_github_id.unwrap_or(repo.owner_github_id)
}

pub async fn cancel_bounty_with_refund(
    state: &AppState,
    repo: &Repo,
    issue_id: uuid::Uuid,
    reward_amount: Decimal,
) -> Result<(), AppError> {
    use crate::modules::repo::repository::{
        cancel_issue, delete_assignments_for_issue, refund_repo_balance,
    };

    cancel_issue(state, issue_id).await?;
    delete_assignments_for_issue(state, issue_id).await?;
    refund_repo_balance(state, repo, reward_amount).await
}

pub fn log_warn_missing_milestone() {
    warn!("missing milestone index for active issue");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_linked_issue_from_supported_closing_phrases() {
        assert_eq!(extract_issue_number(Some("Closes #42")), Some(42));
        assert_eq!(extract_issue_number(Some("fixes: # 17")), Some(17));
        assert_eq!(extract_issue_number(Some("related to #9")), None);
    }

    #[test]
    fn parses_positive_manual_amounts_only() {
        assert_eq!(
            extract_manual_amount(Some("@Trustless-OSS 12.50 USDC")),
            Some("12.50".parse().unwrap())
        );
        assert_eq!(extract_manual_amount(Some("@Trustless-OSS 0")), None);
        assert_eq!(extract_manual_amount(Some("@Trustless-OSS nope")), None);
    }

    #[test]
    fn recognizes_commands_case_insensitively() {
        assert_eq!(
            work_completion_percentage("@trustless-oss /WORK-COMPLETION 75"),
            Some(75)
        );
        assert!(is_reject_command("@Trustless-OSS /rejected"));
        assert!(is_wallet_command("@Trustless-OSS /change-address"));
        assert!(is_help_command("@trustless-oss /HELP"));
    }

    #[test]
    fn splits_reward_without_losing_precision() {
        let reward: Decimal = "10.0000001".parse().unwrap();
        let (contributor, maintainer) = split_amounts(reward, 75);
        assert_eq!(contributor, "7.5000001".parse().unwrap());
        assert_eq!(maintainer, "2.5000000".parse().unwrap());
        assert_eq!(contributor + maintainer, reward);
    }

    #[test]
    fn detects_rejected_label_case_insensitively() {
        assert!(has_rejected_label(&[json!({ "name": "Rejected" })]));
        assert!(!has_rejected_label(&[json!({ "name": "rewarded" })]));
    }
}
