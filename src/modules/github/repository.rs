use rust_decimal::Decimal;

use crate::{
    error::{map_db_err, require_db, AppError},
    modules::repo::repository::{get_repo_by_github_id, invalidate_repo_cache},
    shared::models::schema,
    state::AppState,
};

#[allow(clippy::too_many_arguments)]
pub async fn upsert_installation_repo(
    state: &AppState,
    github_repo_id: i64,
    full_name: &str,
    maintainer_github_id: i64,
    maintainer_username: &str,
    installer_github_id: i64,
    github_installation_id: i64,
) -> Result<(), AppError> {
    let mut db = require_db(&state.db)?;
    let repo = schema::Repository::upsert_by_github_repo_id(github_repo_id)
        .full_name(full_name.to_string())
        .github_install_id(Some(github_installation_id))
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

    if installer_github_id != maintainer_github_id {
        let installer = schema::Profile::upsert_by_github_id(installer_github_id)
            .username(format!("user_{installer_github_id}"))
            .exec(&mut db)
            .await
            .map_err(map_db_err)?;

        schema::RepoMaintainer::upsert_by_repo_id_and_profile_id(repo.id, installer.id)
            .role("maintainer".to_string())
            .exec(&mut db)
            .await
            .map_err(map_db_err)?;
    }

    invalidate_repo_cache(state, repo.id, Some(repo.github_repo_id)).await;
    Ok(())
}

pub async fn delete_repos_by_installation_id(
    state: &AppState,
    installation_id: i64,
) -> Result<(), AppError> {
    let mut db = require_db(&state.db)?;
    let repos = schema::Repository::filter(
        schema::Repository::fields()
            .github_install_id()
            .eq(Some(installation_id)),
    )
    .exec(&mut db)
    .await
    .map_err(map_db_err)?;

    schema::Repository::filter(
        schema::Repository::fields()
            .github_install_id()
            .eq(Some(installation_id)),
    )
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
    schema::Repository::filter_by_github_repo_id(github_repo_id)
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
    toasty::update!(schema::Repository::filter_by_github_repo_id(github_repo_id) {
        github_install_id: Some(installation_id),
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
    let repo = schema::Repository::filter(
        schema::Repository::fields()
            .full_name()
            .eq(full_name.to_string()),
    )
    .first()
    .exec(&mut db)
    .await
    .map_err(map_db_err)?;
    Ok(repo.map(|r| r.github_repo_id))
}
