use diesel::prelude::*;
use diesel::result::{DatabaseErrorKind, Error as DieselError};
use diesel_async::scoped_futures::ScopedFutureExt;
use diesel_async::{AsyncConnection, RunQueryDsl};
use rust_decimal::Decimal;
use uuid::Uuid;

use crate::{
    error::{get_conn, AppError},
    infra::cache_keys,
    schema::{assignments, contributors, issues, repos},
    shared::models::{Assignment, Contributor, Issue, Repo},
    state::AppState,
};

fn db_err<E: std::fmt::Display>(error: E) -> AppError {
    AppError::database(error.to_string())
}

pub async fn get_repo_by_id(state: &AppState, repo_id: Uuid) -> Result<Option<Repo>, AppError> {
    let cache_key = cache_keys::repo(repo_id);
    if let Some(cached) = state.cache.get::<Repo>(&cache_key).await {
        return Ok(Some(cached));
    }

    let mut conn = get_conn(&state.db).await?;
    let repo = repos::table
        .filter(repos::id.eq(repo_id))
        .select(Repo::as_select())
        .first(&mut conn)
        .await
        .optional()
        .map_err(db_err)?;

    if let Some(ref repo) = repo {
        state
            .cache
            .set(&cache_key, repo, Some(cache_keys::REPO_TTL))
            .await;
        state
            .cache
            .set(
                &cache_keys::repo_by_github_id(repo.github_repo_id),
                repo,
                Some(cache_keys::REPO_TTL),
            )
            .await;
    }

    Ok(repo)
}

pub async fn get_repo_by_github_id(
    state: &AppState,
    github_repo_id: i64,
) -> Result<Option<Repo>, AppError> {
    let cache_key = cache_keys::repo_by_github_id(github_repo_id);
    if let Some(cached) = state.cache.get::<Repo>(&cache_key).await {
        return Ok(Some(cached));
    }

    let mut conn = get_conn(&state.db).await?;
    let repo = repos::table
        .filter(repos::github_repo_id.eq(github_repo_id))
        .select(Repo::as_select())
        .first(&mut conn)
        .await
        .optional()
        .map_err(db_err)?;

    if let Some(ref repo) = repo {
        state
            .cache
            .set(&cache_key, repo, Some(cache_keys::REPO_TTL))
            .await;
        state
            .cache
            .set(&cache_keys::repo(repo.id), repo, Some(cache_keys::REPO_TTL))
            .await;
    }

    Ok(repo)
}

pub async fn invalidate_repo_cache(state: &AppState, repo_id: Uuid, github_repo_id: Option<i64>) {
    state.cache.invalidate(&cache_keys::repo(repo_id)).await;
    if let Some(github_repo_id) = github_repo_id {
        state
            .cache
            .invalidate(&cache_keys::repo_by_github_id(github_repo_id))
            .await;
        state
            .cache
            .invalidate(&cache_keys::gh_token(github_repo_id))
            .await;
    }
}

pub async fn list_repos_for_user(
    state: &AppState,
    github_id: i64,
    github_username: Option<&str>,
    limit: i64,
    offset: i64,
) -> Result<(Vec<Repo>, i64), AppError> {
    let mut conn = get_conn(&state.db).await?;

    if github_id == 0 {
        if let Some(username) = github_username.filter(|value| !value.is_empty()) {
            let total: i64 = repos::table
                .filter(repos::owner_username.eq(username))
                .count()
                .get_result(&mut conn)
                .await
                .map_err(db_err)?;

            let repos_list = repos::table
                .filter(repos::owner_username.eq(username))
                .order(repos::created_at.desc())
                .limit(limit)
                .offset(offset)
                .select(Repo::as_select())
                .load(&mut conn)
                .await
                .map_err(db_err)?;

            return Ok((repos_list, total));
        }

        return Err(AppError::bad_request(
            "Could not determine GitHub identity from session",
        ));
    }

    // (owner_github_id = $1 OR installer_github_id = $1)
    //   AND COALESCE(is_fork, false) = false
    //   AND COALESCE(is_private, false) = false
    //   AND (owner_type = 'User' OR installer_github_id = $1)
    let total: i64 = repos::table
        .filter(
            repos::owner_github_id
                .eq(github_id)
                .or(repos::installer_github_id.eq(github_id)),
        )
        .filter(repos::is_fork.is_null().or(repos::is_fork.eq(false)))
        .filter(repos::is_private.is_null().or(repos::is_private.eq(false)))
        .filter(
            repos::owner_type
                .eq("User")
                .or(repos::installer_github_id.eq(github_id)),
        )
        .count()
        .get_result(&mut conn)
        .await
        .map_err(db_err)?;

    let repos_list = repos::table
        .filter(
            repos::owner_github_id
                .eq(github_id)
                .or(repos::installer_github_id.eq(github_id)),
        )
        .filter(repos::is_fork.is_null().or(repos::is_fork.eq(false)))
        .filter(repos::is_private.is_null().or(repos::is_private.eq(false)))
        .filter(
            repos::owner_type
                .eq("User")
                .or(repos::installer_github_id.eq(github_id)),
        )
        .order(repos::created_at.desc())
        .limit(limit)
        .offset(offset)
        .select(Repo::as_select())
        .load(&mut conn)
        .await
        .map_err(db_err)?;

    Ok((repos_list, total))
}

