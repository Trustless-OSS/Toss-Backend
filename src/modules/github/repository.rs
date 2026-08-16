use rust_decimal::Decimal;

use crate::{
    error::{map_db_err, require_db, AppError},
    modules::repo::repository::{get_repo_by_github_id, invalidate_repo_cache},
    shared::models::schema,
    state::AppState,
};

/// Persist a GitHub App installation repo onto the `repos` table.
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
