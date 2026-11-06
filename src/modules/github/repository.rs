use uuid::Uuid;

use crate::{
    error::{map_db_err, require_db, AppError},
    modules::repo::repository::{get_repo_by_github_id, invalidate_repo_cache},
    shared::models::schema,
    state::AppState,
};

pub async fn upsert_installation_repo(
    state: &AppState,
    github_repo_id: i64,
    full_name: &str,
    maintainer_github_id: i64,
    maintainer_username: &str,
    github_installation_id: i64,
) -> Result<(), AppError> {
    let mut db = require_db(&state.db)?;
    let existing = schema::Repository::filter_by_github_repo_id(github_repo_id)
        .first()
        .exec(&mut db)
        .await
        .map_err(map_db_err)?;
    let repo = if let Some(existing) = existing {
        // GitHub's numeric repository ID survives an App reinstall. Keep the same
        // row so funded escrows, rewards, bounties and payout history survive too.
        toasty::update!(schema::Repository::filter_by_id(existing.id) {
            full_name: full_name.to_string(),
            github_install_id: Some(github_installation_id),
        })
        .exec(&mut db)
        .await
        .map_err(map_db_err)?;
        existing
    } else {
        schema::Repository::upsert_by_github_repo_id(github_repo_id)
            .full_name(full_name.to_string())
            .github_install_id(Some(github_installation_id))
            .exec(&mut db)
            .await
            .map_err(map_db_err)?
    };
    // One idempotent insert fills missing defaults after a partial first sync
    // without overwriting a customized reward on reconnect.
    toasty::sql::statement("INSERT INTO reward_levels (id, repo_id, label, amount, created_at, updated_at) VALUES ($1, $4, 'low', 1, NOW(), NOW()), ($2, $4, 'medium', 2, NOW(), NOW()), ($3, $4, 'high', 3, NOW(), NOW()) ON CONFLICT (repo_id, label) DO NOTHING")
        .bind(Uuid::new_v4()).bind(Uuid::new_v4()).bind(Uuid::new_v4()).bind(repo.id)
        .exec(&mut db).await.map_err(map_db_err)?;

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
    Ok(())
}

pub async fn detach_repos_by_installation_id(
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

    for repo in repos {
        detach_repo_from_installation(state, repo.github_repo_id, installation_id).await?;
    }
    Ok(())
}

pub async fn detach_repo_from_installation(
    state: &AppState,
    github_repo_id: i64,
    installation_id: i64,
) -> Result<(), AppError> {
    let mut db = require_db(&state.db)?;
    let mut tx = db.transaction().await.map_err(map_db_err)?;
    let locked = toasty::sql::query("SELECT id FROM repositories WHERE github_repo_id = $1 AND github_install_id = $2 FOR UPDATE")
        .bind(github_repo_id).bind(installation_id).exec(&mut tx).await.map_err(map_db_err)?;
    if locked.is_empty() {
        return Ok(());
    }
    let repo = schema::Repository::filter_by_github_repo_id(github_repo_id)
        .first()
        .exec(&mut tx)
        .await
        .map_err(map_db_err)?
        .ok_or_else(|| AppError::internal("Repository vanished during installation detachment"))?;
    toasty::update!(schema::Repository::filter_by_id(repo.id) {
        github_install_id: Option::<i64>::None,
    })
    .exec(&mut tx)
    .await
    .map_err(map_db_err)?;
    // Old local memberships cannot authorize financial actions after App removal.
    toasty::sql::statement("DELETE FROM repo_maintainers WHERE repo_id = $1")
        .bind(repo.id)
        .exec(&mut tx)
        .await
        .map_err(map_db_err)?;
    tx.commit().await.map_err(map_db_err)?;
    invalidate_repo_cache(state, repo.id, Some(github_repo_id)).await;
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

/// Keep the stored `full_name` in sync with GitHub, keyed by the stable numeric
/// repo id. GitHub mutates `full_name` on org/user rename while the id is stable;
/// without this, every `full_name`-keyed lookup (comments, REST URLs) misses after
/// a rename and automation silently stops in renamed orgs.
pub async fn sync_repo_full_name(
    state: &AppState,
    github_repo_id: i64,
    full_name: &str,
) -> Result<(), AppError> {
    let Some(repo) = get_repo_by_github_id(state, github_repo_id).await? else {
        return Ok(());
    };
    if repo.full_name == full_name {
        return Ok(());
    }

    let mut db = require_db(&state.db)?;
    toasty::update!(schema::Repository::filter_by_github_repo_id(github_repo_id) {
        full_name: full_name.to_string(),
    })
    .exec(&mut db)
    .await
    .map_err(map_db_err)?;

    invalidate_repo_cache(state, repo.id, Some(github_repo_id)).await;
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