pub async fn upsert_repo(
    state: &AppState,
    github_repo_id: i64,
    full_name: &str,
    owner_github_id: i64,
    owner_username: &str,
) -> Result<Repo, AppError> {
    let mut conn = get_conn(&state.db).await?;
    diesel::insert_into(repos::table)
        .values((
            repos::github_repo_id.eq(github_repo_id),
            repos::full_name.eq(full_name),
            repos::owner_github_id.eq(owner_github_id),
            repos::owner_username.eq(owner_username),
            repos::reward_low.eq(Decimal::from(1)),
            repos::reward_medium.eq(Decimal::from(2)),
            repos::reward_high.eq(Decimal::from(3)),
        ))
        .on_conflict(repos::github_repo_id)
        .do_update()
        .set((
            repos::full_name.eq(diesel::upsert::excluded(repos::full_name)),
            repos::owner_github_id.eq(diesel::upsert::excluded(repos::owner_github_id)),
            repos::owner_username.eq(diesel::upsert::excluded(repos::owner_username)),
        ))
        .returning(Repo::as_returning())
        .get_result(&mut conn)
        .await
        .map_err(db_err)
}

pub async fn update_repo_escrow_contract(
    state: &AppState,
    repo_id: Uuid,
    contract_id: &str,
) -> Result<(), AppError> {
    let mut conn = get_conn(&state.db).await?;
    diesel::update(repos::table.filter(repos::id.eq(repo_id)))
        .set(repos::escrow_contract_id.eq(contract_id))
        .execute(&mut conn)
        .await
        .map_err(db_err)?;
    invalidate_repo_cache(state, repo_id, None).await;
    Ok(())
}

pub async fn update_repo_escrow_funder_wallet(
    state: &AppState,
    repo_id: Uuid,
    funder_wallet: &str,
) -> Result<(), AppError> {
    let mut conn = get_conn(&state.db).await?;
    diesel::update(repos::table.filter(repos::id.eq(repo_id)))
        .set(repos::escrow_funder_wallet.eq(funder_wallet))
        .execute(&mut conn)
        .await
        .map_err(db_err)?;
    invalidate_repo_cache(state, repo_id, None).await;
    Ok(())
}

pub async fn update_repo_escrow_balance(
    state: &AppState,
    repo_id: Uuid,
    balance: Decimal,
    github_repo_id: Option<i64>,
) -> Result<(), AppError> {
    let mut conn = get_conn(&state.db).await?;
    diesel::update(repos::table.filter(repos::id.eq(repo_id)))
        .set(repos::escrow_balance.eq(balance))
        .execute(&mut conn)
        .await
        .map_err(db_err)?;
    invalidate_repo_cache(state, repo_id, github_repo_id).await;
    Ok(())
}

pub async fn update_repo_rewards(
    state: &AppState,
    repo_id: Uuid,
    reward_low: Decimal,
    reward_medium: Decimal,
    reward_high: Decimal,
) -> Result<Repo, AppError> {
    let mut conn = get_conn(&state.db).await?;
    let repo = diesel::update(repos::table.filter(repos::id.eq(repo_id)))
        .set((
            repos::reward_low.eq(reward_low),
            repos::reward_medium.eq(reward_medium),
            repos::reward_high.eq(reward_high),
        ))
        .returning(Repo::as_returning())
        .get_result(&mut conn)
        .await
        .map_err(db_err)?;

    invalidate_repo_cache(state, repo.id, Some(repo.github_repo_id)).await;
    Ok(repo)
}

