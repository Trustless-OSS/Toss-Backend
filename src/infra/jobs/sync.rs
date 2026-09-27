use tracing::{info, warn};

use crate::{
    error::AppError, modules::escrow::repository::list_active_escrow_repos,
    modules::escrow::trustless_work::escrow_service::TrustlessWorkAPI, state::AppState,
};

pub(crate) async fn run(state: &AppState) -> Result<serde_json::Value, AppError> {
    let repos = list_active_escrow_repos(state).await?;

    let mut synced = 0usize;
    let mut failed = 0usize;

    for repo in &repos {
        match TrustlessWorkAPI::new(state.clone())
            .sync_balance(repo)
            .await
        {
            Ok(balance) => {
                synced += 1;
                let prev = repo.escrow_balance.unwrap_or_default();
                if balance != prev {
                    info!(
                        repo = %repo.full_name,
                        previous = %prev,
                        current = %balance,
                        "escrow balance reconciled"
                    );
                }
            }
            Err(error) => {
                // continue sweep; next run retries this repo
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
