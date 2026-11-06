use rust_decimal::Decimal;
use uuid::Uuid;

use crate::{
    error::{map_db_err, require_db, AppError},
    modules::repo::repository::invalidate_repo_cache,
    shared::models::{schema, Bounty, Repo},
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

pub async fn finalize_refund(
    state: &AppState,
    repo: &Repo,
    issues: Vec<Bounty>,
) -> Result<Vec<Bounty>, AppError> {
    let mut db = require_db(&state.db)?;
    let issues = finalize_refund_db(&mut db, repo, issues).await?;
    invalidate_repo_cache(state, repo.id, Some(repo.github_repo_id)).await;
    Ok(issues)
}

async fn finalize_refund_db(
    db: &mut toasty::Db,
    repo: &Repo,
    mut issues: Vec<Bounty>,
) -> Result<Vec<Bounty>, AppError> {
    let contract_id = repo
        .escrow_contract_id
        .as_deref()
        .ok_or_else(|| AppError::not_found("Repo or escrow not found"))?;
    let mut tx = db.transaction().await.map_err(map_db_err)?;
    let updated = toasty::sql::statement(
        "UPDATE repositories SET escrow_balance = $1 WHERE id = $2 AND escrow_contract_id = $3",
    )
    .bind(Decimal::ZERO)
    .bind(repo.id)
    .bind(contract_id.to_string())
    .exec(&mut tx)
    .await
    .map_err(map_db_err)?;
    if updated != 1 {
        return Err(AppError::internal(
            "Escrow changed during refund; reconciliation is required",
        ));
    }

    let mut cancelled = std::collections::HashSet::new();
    for chunk in issues.chunks(500) {
        let placeholders = (2..chunk.len() + 2)
            .map(|index| format!("${index}"))
            .collect::<Vec<_>>()
            .join(", ");
        let mut query = toasty::sql::query(format!(
            "UPDATE bounties SET status = 'cancelled' WHERE repo_id = $1 AND id IN ({placeholders}) AND status IN ('open', 'assigned') RETURNING id"
        ))
        .bind(repo.id);
        for issue in chunk {
            query = query.bind(issue.id);
        }
        for row in query.exec(&mut tx).await.map_err(map_db_err)? {
            match row.as_record().and_then(|record| record.first()) {
                Some(toasty::stmt::Value::Uuid(id)) => {
                    cancelled.insert(*id);
                }
                _ => return Err(AppError::internal("Invalid refund cancellation result")),
            }
        }
    }
    tx.commit().await.map_err(map_db_err)?;
    issues.retain(|issue| cancelled.contains(&issue.id));
    Ok(issues)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    #[ignore = "requires SECURITY_TEST_DATABASE_URL pointing to disposable toss_security_tests"]
    async fn refund_finalization_is_atomic_and_preserves_newly_paid_bounties() {
        let url = std::env::var("SECURITY_TEST_DATABASE_URL").unwrap();
        assert!(url.ends_with("/toss_security_tests"));
        let mut db = crate::infra::db::connect(&url).await.unwrap();
        let unique = Uuid::new_v4();
        let github_id = (unique.as_u128() & 0x3fff_ffff_ffff_ffff) as i64;
        let repo: Repo = schema::Repository::create()
            .github_repo_id(github_id)
            .full_name(format!("refund/test-{unique}"))
            .escrow_contract_id(Some("COLD".to_string()))
            .escrow_balance(Some(Decimal::from(10)))
            .exec(&mut db)
            .await
            .unwrap()
            .into();
        let open: Bounty = schema::Bounty::create()
            .repo_id(repo.id)
            .github_issue_id(github_id + 1)
            .github_issue_number(1)
            .status("open".to_string())
            .exec(&mut db)
            .await
            .unwrap()
            .into();
        let now_paid: Bounty = schema::Bounty::create()
            .repo_id(repo.id)
            .github_issue_id(github_id + 2)
            .github_issue_number(2)
            .status("assigned".to_string())
            .exec(&mut db)
            .await
            .unwrap()
            .into();
        let other_repo: Repo = schema::Repository::create()
            .github_repo_id(github_id + 3)
            .full_name(format!("refund/other-{unique}"))
            .exec(&mut db)
            .await
            .unwrap()
            .into();
        let other_issue: Bounty = schema::Bounty::create()
            .repo_id(other_repo.id)
            .github_issue_id(github_id + 4)
            .github_issue_number(1)
            .status("open".to_string())
            .exec(&mut db)
            .await
            .unwrap()
            .into();
        toasty::update!(schema::Bounty::filter_by_id(now_paid.id) {
            status: "paid".to_string(),
        })
        .exec(&mut db)
        .await
        .unwrap();

        let issues = vec![open.clone(), now_paid.clone(), other_issue.clone()];
        let mut stale_repo = repo.clone();
        stale_repo.escrow_contract_id = Some("COTHER".to_string());
        assert!(finalize_refund_db(&mut db, &stale_repo, issues.clone())
            .await
            .is_err());
        let unchanged = schema::Repository::get_by_id(&mut db, &repo.id)
            .await
            .unwrap();
        assert_eq!(unchanged.escrow_balance, Some(Decimal::from(10)));
        let unchanged_issue = schema::Bounty::get_by_id(&mut db, &open.id).await.unwrap();
        assert_eq!(unchanged_issue.status, "open");

        let cancelled = finalize_refund_db(&mut db, &repo, issues).await.unwrap();
        assert_eq!(cancelled.len(), 1);
        assert_eq!(cancelled[0].id, open.id);
        let paid = schema::Bounty::get_by_id(&mut db, &now_paid.id)
            .await
            .unwrap();
        assert_eq!(paid.status, "paid");
        let other = schema::Bounty::get_by_id(&mut db, &other_issue.id)
            .await
            .unwrap();
        assert_eq!(other.status, "open");
        let final_repo = schema::Repository::get_by_id(&mut db, &repo.id)
            .await
            .unwrap();
        assert_eq!(final_repo.escrow_balance, Some(Decimal::ZERO));
    }
}