pub async fn clear_repo_escrow(state: &AppState, repo_id: Uuid) -> Result<(), AppError> {
    let mut conn = get_conn(&state.db).await?;
    diesel::update(repos::table.filter(repos::id.eq(repo_id)))
        .set((
            repos::escrow_contract_id.eq(None::<String>),
            repos::escrow_funder_wallet.eq(None::<String>),
            repos::escrow_balance.eq(Decimal::ZERO),
        ))
        .execute(&mut conn)
        .await
        .map_err(db_err)?;
    invalidate_repo_cache(state, repo_id, None).await;
    Ok(())
}

pub async fn delete_repo_cascade(state: &AppState, repo_id: Uuid) -> Result<(), AppError> {
    let mut conn = get_conn(&state.db).await?;

    let issue_ids: Vec<Uuid> = issues::table
        .filter(issues::repo_id.eq(repo_id))
        .select(issues::id)
        .load(&mut conn)
        .await
        .map_err(db_err)?;

    if !issue_ids.is_empty() {
        diesel::delete(assignments::table.filter(assignments::issue_id.eq_any(&issue_ids)))
            .execute(&mut conn)
            .await
            .map_err(db_err)?;
    }

    diesel::delete(issues::table.filter(issues::repo_id.eq(repo_id)))
        .execute(&mut conn)
        .await
        .map_err(db_err)?;

    diesel::delete(repos::table.filter(repos::id.eq(repo_id)))
        .execute(&mut conn)
        .await
        .map_err(db_err)?;

    invalidate_repo_cache(state, repo_id, None).await;
    Ok(())
}

pub async fn count_repos_for_installation(
    state: &AppState,
    installation_id: i64,
    exclude_repo_id: Uuid,
) -> Result<i64, AppError> {
    let mut conn = get_conn(&state.db).await?;
    repos::table
        .filter(repos::github_installation_id.eq(installation_id))
        .filter(repos::id.ne(exclude_repo_id))
        .count()
        .get_result(&mut conn)
        .await
        .map_err(db_err)
}

pub async fn get_contributor_by_github_id(
    state: &AppState,
    github_user_id: i64,
) -> Result<Option<Contributor>, AppError> {
    let cache_key = cache_keys::contrib(github_user_id);
    if let Some(cached) = state.cache.get::<Contributor>(&cache_key).await {
        return Ok(Some(cached));
    }

    let mut conn = get_conn(&state.db).await?;
    let contributor = contributors::table
        .filter(contributors::github_user_id.eq(github_user_id))
        .select(Contributor::as_select())
        .first(&mut conn)
        .await
        .optional()
        .map_err(db_err)?;

    if let Some(ref contributor) = contributor {
        state
            .cache
            .set(&cache_key, contributor, Some(cache_keys::CONTRIB_TTL))
            .await;
    }

    Ok(contributor)
}

pub async fn invalidate_contributor_cache(state: &AppState, github_user_id: i64) {
    state
        .cache
        .invalidate(&cache_keys::contrib(github_user_id))
        .await;
}

pub async fn upsert_contributor_wallet(
    state: &AppState,
    github_user_id: i64,
    github_username: &str,
    payout_chain: &str,
    payout_address: &str,
) -> Result<(), AppError> {
    let mut conn = get_conn(&state.db).await?;
    let stellar_wallet = if payout_chain == "stellar" {
        Some(payout_address)
    } else {
        None
    };

    diesel::insert_into(contributors::table)
        .values((
            contributors::github_user_id.eq(github_user_id),
            contributors::github_username.eq(github_username),
            contributors::stellar_wallet.eq(stellar_wallet),
            contributors::payout_chain.eq(payout_chain),
            contributors::payout_address.eq(payout_address),
        ))
        .on_conflict(contributors::github_user_id)
        .do_update()
        .set((
            contributors::github_username
                .eq(diesel::upsert::excluded(contributors::github_username)),
            contributors::stellar_wallet.eq(diesel::upsert::excluded(contributors::stellar_wallet)),
            contributors::payout_chain.eq(diesel::upsert::excluded(contributors::payout_chain)),
            contributors::payout_address.eq(diesel::upsert::excluded(contributors::payout_address)),
        ))
        .execute(&mut conn)
        .await
        .map_err(db_err)?;

    invalidate_contributor_cache(state, github_user_id).await;
    Ok(())
}

