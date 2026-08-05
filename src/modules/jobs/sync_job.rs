use rust_decimal::Decimal;
use tracing::info;

use crate::{
    error::AppError,
    modules::{
        escrow::trustless_work::client::tw_fetch,
        repo::repository::{list_active_escrow_repos, update_repo_escrow_balance},
    },
    state::AppState,
};

pub async fn sync_all_escrow_balances(state: &AppState) -> Result<(), AppError> {
    // info!("Starting escrow balance sync for all active repositories");
    // let repos = list_active_escrow_repos(state).await?;
    // if repos.is_empty() {
    //     info!("No active escrows found to sync");
    //     return Ok(());
    // }

    // let query_parts: Vec<String> = repos
    //     .iter()
    //     .filter_map(|repo| {
    //         repo.escrow_contract_id
    //             .as_ref()
    //             .map(|contract_id| format!("contractIds[]={contract_id}"))
    //     })
    //     .collect();

    // if query_parts.is_empty() {
    //     return Ok(());
    // }

    // let escrows = tw_fetch(
    //     state,
    //     &format!(
    //         "/helper/get-escrow-by-contract-ids?{}",
    //         query_parts.join("&")
    //     ),
    //     reqwest::Method::GET,
    //     None,
    // )
    // .await?;

    // let mut escrow_map = std::collections::HashMap::new();
    // if let Some(items) = escrows.as_array() {
    //     for escrow in items {
    //         if let (Some(contract_id), Some(balance)) = (
    //             escrow.get("contractId").and_then(|value| value.as_str()),
    //             escrow.get("balance").and_then(|value| value.as_f64()),
    //         ) {
    //             escrow_map.insert(
    //                 contract_id.to_string(),
    //                 Decimal::from_f64_retain(balance).unwrap_or(Decimal::ZERO),
    //             );
    //         }
    //     }
    // }

    // for repo in repos {
    //     let Some(contract_id) = repo.escrow_contract_id.clone() else {
    //         continue;
    //     };
    //     let Some(on_chain_balance) = escrow_map.get(&contract_id) else {
    //         continue;
    //     };
    //     if *on_chain_balance != repo.escrow_balance {
    //         update_repo_escrow_balance(
    //             state,
    //             repo.id,
    //             *on_chain_balance,
    //             Some(repo.github_repo_id),
    //         )
    //         .await?;
    //     }
    // }

    Ok(())
}
