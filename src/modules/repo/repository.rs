use rust_decimal::Decimal;
use uuid::Uuid;

use crate::{
    error::{map_db_err, require_db, AppError},
    infra::cache_keys,
    shared::models::{schema, Repo, Reward},
    state::AppState,
};

pub async fn get_repo_by_id(state: &AppState, repo_id: Uuid) -> Result<Option<Repo>, AppError> {
    let cache_key = cache_keys::repo(repo_id);
    if let Some(cached) = state.cache.get::<Repo>(&cache_key).await {
        return Ok(Some(cached));
    }

    let mut db = require_db(&state.db)?;
    let repo = schema::Repositories::filter_by_id(repo_id)
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
    let repo = schema::Repositories::filter_by_github_repo_id(github_repo_id)
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

    // Find the profile for this user so we can look up their maintainer repos.
    let profile = if github_id != 0 {
        schema::Profile::filter_by_github_id(github_id)
            .first()
            .exec(&mut db)
            .await
            .map_err(map_db_err)?
    } else {
        None
    };

    let all_repos: Vec<Repo> = if let Some(ref profile) = profile {
        let maintainer_rows = schema::RepoMaintainer::filter_by_profile_id(profile.id)
            .exec(&mut db)
            .await
            .map_err(map_db_err)?;

        let mut repos = Vec::new();
        for row in maintainer_rows {
            if let Some(repo) = schema::Repositories::filter_by_id(row.repo_id)
                .first()
                .exec(&mut db)
                .await
                .map_err(map_db_err)?
            {
                repos.push(Repo::from(repo));
            }
        }
        repos
    } else if let Some(username) = github_username.filter(|v| !v.is_empty()) {
        // Fallback: match by full_name prefix (owner/*)
        let prefix = format!("{username}/");
        let all = schema::Repositories::all()
            .exec(&mut db)
            .await
            .map_err(map_db_err)?;
        all.into_iter()
            .filter(|r| r.full_name.starts_with(&prefix))
            .map(Repo::from)
            .collect()
    } else {
        return Err(AppError::bad_request(
            "Could not determine GitHub identity from session",
        ));
    };

    let total = all_repos.len() as i64;
    let repos = all_repos.into_iter().skip(offset).take(limit).collect();
    Ok((repos, total))
}

pub async fn upsert_repo(
    state: &AppState,
    github_repo_id: i64,
    full_name: &str,
    github_install_id: Option<i64>,
) -> Result<Repo, AppError> {
    let mut db = require_db(&state.db)?;
    let repo = schema::Repositories::upsert_by_github_repo_id(github_repo_id)
        .full_name(full_name.to_string())
        .github_install_id(github_install_id)
        .exec(&mut db)
        .await
        .map_err(map_db_err)?;

    for (label, amount) in [
        ("low", Decimal::from(1)),
        ("medium", Decimal::from(2)),
        ("high", Decimal::from(3)),
    ] {
        schema::Reward::upsert_by_repo_id_and_label(repo.id, label.to_string())
            .amount(amount)
            .exec(&mut db)
            .await
            .map_err(map_db_err)?;
    }

    Ok(Repo::from(repo))
}

pub async fn upsert_repo_with_maintainer(
    state: &AppState,
    github_repo_id: i64,
    full_name: &str,
    github_install_id: Option<i64>,
    maintainer_github_id: i64,
    maintainer_username: &str,
) -> Result<Repo, AppError> {
    let repo = upsert_repo(state, github_repo_id, full_name, github_install_id).await?;

    let mut db = require_db(&state.db)?;
    let profile = schema::Profile::upsert_by_github_id(maintainer_github_id)
        .username(maintainer_username.to_string())
        .exec(&mut db)
        .await
        .map_err(map_db_err)?;

    schema::RepoMaintainer::upsert_by_repo_id_and_profile_id(repo.id, profile.id)
        .role("owner".to_string())
        .exec(&mut db)
        .await
        .map_err(map_db_err)?;

    invalidate_repo_cache(state, repo.id, Some(repo.github_repo_id)).await;
    Ok(repo)
}

pub async fn update_repo_rewards(
    state: &AppState,
    repo_id: Uuid,
    label: String,
    amount: Decimal,
) -> Result<Repo, AppError> {
    let mut db = require_db(&state.db)?;

    schema::Reward::upsert_by_repo_id_and_label(repo_id, label)
        .amount(amount)
        .exec(&mut db)
        .await
        .map_err(map_db_err)?;

    let schema_repo = schema::Repositories::get_by_id(&mut db, &repo_id)
        .await
        .map_err(map_db_err)?;

    let rewards = schema::Reward::filter_by_repo_id(repo_id)
        .exec(&mut db)
        .await
        .map_err(map_db_err)?
        .into_iter()
        .map(Reward::from)
        .collect::<Vec<_>>();

    let mut repo = Repo::from(schema_repo);
    repo.rewards = rewards;

    invalidate_repo_cache(state, repo.id, Some(repo.github_repo_id)).await;
    Ok(repo)
}

pub async fn delete_repo_cascade(state: &AppState, repo_id: Uuid) -> Result<(), AppError> {
    let mut db = require_db(&state.db)?;
    let mut tx = db.transaction().await.map_err(map_db_err)?;

    // Delete all bounties for the repo (cascade handles assignments via FK in old schema,
    // but now bounties are standalone rows referencing the repo).
    schema::Bounty::filter_by_repo_id(repo_id)
        .delete()
        .exec(&mut tx)
        .await
        .map_err(map_db_err)?;

    schema::RepoMaintainer::filter_by_repo_id(repo_id)
        .delete()
        .exec(&mut tx)
        .await
        .map_err(map_db_err)?;

    schema::Repositories::filter_by_id(repo_id)
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
    let repos = schema::Repositories::filter(
        schema::Repositories::fields()
            .github_install_id()
            .eq(Some(installation_id)),
    )
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

    let profile = schema::Profile::filter_by_github_id(github_user_id)
        .first()
        .exec(&mut db)
        .await
        .map_err(map_db_err)?;

    let Some(profile) = profile else {
        return Ok(false);
    };

    let maintainer = schema::RepoMaintainer::filter_by_repo_id_and_profile_id(repo_id, profile.id)
        .first()
        .exec(&mut db)
        .await
        .map_err(map_db_err)?;

    Ok(maintainer.is_some())
}

pub async fn ping_db(state: &AppState) -> Result<(), AppError> {
    let mut db = require_db(&state.db)?;
    let _ = schema::Repositories::all()
        .limit(1)
        .exec(&mut db)
        .await
        .map_err(map_db_err)?;
    Ok(())
}
