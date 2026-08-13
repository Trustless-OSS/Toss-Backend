use rust_decimal::Decimal;
use uuid::Uuid;

use crate::{
    error::{is_unique_violation, map_db_err, require_db, AppError},
    infra::cache_keys,
    shared::models::{
        schema,
        Assignment, Contributor, Issue, Repo,
    },
    state::AppState,
};

fn round_balance(value: Decimal) -> Decimal {
    value.round_dp(7)
}

pub async fn get_repo_by_id(state: &AppState, repo_id: Uuid) -> Result<Option<Repo>, AppError> {
    let cache_key = cache_keys::repo(repo_id);
    if let Some(cached) = state.cache.get::<Repo>(&cache_key).await {
        return Ok(Some(cached));
    }

    let mut db = require_db(&state.db)?;
    let repo = schema::Repo::filter_by_id(&repo_id)
        .first()
        .exec(&mut db)
        .await
        .map_err(map_db_err)?
        .map(Repo::from);

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

    let mut db = require_db(&state.db)?;
    let repo = schema::Repo::filter_by_github_repo_id(github_repo_id)
        .first()
        .exec(&mut db)
        .await
        .map_err(map_db_err)?
        .map(Repo::from);

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
    let mut db = require_db(&state.db)?;
    let limit = limit.max(0) as usize;
    let offset = offset.max(0) as usize;

    if github_id == 0 {
        if let Some(username) = github_username.filter(|value| !value.is_empty()) {
            let all = schema::Repo::filter_by_owner_username(username)
                .exec(&mut db)
                .await
                .map_err(map_db_err)?;
            let total = all.len() as i64;
            let repos = schema::Repo::filter_by_owner_username(username)
                .order_by(schema::Repo::fields().created_at().desc())
                .limit(limit)
                .offset(offset)
                .exec(&mut db)
                .await
                .map_err(map_db_err)?
                .into_iter()
                .map(Repo::from)
                .collect();
            return Ok((repos, total));
        }

        return Err(AppError::bad_request(
            "Could not determine GitHub identity from session",
        ));
    }

    let owned_or_installed = schema::Repo::fields()
        .owner_github_id()
        .eq(github_id)
        .or(schema::Repo::fields()
            .installer_github_id()
            .eq(Some(github_id)));
    let public_not_fork = schema::Repo::fields()
        .is_fork()
        .eq(false)
        .and(schema::Repo::fields().is_private().eq(false));
    let user_or_installer = schema::Repo::fields()
        .owner_type()
        .eq(Some("User".to_string()))
        .or(schema::Repo::fields()
            .installer_github_id()
            .eq(Some(github_id)));

    let all = schema::Repo::filter(
        owned_or_installed
            .clone()
            .and(public_not_fork.clone())
            .and(user_or_installer.clone()),
    )
    .exec(&mut db)
    .await
    .map_err(map_db_err)?;
    let total = all.len() as i64;

    let repos = schema::Repo::filter(owned_or_installed.and(public_not_fork).and(user_or_installer))
        .order_by(schema::Repo::fields().created_at().desc())
        .limit(limit)
        .offset(offset)
        .exec(&mut db)
        .await
        .map_err(map_db_err)?
        .into_iter()
        .map(Repo::from)
        .collect();

    Ok((repos, total))
}

pub async fn upsert_repo(
    state: &AppState,
    github_repo_id: i64,
    full_name: &str,
    owner_github_id: i64,
    owner_username: &str,
) -> Result<Repo, AppError> {
    let mut db = require_db(&state.db)?;
    let repo = schema::Repo::upsert_by_github_repo_id(github_repo_id)
        .full_name(full_name)
        .owner_github_id(owner_github_id)
        .owner_username(owner_username)
        .on_create(|repo| {
            repo.reward_low(Decimal::from(1))
                .reward_medium(Decimal::from(2))
                .reward_high(Decimal::from(3))
        })
        .exec(&mut db)
        .await
        .map_err(map_db_err)?;
    Ok(Repo::from(repo))
}

pub async fn update_repo_escrow_contract(
    state: &AppState,
    repo_id: Uuid,
    contract_id: &str,
) -> Result<(), AppError> {
    let mut db = require_db(&state.db)?;
    toasty::update!(schema::Repo::filter_by_id(&repo_id) {
        escrow_contract_id: Some(contract_id.to_string()),
    })
    .exec(&mut db)
    .await
    .map_err(map_db_err)?;
    invalidate_repo_cache(state, repo_id, None).await;
    Ok(())
}

