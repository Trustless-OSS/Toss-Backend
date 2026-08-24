//! The `escrow-balance-sync` processor.
//!
//! Driven by a BullMQ job scheduler, so exactly one sync is pending at any time
//! instead of a new list entry accumulating every interval.

use tracing::{info, warn};

use crate::{
    error::AppError, modules::escrow::repository::list_active_escrow_repos,
    modules::escrow::service::sync_repo_escrow_balance, state::AppState,
};

/// Reconcile every deployed escrow's balance with the chain.
pub(crate) async fn run(state: &AppState) -> Result<serde_json::Value, AppError> {
    let repos = list_active_escrow_repos(state).await?;

    let mut synced = 0usize;
    let mut failed = 0usize;

    for repo in &repos {
        match sync_repo_escrow_balance(state, repo).await {
            Ok(balance) => {
                synced += 1;
                if balance != repo.escrow_balance {
                    info!(
                        repo = %repo.full_name,
                        previous = %repo.escrow_balance,
                        current = %balance,
                        "escrow balance reconciled"
                    );
                }
            }
            Err(error) => {
                // One unreachable escrow must not abort the whole sweep; the next
                // scheduled run picks it up again.
                failed += 1;
                warn!(%error, repo = %repo.full_name, "escrow balance sync failed for repo");
            }
        }
    }

    Ok(serde_json::json!({
        "repos": repos.len(),
        "synced": synced,
        "failed": failed,
    }))
}