pub async fn get_contributor_profile(
    state: &AppState,
    github_user_id: i64,
) -> Result<Option<Contributor>, AppError> {
    get_contributor_by_github_id(state, github_user_id).await
}

pub async fn is_maintainer(
    state: &AppState,
    github_user_id: i64,
    repo_id: Uuid,
) -> Result<bool, AppError> {
    let mut conn = get_conn(&state.db).await?;
    let row: Option<(i64, Option<i64>)> = repos::table
        .filter(repos::id.eq(repo_id))
        .select((repos::owner_github_id, repos::installer_github_id))
        .first(&mut conn)
        .await
        .optional()
        .map_err(db_err)?;

    Ok(row.is_some_and(|(owner, installer)| {
        owner == github_user_id || installer == Some(github_user_id)
    }))
}

pub async fn is_assigned_contributor(
    state: &AppState,
    github_user_id: i64,
    repo_id: Uuid,
    github_issue_id: i64,
) -> Result<bool, AppError> {
    let mut conn = get_conn(&state.db).await?;
    let assigned: Option<i64> = issues::table
        .inner_join(assignments::table.inner_join(contributors::table))
        .filter(issues::repo_id.eq(repo_id))
        .filter(issues::github_issue_id.eq(github_issue_id))
        .select(contributors::github_user_id)
        .first(&mut conn)
        .await
        .optional()
        .map_err(db_err)?;

    Ok(assigned == Some(github_user_id))
}

pub async fn list_issues_for_repo(
    state: &AppState,
    repo_id: Uuid,
    limit: i64,
    offset: i64,
) -> Result<(Vec<serde_json::Value>, i64), AppError> {
    let mut conn = get_conn(&state.db).await?;

    let total: i64 = issues::table
        .filter(issues::repo_id.eq(repo_id))
        .count()
        .get_result(&mut conn)
        .await
        .map_err(db_err)?;

    let issue_rows = issues::table
        .filter(issues::repo_id.eq(repo_id))
        .order(issues::created_at.desc())
        .limit(limit)
        .offset(offset)
        .select(Issue::as_select())
        .load(&mut conn)
        .await
        .map_err(db_err)?;

    let mut rows = Vec::with_capacity(issue_rows.len());
    for issue in issue_rows {
        let issue_assignments = assignments::table
            .filter(assignments::issue_id.eq(issue.id))
            .select(Assignment::as_select())
            .load(&mut conn)
            .await
            .map_err(db_err)?;

        let mut assignment_rows = Vec::new();
        for assignment in issue_assignments {
            let contributor = if let Some(contributor_id) = assignment.contributor_id {
                contributors::table
                    .filter(contributors::id.eq(contributor_id))
                    .select(Contributor::as_select())
                    .first(&mut conn)
                    .await
                    .optional()
                    .map_err(db_err)?
            } else {
                None
            };

            assignment_rows.push(serde_json::json!({
                "id": assignment.id,
                "issue_id": assignment.issue_id,
                "contributor_id": assignment.contributor_id,
                "assigned_at": assignment.assigned_at,
                "pr_number": assignment.pr_number,
                "pr_merged_at": assignment.pr_merged_at,
                "payout_status": assignment.payout_status,
                "completion_percentage": assignment.completion_percentage,
                "contributors": contributor,
            }));
        }

        rows.push(serde_json::json!({
            "id": issue.id,
            "repo_id": issue.repo_id,
            "github_issue_id": issue.github_issue_id,
            "github_issue_number": issue.github_issue_number,
            "title": issue.title,
            "reward_amount": issue.reward_amount,
            "difficulty_label": issue.difficulty_label,
            "milestone_index": issue.milestone_index,
            "status": issue.status,
            "created_at": issue.created_at,
            "assignments": assignment_rows,
        }));
    }

    Ok((rows, total))
}

