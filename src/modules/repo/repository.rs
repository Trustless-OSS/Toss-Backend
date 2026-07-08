use rust_decimal::Decimal;
use sqlx::PgPool;
use uuid::Uuid;

use crate::{
    error::{require_db, AppError},
    infra::{cache, cache_keys},
    shared::models::{Assignment, Contributor, Issue, Repo},
    state::AppState,
};

pub async fn get_repo_by_id(state: &AppState, repo_id: Uuid) -> Result<Option<Repo>, AppError> {
    let cache_key = cache_keys::repo(repo_id);
    if let Some(cached) = state.cache.get::<Repo>(&cache_key).await {
        return Ok(Some(cached));
    }

    let pool = require_db(&state.db)?;
    let repo = sqlx::query_as::<_, Repo>("SELECT * FROM repos WHERE id = $1")
        .bind(repo_id)
        .fetch_optional(pool)
        .await
        .map_err(|error| AppError::database(error.to_string()))?;

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

    let pool = require_db(&state.db)?;
    let repo = sqlx::query_as::<_, Repo>("SELECT * FROM repos WHERE github_repo_id = $1")
        .bind(github_repo_id)
        .fetch_optional(pool)
        .await
        .map_err(|error| AppError::database(error.to_string()))?;

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
    let pool = require_db(&state.db)?;

    if github_id == 0 {
        if let Some(username) = github_username.filter(|value| !value.is_empty()) {
            let total: i64 =
                sqlx::query_scalar("SELECT COUNT(*) FROM repos WHERE owner_username = $1")
                    .bind(username)
                    .fetch_one(pool)
                    .await
                    .map_err(|error| AppError::database(error.to_string()))?;

            let repos = sqlx::query_as::<_, Repo>(
                "SELECT * FROM repos WHERE owner_username = $1 ORDER BY created_at DESC LIMIT $2 OFFSET $3",
            )
            .bind(username)
            .bind(limit)
            .bind(offset)
            .fetch_all(pool)
            .await
            .map_err(|error| AppError::database(error.to_string()))?;

            return Ok((repos, total));
        }

        return Err(AppError::bad_request(
            "Could not determine GitHub identity from session",
        ));
    }

    let total: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM repos
         WHERE (owner_github_id = $1 OR installer_github_id = $1)
           AND COALESCE(is_fork, false) = false
           AND COALESCE(is_private, false) = false
           AND (owner_type = 'User' OR installer_github_id = $1)",
    )
    .bind(github_id)
    .fetch_one(pool)
    .await
    .map_err(|error| AppError::database(error.to_string()))?;

    let repos = sqlx::query_as::<_, Repo>(
        "SELECT * FROM repos
         WHERE (owner_github_id = $1 OR installer_github_id = $1)
           AND COALESCE(is_fork, false) = false
           AND COALESCE(is_private, false) = false
           AND (owner_type = 'User' OR installer_github_id = $1)
         ORDER BY created_at DESC
         LIMIT $2 OFFSET $3",
    )
    .bind(github_id)
    .bind(limit)
    .bind(offset)
    .fetch_all(pool)
    .await
    .map_err(|error| AppError::database(error.to_string()))?;

    Ok((repos, total))
}

pub async fn upsert_repo(
    pool: &PgPool,
    github_repo_id: i64,
    full_name: &str,
    owner_github_id: i64,
    owner_username: &str,
) -> Result<Repo, AppError> {
    sqlx::query_as::<_, Repo>(
        "INSERT INTO repos (github_repo_id, full_name, owner_github_id, owner_username, reward_low, reward_medium, reward_high)
         VALUES ($1, $2, $3, $4, 1, 2, 3)
         ON CONFLICT (github_repo_id) DO UPDATE SET
           full_name = EXCLUDED.full_name,
           owner_github_id = EXCLUDED.owner_github_id,
           owner_username = EXCLUDED.owner_username
         RETURNING *",
    )
    .bind(github_repo_id)
    .bind(full_name)
    .bind(owner_github_id)
    .bind(owner_username)
    .fetch_one(pool)
    .await
    .map_err(|error| AppError::database(error.to_string()))
}

