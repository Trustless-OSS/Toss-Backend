use uuid::Uuid;

use crate::{
    error::{is_unique_violation, map_db_err, require_db, AppError},
    infra::cache_keys,
    shared::models::{schema, Assignment, Contributor, Issue},
    state::AppState,
};

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

pub async fn get_contributor_by_id(
    state: &AppState,
    contributor_id: Uuid,
) -> Result<Option<Contributor>, AppError> {
    let mut db = require_db(&state.db)?;
    Ok(schema::Contributor::filter_by_id(&contributor_id)
        .first()
        .exec(&mut db)
        .await
        .map_err(map_db_err)?
        .map(Contributor::from))
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