pub async fn get_issue_by_id(state: &AppState, issue_id: Uuid) -> Result<Option<Issue>, AppError> {
    let mut conn = get_conn(&state.db).await?;
    issues::table
        .filter(issues::id.eq(issue_id))
        .select(Issue::as_select())
        .first(&mut conn)
        .await
        .optional()
        .map_err(db_err)
}

pub async fn get_issue_with_repo(
    state: &AppState,
    issue_id: Uuid,
) -> Result<Option<(Issue, Repo)>, AppError> {
    let issue = get_issue_by_id(state, issue_id).await?;
    let Some(issue) = issue else {
        return Ok(None);
    };
    let repo = get_repo_by_id(state, issue.repo_id).await?;
    Ok(repo.map(|repo| (issue, repo)))
}

pub async fn get_assignment_for_issue(
    state: &AppState,
    issue_id: Uuid,
) -> Result<Option<(Assignment, Option<Contributor>)>, AppError> {
    let mut conn = get_conn(&state.db).await?;
    let assignment = assignments::table
        .filter(assignments::issue_id.eq(issue_id))
        .select(Assignment::as_select())
        .first(&mut conn)
        .await
        .optional()
        .map_err(db_err)?;

    let Some(assignment) = assignment else {
        return Ok(None);
    };

    let contributor = if let Some(contributor_id) = assignment.contributor_id {
        contributors::table
            .filter(contributors::id.eq(contributor_id))
            .select(Contributor::as_select())
            .first(&mut conn)
            .await
            .optional()
            .map_err(db_err)?
    } else {
        None
    };

    Ok(Some((assignment, contributor)))
}

pub async fn get_issue_by_repo_and_github_id(
    state: &AppState,
    repo_id: Uuid,
    github_issue_id: i64,
) -> Result<Option<Issue>, AppError> {
    let mut conn = get_conn(&state.db).await?;
    issues::table
        .filter(issues::repo_id.eq(repo_id))
        .filter(issues::github_issue_id.eq(github_issue_id))
        .select(Issue::as_select())
        .first(&mut conn)
        .await
        .optional()
        .map_err(db_err)
}

pub async fn get_issue_by_repo_and_number(
    state: &AppState,
    repo_id: Uuid,
    github_issue_number: i32,
) -> Result<Option<Issue>, AppError> {
    let mut conn = get_conn(&state.db).await?;
    issues::table
        .filter(issues::repo_id.eq(repo_id))
        .filter(issues::github_issue_number.eq(github_issue_number))
        .select(Issue::as_select())
        .first(&mut conn)
        .await
        .optional()
        .map_err(db_err)
}

pub async fn update_issue_status(
    state: &AppState,
    issue_id: Uuid,
    status: &str,
    milestone_index: Option<i32>,
) -> Result<(), AppError> {
    let mut conn = get_conn(&state.db).await?;
    if let Some(milestone_index) = milestone_index {
        diesel::update(issues::table.filter(issues::id.eq(issue_id)))
            .set((
                issues::status.eq(status),
                issues::milestone_index.eq(milestone_index),
            ))
            .execute(&mut conn)
            .await
            .map_err(db_err)?;
    } else {
        diesel::update(issues::table.filter(issues::id.eq(issue_id)))
            .set(issues::status.eq(status))
            .execute(&mut conn)
            .await
            .map_err(db_err)?;
    }
    Ok(())
}

pub async fn fail_assignments_for_issues(
    state: &AppState,
    issue_ids: &[Uuid],
) -> Result<(), AppError> {
    if issue_ids.is_empty() {
        return Ok(());
    }

    let mut conn = get_conn(&state.db).await?;
    diesel::update(assignments::table.filter(assignments::issue_id.eq_any(issue_ids)))
        .set(assignments::payout_status.eq("failed"))
        .execute(&mut conn)
        .await
        .map_err(db_err)?;
    Ok(())
}

pub async fn update_assignment_payout_status(
    state: &AppState,
    assignment_id: Uuid,
    payout_status: &str,
) -> Result<(), AppError> {
    let mut conn = get_conn(&state.db).await?;
    diesel::update(assignments::table.filter(assignments::id.eq(assignment_id)))
        .set(assignments::payout_status.eq(payout_status))
        .execute(&mut conn)
        .await
        .map_err(db_err)?;
    Ok(())
}

