use rust_decimal::Decimal;
use uuid::Uuid;

use crate::{
    error::{map_db_err, require_db, AppError},
    infra::cache_keys,
    shared::models::{schema, Repo},
    state::AppState,
};

pub async fn get_repo_by_id(state: &AppState, repo_id: Uuid) -> Result<Option<Repo>, AppError> {
    let cache_key = cache_keys::repo(repo_id);
    if let Some(cached) = state.cache.get::<Repo>(&cache_key).await {
        return Ok(Some(cached));
    }

    let mut db = require_db(&state.db)?;
    let repo = schema::Repo::filter_by_id(repo_id)
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

    let owned_or_installed =
        schema::Repo::fields()
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

    let repos = schema::Repo::filter(
        owned_or_installed
            .and(public_not_fork)
            .and(user_or_installer),
    )
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

pub async fn delete_repo_cascade(state: &AppState, repo_id: Uuid) -> Result<(), AppError> {
    let mut db = require_db(&state.db)?;
    let mut tx = db.transaction().await.map_err(map_db_err)?;

    let issues = schema::Issue::filter_by_repo_id(repo_id)
        .exec(&mut tx)
        .await
        .map_err(map_db_err)?;

    for issue in issues {
        schema::Assignment::filter_by_issue_id(issue.id)
            .delete()
            .exec(&mut tx)
            .await
            .map_err(map_db_err)?;
    }

    schema::Issue::filter_by_repo_id(repo_id)
        .delete()
        .exec(&mut tx)
        .await
        .map_err(map_db_err)?;

    schema::Repo::filter_by_id(repo_id)
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
    Ok(repos
        .into_iter()
        .filter(|repo| repo.id != exclude_repo_id)
        .count() as i64)
}

pub async fn is_maintainer(
    state: &AppState,
    github_user_id: i64,
    repo_id: Uuid,
) -> Result<bool, AppError> {
    let mut db = require_db(&state.db)?;
    let repo = schema::Repo::filter_by_id(repo_id)
        .first()
        .exec(&mut db)
        .await
        .map_err(map_db_err)?;

    Ok(repo.is_some_and(|repo| {
        repo.owner_github_id == github_user_id || repo.installer_github_id == Some(github_user_id)
    }))
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
