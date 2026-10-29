use rust_decimal::Decimal;
use uuid::Uuid;

use crate::{
    error::{map_db_err, require_db, AppError},
    modules::repo::repository::invalidate_repo_cache,
    shared::models::{schema, Repo},
    state::AppState,
};

fn round_balance(value: Decimal) -> Decimal {
    value.round_dp(7)
}

pub async fn update_repo_escrow_contract(
    state: &AppState,
    repo_id: Uuid,
    contract_id: &str,
) -> Result<(), AppError> {
    let mut db = require_db(&state.db)?;
    toasty::update!(schema::Repository::filter_by_id(repo_id) {
        escrow_contract_id: Some(contract_id.to_string()),
    })
    .exec(&mut db)
    .await
    .map_err(map_db_err)?;
    invalidate_repo_cache(state, repo_id, None).await;
    Ok(())
}

pub async fn update_repo_escrow_balance(
    state: &AppState,
    repo_id: Uuid,
    balance: Decimal,
    github_repo_id: Option<i64>,
) -> Result<(), AppError> {
    let mut db = require_db(&state.db)?;
    toasty::update!(schema::Repository::filter_by_id(repo_id) {
        escrow_balance: Some(round_balance(balance)),
    })
    .exec(&mut db)
    .await
    .map_err(map_db_err)?;
    invalidate_repo_cache(state, repo_id, github_repo_id).await;
    Ok(())
}

pub async fn clear_repo_escrow(state: &AppState, repo_id: Uuid) -> Result<(), AppError> {
    let mut db = require_db(&state.db)?;
    toasty::update!(schema::Repository::filter_by_id(repo_id) {
        escrow_contract_id: Option::<String>::None,
        escrow_balance: Option::<Decimal>::None,
    })
    .exec(&mut db)
    .await
    .map_err(map_db_err)?;
    invalidate_repo_cache(state, repo_id, None).await;
    Ok(())
}

pub async fn list_active_escrow_repos(state: &AppState) -> Result<Vec<Repo>, AppError> {
    let mut db = require_db(&state.db)?;
    Ok(
        schema::Repository::filter(schema::Repository::fields().escrow_contract_id().is_some())
            .exec(&mut db)
            .await
            .map_err(map_db_err)?
            .into_iter()
            .map(|repo| Repo::from_parts(repo, vec![]))
            .filter(|repo| {
                repo.escrow_contract_id
                    .as_deref()
                    .is_some_and(|id| !id.trim().is_empty())
            })
            .collect(),
    )
}

pub async fn refund_repo_balance(
    state: &AppState,
    repo: &Repo,
    amount: Option<Decimal>,
) -> Result<(), AppError> {
    let current = repo.escrow_balance.unwrap_or(Decimal::ZERO);
    let new_balance = current + amount.unwrap_or(Decimal::ZERO);
    update_repo_escrow_balance(state, repo.id, new_balance, Some(repo.github_repo_id)).await
}