pub async fn list_active_escrow_repos(state: &AppState) -> Result<Vec<Repo>, AppError> {
    let mut conn = get_conn(&state.db).await?;
    repos::table
        .filter(repos::escrow_contract_id.is_not_null())
        .select(Repo::as_select())
        .load(&mut conn)
        .await
        .map_err(db_err)
}

pub async fn list_issues_to_cancel(
    state: &AppState,
    repo_id: Uuid,
) -> Result<Vec<Issue>, AppError> {
    let mut conn = get_conn(&state.db).await?;
    issues::table
        .filter(issues::repo_id.eq(repo_id))
        .filter(issues::status.eq_any(vec!["pending", "active"]))
        .select(Issue::as_select())
        .load(&mut conn)
        .await
        .map_err(db_err)
}

fn is_unique_violation(error: &DieselError) -> bool {
    matches!(
        error,
        DieselError::DatabaseError(DatabaseErrorKind::UniqueViolation, _)
    )
}

pub async fn create_issue_and_reserve_balance(
    state: &AppState,
    repo: &Repo,
    github_issue_id: i64,
    github_issue_number: i32,
    title: &str,
    reward_amount: Decimal,
    difficulty_label: &str,
) -> Result<Option<Issue>, AppError> {
    let mut conn = get_conn(&state.db).await?;

    let result = conn
        .transaction::<Issue, DieselError, _>(|conn| {
            async move {
                // Lock the repo row and reserve the reward from the escrow balance.
                let current_balance: Option<Decimal> = repos::table
                    .filter(repos::id.eq(repo.id))
                    .select(repos::escrow_balance)
                    .for_update()
                    .first(conn)
                    .await
                    .optional()?;

                let Some(current_balance) = current_balance else {
                    // No such repo -> signal insufficient balance via rollback.
                    return Err(DieselError::RollbackTransaction);
                };
                if current_balance < reward_amount {
                    return Err(DieselError::RollbackTransaction);
                }

                let new_balance = (current_balance - reward_amount).round_dp(7);
                diesel::update(repos::table.filter(repos::id.eq(repo.id)))
                    .set(repos::escrow_balance.eq(new_balance))
                    .execute(conn)
                    .await?;

                let issue = diesel::insert_into(issues::table)
                    .values((
                        issues::repo_id.eq(repo.id),
                        issues::github_issue_id.eq(github_issue_id),
                        issues::github_issue_number.eq(github_issue_number),
                        issues::title.eq(title),
                        issues::reward_amount.eq(reward_amount),
                        issues::difficulty_label.eq(difficulty_label),
                        issues::status.eq("pending"),
                    ))
                    .returning(Issue::as_returning())
                    .get_result(conn)
                    .await?;

                Ok(issue)
            }
            .scope_boxed()
        })
        .await;

    match result {
        Ok(issue) => {
            invalidate_repo_cache(state, repo.id, Some(repo.github_repo_id)).await;
            Ok(Some(issue))
        }
        // Duplicate issue: transaction rolled back, escrow untouched.
        Err(ref error) if is_unique_violation(error) => Ok(None),
        Err(DieselError::RollbackTransaction) => {
            Err(AppError::bad_request("Insufficient escrow balance"))
        }
        Err(error) => Err(db_err(error)),
    }
}