pub async fn update_repo_escrow_funder_wallet(
    state: &AppState,
    repo_id: Uuid,
    funder_wallet: &str,
) -> Result<(), AppError> {
    let mut db = require_db(&state.db)?;
    toasty::update!(schema::Repo::filter_by_id(&repo_id) {
        escrow_funder_wallet: Some(funder_wallet.to_string()),
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
    toasty::update!(schema::Repo::filter_by_id(&repo_id) {
        escrow_balance: round_balance(balance),
    })
    .exec(&mut db)
    .await
    .map_err(map_db_err)?;
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
    let mut db = require_db(&state.db)?;
    let mut repo = schema::Repo::get_by_id(&mut db, &repo_id)
        .await
        .map_err(map_db_err)?;
    toasty::update!(repo {
        reward_low,
        reward_medium,
        reward_high,
    })
    .exec(&mut db)
    .await
    .map_err(map_db_err)?;

    let repo = Repo::from(repo);
    invalidate_repo_cache(state, repo.id, Some(repo.github_repo_id)).await;
    Ok(repo)
}

pub async fn clear_repo_escrow(state: &AppState, repo_id: Uuid) -> Result<(), AppError> {
    let mut db = require_db(&state.db)?;
    toasty::update!(schema::Repo::filter_by_id(&repo_id) {
        escrow_contract_id: Option::<String>::None,
        escrow_funder_wallet: Option::<String>::None,
        escrow_balance: Decimal::ZERO,
    })
    .exec(&mut db)
    .await
    .map_err(map_db_err)?;
    invalidate_repo_cache(state, repo_id, None).await;
    Ok(())
}

pub async fn delete_repo_cascade(state: &AppState, repo_id: Uuid) -> Result<(), AppError> {
    let mut db = require_db(&state.db)?;
    let mut tx = db.transaction().await.map_err(map_db_err)?;

    let issues = schema::Issue::filter_by_repo_id(&repo_id)
        .exec(&mut tx)
        .await
        .map_err(map_db_err)?;

    for issue in issues {
        schema::Assignment::filter_by_issue_id(&issue.id)
            .delete()
            .exec(&mut tx)
            .await
            .map_err(map_db_err)?;
    }

    schema::Issue::filter_by_repo_id(&repo_id)
        .delete()
        .exec(&mut tx)
        .await
        .map_err(map_db_err)?;

    schema::Repo::filter_by_id(&repo_id)
        .delete()
        .exec(&mut tx)
        .await
        .map_err(map_db_err)?;

    tx.commit().await.map_err(map_db_err)?;
    invalidate_repo_cache(state, repo_id, None).await;
    Ok(())
}

pub async fn count_repos_for_installation(
    state: &AppState,
    installation_id: i64,
    exclude_repo_id: Uuid,
) -> Result<i64, AppError> {
    let mut db = require_db(&state.db)?;
    let repos = schema::Repo::filter_by_github_installation_id(Some(installation_id))
        .exec(&mut db)
        .await
        .map_err(map_db_err)?;
    Ok(repos.into_iter().filter(|repo| repo.id != exclude_repo_id).count() as i64)
}

pub async fn get_contributor_by_github_id(
    state: &AppState,
    github_user_id: i64,
) -> Result<Option<Contributor>, AppError> {
    let cache_key = cache_keys::contrib(github_user_id);
    if let Some(cached) = state.cache.get::<Contributor>(&cache_key).await {
        return Ok(Some(cached));
    }

    let mut db = require_db(&state.db)?;
    let contributor = schema::Contributor::filter_by_github_user_id(github_user_id)
        .first()
        .exec(&mut db)
        .await
        .map_err(map_db_err)?
        .map(Contributor::from);

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
    let mut db = require_db(&state.db)?;
    let stellar_wallet = if payout_chain == "stellar" {
        Some(payout_address.to_string())
    } else {
        None
    };

    schema::Contributor::upsert_by_github_user_id(github_user_id)
        .github_username(github_username)
        .stellar_wallet(stellar_wallet)
        .payout_chain(payout_chain)
        .payout_address(Some(payout_address.to_string()))
        .exec(&mut db)
        .await
        .map_err(map_db_err)?;

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
    let mut db = require_db(&state.db)?;
    let repo = schema::Repo::filter_by_id(&repo_id)
        .first()
        .exec(&mut db)
        .await
        .map_err(map_db_err)?;

    Ok(repo.is_some_and(|repo| {
        repo.owner_github_id == github_user_id || repo.installer_github_id == Some(github_user_id)
    }))
}

pub async fn is_assigned_contributor(
    state: &AppState,
    github_user_id: i64,
    repo_id: Uuid,
    github_issue_id: i64,
) -> Result<bool, AppError> {
    let mut db = require_db(&state.db)?;
    let issue = schema::Issue::filter_by_repo_id_and_github_issue_id(&repo_id, github_issue_id)
        .first()
        .exec(&mut db)
        .await
        .map_err(map_db_err)?;

    let Some(issue) = issue else {
        return Ok(false);
    };

    let assignment = schema::Assignment::filter_by_issue_id(&issue.id)
        .first()
        .exec(&mut db)
        .await
        .map_err(map_db_err)?;

    let Some(assignment) = assignment else {
        return Ok(false);
    };

    let Some(contributor_id) = assignment.contributor_id else {
        return Ok(false);
    };

    let contributor = schema::Contributor::filter_by_id(&contributor_id)
        .first()
        .exec(&mut db)
        .await
        .map_err(map_db_err)?;

    Ok(contributor.is_some_and(|c| c.github_user_id == github_user_id))
}

pub async fn list_issues_for_repo(
    state: &AppState,
    repo_id: Uuid,
    limit: i64,
    offset: i64,
) -> Result<(Vec<serde_json::Value>, i64), AppError> {
    let mut db = require_db(&state.db)?;
    let limit = limit.max(0) as usize;
    let offset = offset.max(0) as usize;

    let all = schema::Issue::filter_by_repo_id(&repo_id)
        .exec(&mut db)
        .await
        .map_err(map_db_err)?;
    let total = all.len() as i64;

    let issues = schema::Issue::filter_by_repo_id(&repo_id)
        .order_by(schema::Issue::fields().created_at().desc())
        .limit(limit)
        .offset(offset)
        .exec(&mut db)
        .await
        .map_err(map_db_err)?;

    let mut rows = Vec::with_capacity(issues.len());
    for issue in issues {
        let issue_dto = Issue::from(issue);
        let assignments = schema::Assignment::filter_by_issue_id(&issue_dto.id)
            .exec(&mut db)
            .await
            .map_err(map_db_err)?;

        let mut assignment_rows = Vec::new();
        for assignment in assignments {
            let assignment = Assignment::from(assignment);
            let contributor = if let Some(contributor_id) = assignment.contributor_id {
                schema::Contributor::filter_by_id(&contributor_id)
                    .first()
                    .exec(&mut db)
                    .await
                    .map_err(map_db_err)?
                    .map(Contributor::from)
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
            "id": issue_dto.id,
            "repo_id": issue_dto.repo_id,
            "github_issue_id": issue_dto.github_issue_id,
            "github_issue_number": issue_dto.github_issue_number,
            "title": issue_dto.title,
            "reward_amount": issue_dto.reward_amount,
            "difficulty_label": issue_dto.difficulty_label,
            "milestone_index": issue_dto.milestone_index,
            "status": issue_dto.status,
            "created_at": issue_dto.created_at,
            "assignments": assignment_rows,
        }));
    }

    Ok((rows, total))
}

pub async fn get_issue_by_id(state: &AppState, issue_id: Uuid) -> Result<Option<Issue>, AppError> {
    let mut db = require_db(&state.db)?;
    Ok(schema::Issue::filter_by_id(&issue_id)
        .first()
        .exec(&mut db)
        .await
        .map_err(map_db_err)?
        .map(Issue::from))
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
    let mut db = require_db(&state.db)?;
    let assignment = schema::Assignment::filter_by_issue_id(&issue_id)
        .first()
        .exec(&mut db)
        .await
        .map_err(map_db_err)?;

    let Some(assignment) = assignment else {
        return Ok(None);
    };
    let assignment = Assignment::from(assignment);

    let contributor = if let Some(contributor_id) = assignment.contributor_id {
        schema::Contributor::filter_by_id(&contributor_id)
            .first()
            .exec(&mut db)
            .await
            .map_err(map_db_err)?
            .map(Contributor::from)
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
    let mut db = require_db(&state.db)?;
    Ok(
        schema::Issue::filter_by_repo_id_and_github_issue_id(&repo_id, github_issue_id)
            .first()
            .exec(&mut db)
            .await
            .map_err(map_db_err)?
            .map(Issue::from),
    )
}

pub async fn get_issue_by_repo_and_number(
    state: &AppState,
    repo_id: Uuid,
    github_issue_number: i32,
) -> Result<Option<Issue>, AppError> {
    let mut db = require_db(&state.db)?;
    Ok(schema::Issue::filter(
        schema::Issue::fields()
            .repo_id()
            .eq(repo_id)
            .and(schema::Issue::fields().github_issue_number().eq(github_issue_number)),
    )
    .first()
    .exec(&mut db)
    .await
    .map_err(map_db_err)?
    .map(Issue::from))
}

pub async fn update_issue_status(
    state: &AppState,
    issue_id: Uuid,
    status: &str,
    milestone_index: Option<i32>,
) -> Result<(), AppError> {
    let mut db = require_db(&state.db)?;
    if let Some(milestone_index) = milestone_index {
        toasty::update!(schema::Issue::filter_by_id(&issue_id) {
            status: status.to_string(),
            milestone_index: Some(milestone_index),
        })
        .exec(&mut db)
        .await
        .map_err(map_db_err)?;
    } else {
        toasty::update!(schema::Issue::filter_by_id(&issue_id) {
            status: status.to_string(),
        })
        .exec(&mut db)
        .await
        .map_err(map_db_err)?;
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

    let mut db = require_db(&state.db)?;
    for issue_id in issue_ids {
        toasty::update!(schema::Assignment::filter_by_issue_id(issue_id) {
            payout_status: "failed".to_string(),
        })
        .exec(&mut db)
        .await
        .map_err(map_db_err)?;
    }
    Ok(())
}

pub async fn update_assignment_payout_status(
    state: &AppState,
    assignment_id: Uuid,
    payout_status: &str,
) -> Result<(), AppError> {
    let mut db = require_db(&state.db)?;
    toasty::update!(schema::Assignment::filter_by_id(&assignment_id) {
        payout_status: payout_status.to_string(),
    })
    .exec(&mut db)
    .await
    .map_err(map_db_err)?;
    Ok(())
}

pub async fn list_active_escrow_repos(state: &AppState) -> Result<Vec<Repo>, AppError> {
    let mut db = require_db(&state.db)?;
    Ok(schema::Repo::filter(schema::Repo::fields().escrow_contract_id().is_some())
        .exec(&mut db)
        .await
        .map_err(map_db_err)?
        .into_iter()
        .map(Repo::from)
        .collect())
}

pub async fn list_issues_to_cancel(
    state: &AppState,
    repo_id: Uuid,
) -> Result<Vec<Issue>, AppError> {
    let mut db = require_db(&state.db)?;
    Ok(schema::Issue::filter(
        schema::Issue::fields()
            .repo_id()
            .eq(repo_id)
            .and(
                schema::Issue::fields()
                    .status()
                    .eq("pending".to_string())
                    .or(schema::Issue::fields().status().eq("active".to_string())),
            ),
    )
    .exec(&mut db)
    .await
    .map_err(map_db_err)?
    .into_iter()
    .map(Issue::from)
    .collect())
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
    let mut db = require_db(&state.db)?;
    let mut tx = db.transaction().await.map_err(map_db_err)?;

    let mut schema_repo = schema::Repo::get_by_id(&mut tx, &repo.id)
        .await
        .map_err(map_db_err)?;

    if schema_repo.escrow_balance < reward_amount {
        return Err(AppError::bad_request("Insufficient escrow balance"));
    }

    let new_balance = round_balance(schema_repo.escrow_balance - reward_amount);
    toasty::update!(schema_repo { escrow_balance: new_balance })
        .exec(&mut tx)
        .await
        .map_err(map_db_err)?;

    let issue = toasty::create!(schema::Issue {
        repo_id: repo.id,
        github_issue_id,
        github_issue_number,
        title: title.to_string(),
        reward_amount,
        difficulty_label: Some(difficulty_label.to_string()),
        status: "pending".to_string(),
    })
    .exec(&mut tx)
    .await;

    let issue = match issue {
        Ok(issue) => issue,
        Err(error) if is_unique_violation(&error) => {
            return Ok(None);
        }
        Err(error) => return Err(map_db_err(error)),
    };

    tx.commit().await.map_err(map_db_err)?;
    invalidate_repo_cache(state, repo.id, Some(repo.github_repo_id)).await;
    Ok(Some(Issue::from(issue)))
}

pub async fn update_pending_issue_reward(
    state: &AppState,
    repo: &Repo,
    issue_id: Uuid,
    reward_amount: Decimal,
    difficulty_label: &str,
) -> Result<bool, AppError> {
    let mut db = require_db(&state.db)?;
    let mut tx = db.transaction().await.map_err(map_db_err)?;

    let mut issue = match schema::Issue::filter_by_id(&issue_id)
        .first()
        .exec(&mut tx)
        .await
        .map_err(map_db_err)?
    {
        Some(issue) if issue.repo_id == repo.id => issue,
        _ => return Ok(false),
    };

    if issue.status != "pending" {
        return Ok(false);
    }

    let difference = reward_amount - issue.reward_amount;
    let mut schema_repo = schema::Repo::get_by_id(&mut tx, &repo.id)
        .await
        .map_err(map_db_err)?;

    if difference > Decimal::ZERO && schema_repo.escrow_balance < difference {
        return Ok(false);
    }

    let new_balance = round_balance(schema_repo.escrow_balance - difference);
    toasty::update!(schema_repo { escrow_balance: new_balance })
        .exec(&mut tx)
        .await
        .map_err(map_db_err)?;

    toasty::update!(issue {
        reward_amount,
        difficulty_label: Some(difficulty_label.to_string()),
    })
    .exec(&mut tx)
    .await
    .map_err(map_db_err)?;

    tx.commit().await.map_err(map_db_err)?;
    invalidate_repo_cache(state, repo.id, Some(repo.github_repo_id)).await;
    Ok(true)
}

pub async fn cancel_issue(state: &AppState, issue_id: Uuid) -> Result<(), AppError> {
    update_issue_status(state, issue_id, "cancelled", None).await
}

pub async fn complete_issue(state: &AppState, issue_id: Uuid) -> Result<(), AppError> {
    update_issue_status(state, issue_id, "completed", None).await
}

pub async fn reset_issue_to_pending(state: &AppState, issue_id: Uuid) -> Result<(), AppError> {
    let mut db = require_db(&state.db)?;
    toasty::update!(schema::Issue::filter_by_id(&issue_id) {
        status: "pending".to_string(),
        milestone_index: Option::<i32>::None,
    })
    .exec(&mut db)
    .await
    .map_err(map_db_err)?;
    Ok(())
}

pub async fn delete_assignments_for_issue(
    state: &AppState,
    issue_id: Uuid,
) -> Result<(), AppError> {
    let mut db = require_db(&state.db)?;
    schema::Assignment::filter_by_issue_id(&issue_id)
        .delete()
        .exec(&mut db)
        .await
        .map_err(map_db_err)?;
    Ok(())
}

pub async fn upsert_assignment(
    state: &AppState,
    issue_id: Uuid,
    contributor_id: Uuid,
) -> Result<(), AppError> {
    let mut db = require_db(&state.db)?;
    schema::Assignment::upsert_by_issue_id(issue_id)
        .contributor_id(Some(contributor_id))
        .exec(&mut db)
        .await
        .map_err(map_db_err)?;
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

    let mut db = require_db(&state.db)?;
    let contributor = match toasty::create!(schema::Contributor {
        github_user_id,
        github_username: github_username.to_string(),
    })
    .exec(&mut db)
    .await
    {
        Ok(contributor) => contributor,
        Err(error) if is_unique_violation(&error) => {
            schema::Contributor::filter_by_github_user_id(github_user_id)
                .first()
                .exec(&mut db)
                .await
                .map_err(map_db_err)?
                .ok_or_else(|| AppError::database("contributor missing after unique conflict"))?
        }
        Err(error) => return Err(map_db_err(error)),
    };

    invalidate_contributor_cache(state, github_user_id).await;
    Ok(Contributor::from(contributor))
}

pub async fn update_assignment_completion_percentage(
    state: &AppState,
    assignment_id: Uuid,
    percentage: Decimal,
) -> Result<(), AppError> {
    let mut db = require_db(&state.db)?;
    toasty::update!(schema::Assignment::filter_by_id(&assignment_id) {
        completion_percentage: Some(percentage),
    })
    .exec(&mut db)
    .await
    .map_err(map_db_err)?;
    Ok(())
}

pub async fn update_assignment_pr_merge(
    state: &AppState,
    assignment_id: Uuid,
    pr_number: i32,
) -> Result<(), AppError> {
    let mut db = require_db(&state.db)?;
    toasty::update!(schema::Assignment::filter_by_id(&assignment_id) {
        pr_number: Some(pr_number),
        pr_merged_at: Some(jiff::Timestamp::now()),
    })
    .exec(&mut db)
    .await
    .map_err(map_db_err)?;
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
    let mut db = require_db(&state.db)?;
    let repo = schema::Repo::upsert_by_github_repo_id(github_repo_id)
        .full_name(full_name)
        .owner_github_id(owner_github_id)
        .owner_username(owner_username)
        .owner_type(Some(owner_type.to_string()))
        .is_fork(is_fork)
        .is_private(is_private)
        .installer_github_id(Some(installer_github_id))
        .github_installation_id(Some(github_installation_id))
        .on_create(|repo| {
            repo.reward_low(Decimal::from(1))
                .reward_medium(Decimal::from(2))
                .reward_high(Decimal::from(3))
        })
        .exec(&mut db)
        .await
        .map_err(map_db_err)?;
    invalidate_repo_cache(state, repo.id, Some(repo.github_repo_id)).await;
    Ok(())
}

pub async fn delete_repos_by_installation_id(
    state: &AppState,
    installation_id: i64,
) -> Result<(), AppError> {
    let mut db = require_db(&state.db)?;
    let repos = schema::Repo::filter_by_github_installation_id(Some(installation_id))
        .exec(&mut db)
        .await
        .map_err(map_db_err)?;

    schema::Repo::filter_by_github_installation_id(Some(installation_id))
        .delete()
        .exec(&mut db)
        .await
        .map_err(map_db_err)?;

    for repo in repos {
        invalidate_repo_cache(state, repo.id, Some(repo.github_repo_id)).await;
    }
    Ok(())
}

pub async fn delete_repo_by_github_id(
    state: &AppState,
    github_repo_id: i64,
) -> Result<(), AppError> {
    let mut db = require_db(&state.db)?;
    let repo = get_repo_by_github_id(state, github_repo_id).await?;
    schema::Repo::filter_by_github_repo_id(github_repo_id)
        .delete()
        .exec(&mut db)
        .await
        .map_err(map_db_err)?;
    if let Some(repo) = repo {
        invalidate_repo_cache(state, repo.id, Some(repo.github_repo_id)).await;
    }
    Ok(())
}

pub async fn update_repo_installation_id(
    state: &AppState,
    github_repo_id: i64,
    installation_id: i64,
) -> Result<(), AppError> {
    let mut db = require_db(&state.db)?;
    toasty::update!(schema::Repo::filter_by_github_repo_id(github_repo_id) {
        github_installation_id: Some(installation_id),
    })
    .exec(&mut db)
    .await
    .map_err(map_db_err)?;
    Ok(())
}

pub async fn get_github_repo_id_by_full_name(
    state: &AppState,
    full_name: &str,
) -> Result<Option<i64>, AppError> {
    let mut db = require_db(&state.db)?;
    let repo = schema::Repo::filter(schema::Repo::fields().full_name().eq(full_name.to_string()))
        .first()
        .exec(&mut db)
        .await
        .map_err(map_db_err)?;
    Ok(repo.map(|repo| repo.github_repo_id))
}

pub async fn list_assignments_for_contributor(
    state: &AppState,
    contributor_id: Uuid,
) -> Result<Vec<(Assignment, Option<Issue>)>, AppError> {
    let mut db = require_db(&state.db)?;
    let assignments = schema::Assignment::filter_by_contributor_id(Some(contributor_id))
        .exec(&mut db)
        .await
        .map_err(map_db_err)?;

    let mut rows = Vec::with_capacity(assignments.len());
    for assignment in assignments {
        let assignment = Assignment::from(assignment);
        let issue = schema::Issue::filter_by_id(&assignment.issue_id)
            .first()
            .exec(&mut db)
            .await
            .map_err(map_db_err)?
            .map(Issue::from);
        rows.push((assignment, issue));
    }
    Ok(rows)
}

pub async fn ping_db(state: &AppState) -> Result<(), AppError> {
    let mut db = require_db(&state.db)?;
    let _ = schema::Repo::all()
        .limit(1)
        .exec(&mut db)
        .await
        .map_err(map_db_err)?;
    Ok(())
}