pub async fn update_repo_escrow_contract(
    state: &AppState,
    repo_id: Uuid,
    contract_id: &str,
) -> Result<(), AppError> {
    let pool = require_db(&state.db)?;
    sqlx::query("UPDATE repos SET escrow_contract_id = $1 WHERE id = $2")
        .bind(contract_id)
        .bind(repo_id)
        .execute(pool)
        .await
        .map_err(|error| AppError::database(error.to_string()))?;
    invalidate_repo_cache(state, repo_id, None).await;
    Ok(())
}

pub async fn update_repo_escrow_balance(
    state: &AppState,
    repo_id: Uuid,
    balance: Decimal,
    github_repo_id: Option<i64>,
) -> Result<(), AppError> {
    let pool = require_db(&state.db)?;
    sqlx::query("UPDATE repos SET escrow_balance = $1 WHERE id = $2")
        .bind(balance)
        .bind(repo_id)
        .execute(pool)
        .await
        .map_err(|error| AppError::database(error.to_string()))?;
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
    let pool = require_db(&state.db)?;
    let repo = sqlx::query_as::<_, Repo>(
        "UPDATE repos SET reward_low = $1, reward_medium = $2, reward_high = $3 WHERE id = $4 RETURNING *",
    )
    .bind(reward_low)
    .bind(reward_medium)
    .bind(reward_high)
    .bind(repo_id)
    .fetch_one(pool)
    .await
    .map_err(|error| AppError::database(error.to_string()))?;

    invalidate_repo_cache(state, repo.id, Some(repo.github_repo_id)).await;
    Ok(repo)
}

pub async fn clear_repo_escrow(state: &AppState, repo_id: Uuid) -> Result<(), AppError> {
    let pool = require_db(&state.db)?;
    sqlx::query("UPDATE repos SET escrow_contract_id = NULL, escrow_balance = 0 WHERE id = $1")
        .bind(repo_id)
        .execute(pool)
        .await
        .map_err(|error| AppError::database(error.to_string()))?;
    invalidate_repo_cache(state, repo_id, None).await;
    Ok(())
}

pub async fn delete_repo_cascade(state: &AppState, repo_id: Uuid) -> Result<(), AppError> {
    let pool = require_db(&state.db)?;

    let issue_ids: Vec<Uuid> = sqlx::query_scalar("SELECT id FROM issues WHERE repo_id = $1")
        .bind(repo_id)
        .fetch_all(pool)
        .await
        .map_err(|error| AppError::database(error.to_string()))?;

    if !issue_ids.is_empty() {
        sqlx::query("DELETE FROM assignments WHERE issue_id = ANY($1)")
            .bind(&issue_ids)
            .execute(pool)
            .await
            .map_err(|error| AppError::database(error.to_string()))?;
    }

    sqlx::query("DELETE FROM issues WHERE repo_id = $1")
        .bind(repo_id)
        .execute(pool)
        .await
        .map_err(|error| AppError::database(error.to_string()))?;

    sqlx::query("DELETE FROM repos WHERE id = $1")
        .bind(repo_id)
        .execute(pool)
        .await
        .map_err(|error| AppError::database(error.to_string()))?;

    invalidate_repo_cache(state, repo_id, None).await;
    Ok(())
}

pub async fn count_repos_for_installation(
    state: &AppState,
    installation_id: i64,
    exclude_repo_id: Uuid,
) -> Result<i64, AppError> {
    let pool = require_db(&state.db)?;
    sqlx::query_scalar("SELECT COUNT(*) FROM repos WHERE github_installation_id = $1 AND id <> $2")
        .bind(installation_id)
        .bind(exclude_repo_id)
        .fetch_one(pool)
        .await
        .map_err(|error| AppError::database(error.to_string()))
}