pub async fn update_pending_issue_reward(
    state: &AppState,
    repo: &Repo,
    issue_id: Uuid,
    reward_amount: Decimal,
    difficulty_label: &str,
) -> Result<bool, AppError> {
    let mut conn = get_conn(&state.db).await?;

    let updated = conn
        .transaction::<bool, DieselError, _>(|conn| {
            async move {
                let current: Option<(Decimal, String)> = issues::table
                    .filter(issues::id.eq(issue_id))
                    .filter(issues::repo_id.eq(repo.id))
                    .select((issues::reward_amount, issues::status))
                    .for_update()
                    .first(conn)
                    .await
                    .optional()?;

                let Some((current_amount, status)) = current else {
                    return Ok(false);
                };
                if status != "pending" {
                    return Ok(false);
                }

                let difference = reward_amount - current_amount;

                let current_balance: Decimal = repos::table
                    .filter(repos::id.eq(repo.id))
                    .select(repos::escrow_balance)
                    .for_update()
                    .first(conn)
                    .await?;

                // Only enforce sufficiency when the reward increases.
                if difference > Decimal::ZERO && current_balance < difference {
                    return Ok(false);
                }

                let new_balance = (current_balance - difference).round_dp(7);
                diesel::update(repos::table.filter(repos::id.eq(repo.id)))
                    .set(repos::escrow_balance.eq(new_balance))
                    .execute(conn)
                    .await?;

                diesel::update(issues::table.filter(issues::id.eq(issue_id)))
                    .set((
                        issues::reward_amount.eq(reward_amount),
                        issues::difficulty_label.eq(difficulty_label),
                    ))
                    .execute(conn)
                    .await?;

                Ok(true)
            }
            .scope_boxed()
        })
        .await
        .map_err(db_err)?;

    if updated {
        invalidate_repo_cache(state, repo.id, Some(repo.github_repo_id)).await;
    }
    Ok(updated)
}

pub async fn cancel_issue(state: &AppState, issue_id: Uuid) -> Result<(), AppError> {
    let mut conn = get_conn(&state.db).await?;
    diesel::update(issues::table.filter(issues::id.eq(issue_id)))
        .set(issues::status.eq("cancelled"))
        .execute(&mut conn)
        .await
        .map_err(db_err)?;
    Ok(())
}

pub async fn complete_issue(state: &AppState, issue_id: Uuid) -> Result<(), AppError> {
    update_issue_status(state, issue_id, "completed", None).await
}

pub async fn reset_issue_to_pending(state: &AppState, issue_id: Uuid) -> Result<(), AppError> {
    let mut conn = get_conn(&state.db).await?;
    diesel::update(issues::table.filter(issues::id.eq(issue_id)))
        .set((
            issues::status.eq("pending"),
            issues::milestone_index.eq(None::<i32>),
        ))
        .execute(&mut conn)
        .await
        .map_err(db_err)?;
    Ok(())
}

pub async fn delete_assignments_for_issue(
    state: &AppState,
    issue_id: Uuid,
) -> Result<(), AppError> {
    let mut conn = get_conn(&state.db).await?;
    diesel::delete(assignments::table.filter(assignments::issue_id.eq(issue_id)))
        .execute(&mut conn)
        .await
        .map_err(db_err)?;
    Ok(())
}

pub async fn upsert_assignment(
    state: &AppState,
    issue_id: Uuid,
    contributor_id: Uuid,
) -> Result<(), AppError> {
    let mut conn = get_conn(&state.db).await?;
    diesel::insert_into(assignments::table)
        .values((
            assignments::issue_id.eq(issue_id),
            assignments::contributor_id.eq(contributor_id),
        ))
        .on_conflict(assignments::issue_id)
        .do_update()
        .set(assignments::contributor_id.eq(diesel::upsert::excluded(assignments::contributor_id)))
        .execute(&mut conn)
        .await
        .map_err(db_err)?;
    Ok(())
}

pub async fn ensure_contributor(
    state: &AppState,
    github_user_id: i64,
    github_username: &str,
) -> Result<Contributor, AppError> {
    if let Some(contributor) = get_contributor_by_github_id(state, github_user_id).await? {
        return Ok(contributor);
    }

    let mut conn = get_conn(&state.db).await?;
    let contributor = diesel::insert_into(contributors::table)
        .values((
            contributors::github_user_id.eq(github_user_id),
            contributors::github_username.eq(github_username),
        ))
        .returning(Contributor::as_returning())
        .get_result(&mut conn)
        .await
        .map_err(db_err)?;

    invalidate_contributor_cache(state, github_user_id).await;
    Ok(contributor)
}

pub async fn update_assignment_completion_percentage(
    state: &AppState,
    assignment_id: Uuid,
    percentage: Decimal,
) -> Result<(), AppError> {
    let mut conn = get_conn(&state.db).await?;
    diesel::update(assignments::table.filter(assignments::id.eq(assignment_id)))
        .set(assignments::completion_percentage.eq(percentage))
        .execute(&mut conn)
        .await
        .map_err(db_err)?;
    Ok(())
}

pub async fn update_assignment_pr_merge(
    state: &AppState,
    assignment_id: Uuid,
    pr_number: i32,
) -> Result<(), AppError> {
    let mut conn = get_conn(&state.db).await?;
    diesel::update(assignments::table.filter(assignments::id.eq(assignment_id)))
        .set((
            assignments::pr_number.eq(pr_number),
            assignments::pr_merged_at.eq(chrono::Utc::now()),
        ))
        .execute(&mut conn)
        .await
        .map_err(db_err)?;
    Ok(())
}

pub async fn refund_repo_balance(
    state: &AppState,
    repo: &Repo,
    amount: Decimal,
) -> Result<(), AppError> {
    let new_balance = repo.escrow_balance + amount;
    update_repo_escrow_balance(state, repo.id, new_balance, Some(repo.github_repo_id)).await
}

#[allow(clippy::too_many_arguments)]
pub async fn upsert_installation_repo(
    state: &AppState,
    github_repo_id: i64,
    full_name: &str,
    owner_github_id: i64,
    owner_username: &str,
    owner_type: &str,
    is_fork: bool,
    is_private: bool,
    installer_github_id: i64,
    github_installation_id: i64,
) -> Result<(), AppError> {
    let mut conn = get_conn(&state.db).await?;
    let repo = diesel::insert_into(repos::table)
        .values((
            repos::github_repo_id.eq(github_repo_id),
            repos::full_name.eq(full_name),
            repos::owner_github_id.eq(owner_github_id),
            repos::owner_username.eq(owner_username),
            repos::owner_type.eq(owner_type),
            repos::is_fork.eq(is_fork),
            repos::is_private.eq(is_private),
            repos::reward_low.eq(Decimal::from(1)),
            repos::reward_medium.eq(Decimal::from(2)),
            repos::reward_high.eq(Decimal::from(3)),
            repos::installer_github_id.eq(installer_github_id),
            repos::github_installation_id.eq(github_installation_id),
        ))
        .on_conflict(repos::github_repo_id)
        .do_update()
        .set((
            repos::full_name.eq(diesel::upsert::excluded(repos::full_name)),
            repos::owner_github_id.eq(diesel::upsert::excluded(repos::owner_github_id)),
            repos::owner_username.eq(diesel::upsert::excluded(repos::owner_username)),
            repos::owner_type.eq(diesel::upsert::excluded(repos::owner_type)),
            repos::is_fork.eq(diesel::upsert::excluded(repos::is_fork)),
            repos::is_private.eq(diesel::upsert::excluded(repos::is_private)),
            repos::installer_github_id.eq(diesel::upsert::excluded(repos::installer_github_id)),
            repos::github_installation_id
                .eq(diesel::upsert::excluded(repos::github_installation_id)),
        ))
        .returning(Repo::as_returning())
        .get_result(&mut conn)
        .await
        .map_err(db_err)?;
    invalidate_repo_cache(state, repo.id, Some(repo.github_repo_id)).await;
    Ok(())
}

pub async fn delete_repos_by_installation_id(
    state: &AppState,
    installation_id: i64,
) -> Result<(), AppError> {
    let mut conn = get_conn(&state.db).await?;
    let repos_list = repos::table
        .filter(repos::github_installation_id.eq(installation_id))
        .select(Repo::as_select())
        .load(&mut conn)
        .await
        .map_err(db_err)?;
    diesel::delete(repos::table.filter(repos::github_installation_id.eq(installation_id)))
        .execute(&mut conn)
        .await
        .map_err(db_err)?;
    for repo in repos_list {
        invalidate_repo_cache(state, repo.id, Some(repo.github_repo_id)).await;
    }
    Ok(())
}

//  Delete Repository from DB
pub async fn delete_repo_by_github_id(
    state: &AppState,
    github_repo_id: i64,
) -> Result<(), AppError> {
    let mut conn = get_conn(&state.db).await?;
    let repo = get_repo_by_github_id(state, github_repo_id).await?;
    diesel::delete(repos::table.filter(repos::github_repo_id.eq(github_repo_id)))
        .execute(&mut conn)
        .await
        .map_err(db_err)?;
    if let Some(repo) = repo {
        invalidate_repo_cache(state, repo.id, Some(repo.github_repo_id)).await;
    }
    Ok(())
}