pub async fn get_contributor_by_github_id(
    state: &AppState,
    github_user_id: i64,
) -> Result<Option<Contributor>, AppError> {
    let cache_key = cache_keys::contrib(github_user_id);
    if let Some(cached) = state.cache.get::<Contributor>(&cache_key).await {
        return Ok(Some(cached));
    }

    let pool = require_db(&state.db)?;
    let contributor =
        sqlx::query_as::<_, Contributor>("SELECT * FROM contributors WHERE github_user_id = $1")
            .bind(github_user_id)
            .fetch_optional(pool)
            .await
            .map_err(|error| AppError::database(error.to_string()))?;

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
    let pool = require_db(&state.db)?;
    let stellar_wallet = if payout_chain == "stellar" {
        Some(payout_address)
    } else {
        None
    };

    sqlx::query(
        "INSERT INTO contributors (github_user_id, github_username, stellar_wallet, payout_chain, payout_address)
         VALUES ($1, $2, $3, $4, $5)
         ON CONFLICT (github_user_id) DO UPDATE SET
           github_username = EXCLUDED.github_username,
           stellar_wallet = EXCLUDED.stellar_wallet,
           payout_chain = EXCLUDED.payout_chain,
           payout_address = EXCLUDED.payout_address",
    )
    .bind(github_user_id)
    .bind(github_username)
    .bind(stellar_wallet)
    .bind(payout_chain)
    .bind(payout_address)
    .execute(pool)
    .await
    .map_err(|error| AppError::database(error.to_string()))?;

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
    let pool = require_db(&state.db)?;
    let row: Option<(i64, Option<i64>)> =
        sqlx::query_as("SELECT owner_github_id, installer_github_id FROM repos WHERE id = $1")
            .bind(repo_id)
            .fetch_optional(pool)
            .await
            .map_err(|error| AppError::database(error.to_string()))?;

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
    let pool = require_db(&state.db)?;
    let assigned: Option<i64> = sqlx::query_scalar(
        "SELECT c.github_user_id
         FROM issues i
         JOIN assignments a ON a.issue_id = i.id
         JOIN contributors c ON c.id = a.contributor_id
         WHERE i.repo_id = $1 AND i.github_issue_id = $2",
    )
    .bind(repo_id)
    .bind(github_issue_id)
    .fetch_optional(pool)
    .await
    .map_err(|error| AppError::database(error.to_string()))?;

    Ok(assigned == Some(github_user_id))
}

pub async fn list_issues_for_repo(
    state: &AppState,
    repo_id: Uuid,
    limit: i64,
    offset: i64,
) -> Result<(Vec<serde_json::Value>, i64), AppError> {
    let pool = require_db(&state.db)?;

    let total: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM issues WHERE repo_id = $1")
        .bind(repo_id)
        .fetch_one(pool)
        .await
        .map_err(|error| AppError::database(error.to_string()))?;

    let issues = sqlx::query_as::<_, Issue>(
        "SELECT * FROM issues WHERE repo_id = $1 ORDER BY created_at DESC LIMIT $2 OFFSET $3",
    )
    .bind(repo_id)
    .bind(limit)
    .bind(offset)
    .fetch_all(pool)
    .await
    .map_err(|error| AppError::database(error.to_string()))?;

    let mut rows = Vec::with_capacity(issues.len());
    for issue in issues {
        let assignments =
            sqlx::query_as::<_, Assignment>("SELECT * FROM assignments WHERE issue_id = $1")
                .bind(issue.id)
                .fetch_all(pool)
                .await
                .map_err(|error| AppError::database(error.to_string()))?;

        let mut assignment_rows = Vec::new();
        for assignment in assignments {
            let contributor = if let Some(contributor_id) = assignment.contributor_id {
                sqlx::query_as::<_, Contributor>("SELECT * FROM contributors WHERE id = $1")
                    .bind(contributor_id)
                    .fetch_optional(pool)
                    .await
                    .map_err(|error| AppError::database(error.to_string()))?
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
    let pool = require_db(&state.db)?;
    sqlx::query_as::<_, Issue>("SELECT * FROM issues WHERE id = $1")
        .bind(issue_id)
        .fetch_optional(pool)
        .await
        .map_err(|error| AppError::database(error.to_string()))
}

pub async fn get_issue_with_repo(
    state: &AppState,
    issue_id: Uuid,
) -> Result<Option<(Issue, Repo)>, AppError> {
    let pool = require_db(&state.db)?;
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
    let pool = require_db(&state.db)?;
    let assignment =
        sqlx::query_as::<_, Assignment>("SELECT * FROM assignments WHERE issue_id = $1")
            .bind(issue_id)
            .fetch_optional(pool)
            .await
            .map_err(|error| AppError::database(error.to_string()))?;

    let Some(assignment) = assignment else {
        return Ok(None);
    };

    let contributor = if let Some(contributor_id) = assignment.contributor_id {
        sqlx::query_as::<_, Contributor>("SELECT * FROM contributors WHERE id = $1")
            .bind(contributor_id)
            .fetch_optional(pool)
            .await
            .map_err(|error| AppError::database(error.to_string()))?
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
    let pool = require_db(&state.db)?;
    sqlx::query_as::<_, Issue>("SELECT * FROM issues WHERE repo_id = $1 AND github_issue_id = $2")
        .bind(repo_id)
        .bind(github_issue_id)
        .fetch_optional(pool)
        .await
        .map_err(|error| AppError::database(error.to_string()))
}

pub async fn get_issue_by_repo_and_number(
    state: &AppState,
    repo_id: Uuid,
    github_issue_number: i32,
) -> Result<Option<Issue>, AppError> {
    let pool = require_db(&state.db)?;
    sqlx::query_as::<_, Issue>(
        "SELECT * FROM issues WHERE repo_id = $1 AND github_issue_number = $2",
    )
    .bind(repo_id)
    .bind(github_issue_number)
    .fetch_optional(pool)
    .await
    .map_err(|error| AppError::database(error.to_string()))
}

pub async fn update_issue_status(
    state: &AppState,
    issue_id: Uuid,
    status: &str,
    milestone_index: Option<i32>,
) -> Result<(), AppError> {
    let pool = require_db(&state.db)?;
    if let Some(milestone_index) = milestone_index {
        sqlx::query("UPDATE issues SET status = $1, milestone_index = $2 WHERE id = $3")
            .bind(status)
            .bind(milestone_index)
            .bind(issue_id)
            .execute(pool)
            .await
            .map_err(|error| AppError::database(error.to_string()))?;
    } else {
        sqlx::query("UPDATE issues SET status = $1 WHERE id = $2")
            .bind(status)
            .bind(issue_id)
            .execute(pool)
            .await
            .map_err(|error| AppError::database(error.to_string()))?;
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

    let pool = require_db(&state.db)?;
    sqlx::query("UPDATE assignments SET payout_status = 'failed' WHERE issue_id = ANY($1)")
        .bind(issue_ids)
        .execute(pool)
        .await
        .map_err(|error| AppError::database(error.to_string()))?;
    Ok(())
}

pub async fn update_assignment_payout_status(
    state: &AppState,
    assignment_id: Uuid,
    payout_status: &str,
) -> Result<(), AppError> {
    let pool = require_db(&state.db)?;
    sqlx::query("UPDATE assignments SET payout_status = $1 WHERE id = $2")
        .bind(payout_status)
        .bind(assignment_id)
        .execute(pool)
        .await
        .map_err(|error| AppError::database(error.to_string()))?;
    Ok(())
}

pub async fn list_active_escrow_repos(state: &AppState) -> Result<Vec<Repo>, AppError> {
    let pool = require_db(&state.db)?;
    sqlx::query_as::<_, Repo>("SELECT * FROM repos WHERE escrow_contract_id IS NOT NULL")
        .fetch_all(pool)
        .await
        .map_err(|error| AppError::database(error.to_string()))
}

pub async fn list_issues_to_cancel(
    state: &AppState,
    repo_id: Uuid,
) -> Result<Vec<Issue>, AppError> {
    let pool = require_db(&state.db)?;
    sqlx::query_as::<_, Issue>(
        "SELECT * FROM issues WHERE repo_id = $1 AND status IN ('pending', 'active')",
    )
    .bind(repo_id)
    .fetch_all(pool)
    .await
    .map_err(|error| AppError::database(error.to_string()))
}

fn is_unique_violation(error: &sqlx::Error) -> bool {
    error
        .as_database_error()
        .and_then(|db| db.code())
        .is_some_and(|code| code == "23505")
}

pub async fn try_insert_issue(
    state: &AppState,
    repo_id: Uuid,
    github_issue_id: i64,
    github_issue_number: i32,
    title: &str,
    reward_amount: Decimal,
    difficulty_label: &str,
) -> Result<Option<Issue>, AppError> {
    let pool = require_db(&state.db)?;
    match sqlx::query_as::<_, Issue>(
        "INSERT INTO issues (repo_id, github_issue_id, github_issue_number, title, reward_amount, difficulty_label, status)
         VALUES ($1, $2, $3, $4, $5, $6, 'pending')
         RETURNING *",
    )
    .bind(repo_id)
    .bind(github_issue_id)
    .bind(github_issue_number)
    .bind(title)
    .bind(reward_amount)
    .bind(difficulty_label)
    .fetch_one(pool)
    .await
    {
        Ok(issue) => Ok(Some(issue)),
        Err(error) if is_unique_violation(&error) => Ok(None),
        Err(error) => Err(AppError::database(error.to_string())),
    }
}

pub async fn update_issue_reward(
    state: &AppState,
    issue_id: Uuid,
    reward_amount: Decimal,
    difficulty_label: &str,
) -> Result<(), AppError> {
    let pool = require_db(&state.db)?;
    sqlx::query("UPDATE issues SET reward_amount = $1, difficulty_label = $2 WHERE id = $3")
        .bind(reward_amount)
        .bind(difficulty_label)
        .bind(issue_id)
        .execute(pool)
        .await
        .map_err(|error| AppError::database(error.to_string()))?;
    Ok(())
}

pub async fn cancel_issue(state: &AppState, issue_id: Uuid) -> Result<(), AppError> {
    let pool = require_db(&state.db)?;
    sqlx::query("UPDATE issues SET status = 'cancelled' WHERE id = $1")
        .bind(issue_id)
        .execute(pool)
        .await
        .map_err(|error| AppError::database(error.to_string()))?;
    Ok(())
}

pub async fn complete_issue(state: &AppState, issue_id: Uuid) -> Result<(), AppError> {
    update_issue_status(state, issue_id, "completed", None).await
}

pub async fn reset_issue_to_pending(state: &AppState, issue_id: Uuid) -> Result<(), AppError> {
    let pool = require_db(&state.db)?;
    sqlx::query("UPDATE issues SET status = 'pending', milestone_index = NULL WHERE id = $1")
        .bind(issue_id)
        .execute(pool)
        .await
        .map_err(|error| AppError::database(error.to_string()))?;
    Ok(())
}

pub async fn delete_assignments_for_issue(
    state: &AppState,
    issue_id: Uuid,
) -> Result<(), AppError> {
    let pool = require_db(&state.db)?;
    sqlx::query("DELETE FROM assignments WHERE issue_id = $1")
        .bind(issue_id)
        .execute(pool)
        .await
        .map_err(|error| AppError::database(error.to_string()))?;
    Ok(())
}

pub async fn upsert_assignment(
    state: &AppState,
    issue_id: Uuid,
    contributor_id: Uuid,
) -> Result<(), AppError> {
    let pool = require_db(&state.db)?;
    sqlx::query(
        "INSERT INTO assignments (issue_id, contributor_id)
         VALUES ($1, $2)
         ON CONFLICT (issue_id) DO UPDATE SET contributor_id = EXCLUDED.contributor_id",
    )
    .bind(issue_id)
    .bind(contributor_id)
    .execute(pool)
    .await
    .map_err(|error| AppError::database(error.to_string()))?;
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

    let pool = require_db(&state.db)?;
    let contributor = sqlx::query_as::<_, Contributor>(
        "INSERT INTO contributors (github_user_id, github_username)
         VALUES ($1, $2)
         RETURNING *",
    )
    .bind(github_user_id)
    .bind(github_username)
    .fetch_one(pool)
    .await
    .map_err(|error| AppError::database(error.to_string()))?;

    invalidate_contributor_cache(state, github_user_id).await;
    Ok(contributor)
}

pub async fn update_assignment_completion_percentage(
    state: &AppState,
    assignment_id: Uuid,
    percentage: Decimal,
) -> Result<(), AppError> {
    let pool = require_db(&state.db)?;
    sqlx::query("UPDATE assignments SET completion_percentage = $1 WHERE id = $2")
        .bind(percentage)
        .bind(assignment_id)
        .execute(pool)
        .await
        .map_err(|error| AppError::database(error.to_string()))?;
    Ok(())
}

pub async fn update_assignment_pr_merge(
    state: &AppState,
    assignment_id: Uuid,
    pr_number: i32,
) -> Result<(), AppError> {
    let pool = require_db(&state.db)?;
    sqlx::query("UPDATE assignments SET pr_number = $1, pr_merged_at = NOW() WHERE id = $2")
        .bind(pr_number)
        .bind(assignment_id)
        .execute(pool)
        .await
        .map_err(|error| AppError::database(error.to_string()))?;
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

pub async fn reserve_repo_balance(
    state: &AppState,
    repo: &Repo,
    amount: Decimal,
) -> Result<Decimal, AppError> {
    let new_balance = (repo.escrow_balance - amount).round_dp(7);
    update_repo_escrow_balance(state, repo.id, new_balance, Some(repo.github_repo_id)).await?;
    Ok(new_balance)
}

pub async fn upsert_installation_repo(
    pool: &PgPool,
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
    sqlx::query(
        "INSERT INTO repos (
            github_repo_id, full_name, owner_github_id, owner_username, owner_type,
            is_fork, is_private, reward_low, reward_medium, reward_high,
            installer_github_id, github_installation_id
         ) VALUES ($1, $2, $3, $4, $5, $6, $7, 1, 2, 3, $8, $9)
         ON CONFLICT (github_repo_id) DO UPDATE SET
           full_name = EXCLUDED.full_name,
           owner_github_id = EXCLUDED.owner_github_id,
           owner_username = EXCLUDED.owner_username,
           owner_type = EXCLUDED.owner_type,
           is_fork = EXCLUDED.is_fork,
           is_private = EXCLUDED.is_private,
           installer_github_id = EXCLUDED.installer_github_id,
           github_installation_id = EXCLUDED.github_installation_id",
    )
    .bind(github_repo_id)
    .bind(full_name)
    .bind(owner_github_id)
    .bind(owner_username)
    .bind(owner_type)
    .bind(is_fork)
    .bind(is_private)
    .bind(installer_github_id)
    .bind(github_installation_id)
    .execute(pool)
    .await
    .map_err(|error| AppError::database(error.to_string()))?;
    Ok(())
}

pub async fn delete_repos_by_installation_id(
    state: &AppState,
    installation_id: i64,
) -> Result<(), AppError> {
    let pool = require_db(&state.db)?;
    sqlx::query("DELETE FROM repos WHERE github_installation_id = $1")
        .bind(installation_id)
        .execute(pool)
        .await
        .map_err(|error| AppError::database(error.to_string()))?;
    Ok(())
}

pub async fn delete_repo_by_github_id(
    state: &AppState,
    github_repo_id: i64,
) -> Result<(), AppError> {
    let pool = require_db(&state.db)?;
    sqlx::query("DELETE FROM repos WHERE github_repo_id = $1")
        .bind(github_repo_id)
        .execute(pool)
        .await
        .map_err(|error| AppError::database(error.to_string()))?;
    Ok(())
}
