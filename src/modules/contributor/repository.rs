use uuid::Uuid;

use crate::{
    error::{is_unique_violation, map_db_err, require_db, AppError},
    infra::cache_keys,
    shared::models::{schema, Bounty, Profile, Wallet},
    state::AppState,
};

pub async fn get_profile_by_github_id(
    state: &AppState,
    github_id: i64,
) -> Result<Option<Profile>, AppError> {
    let cache_key = cache_keys::contrib(github_id);
    if let Some(cached) = state.cache.get::<Profile>(&cache_key).await {
        return Ok(Some(cached));
    }

    let mut db = require_db(&state.db)?;
    let profile = schema::Profile::filter_by_github_id(github_id)
        .first()
        .exec(&mut db)
        .await
        .map_err(map_db_err)?
        .map(Profile::from);

    if let Some(ref profile) = profile {
        state
            .cache
            .set(&cache_key, profile, Some(cache_keys::CONTRIB_TTL))
            .await;
    }

    Ok(profile)
}

pub async fn invalidate_contributor_cache(state: &AppState, github_id: i64) {
    state
        .cache
        .invalidate(&cache_keys::contrib(github_id))
        .await;
}

pub async fn set_profile_email(
    state: &AppState,
    github_id: i64,
    email: &str,
) -> Result<(), AppError> {
    let mut db = require_db(&state.db)?;

    toasty::update!(schema::Profile::filter_by_github_id(github_id) {
        email: Some(email.to_string()),
    })
    .exec(&mut db)
    .await
    .map_err(map_db_err)?;

    invalidate_contributor_cache(state, github_id).await;
    Ok(())
}

pub async fn upsert_contributor_wallet(
    state: &AppState,
    github_id: i64,
    username: &str,
    chain: &str,
    address: &str,
) -> Result<(), AppError> {
    let mut db = require_db(&state.db)?;

    let profile = schema::Profile::upsert_by_github_id(github_id)
        .username(username.to_string())
        .exec(&mut db)
        .await
        .map_err(map_db_err)?;

    schema::Wallet::upsert_by_profile_id_and_chain_and_address(
        profile.id,
        chain.to_string(),
        address.to_string(),
    )
    .is_primary(true)
    .exec(&mut db)
    .await
    .map_err(map_db_err)?;

    invalidate_contributor_cache(state, github_id).await;
    Ok(())
}

pub async fn ensure_contributor(
    state: &AppState,
    github_id: i64,
    username: &str,
) -> Result<Profile, AppError> {
    ensure_contributor_with_email(state, github_id, username, None).await
}

pub async fn ensure_contributor_with_email(
    state: &AppState,
    github_id: i64,
    username: &str,
    email: Option<&str>,
) -> Result<Profile, AppError> {
    if let Some(profile) = get_profile_by_github_id(state, github_id).await? {
        // Update email if provided and not already set
        if let Some(email) = email {
            if !email.is_empty() && profile.email.is_none() {
                let _ = set_profile_email(state, github_id, email).await;
            }
        }
        return Ok(profile);
    }

    let mut db = require_db(&state.db)?;
    let profile = match toasty::create!(schema::Profile {
        github_id,
        username: username.to_string(),
        email: email.map(|e| e.to_string()),
    })
    .exec(&mut db)
    .await
    {
        Ok(profile) => profile,
        Err(error) if is_unique_violation(&error) => {
            schema::Profile::filter_by_github_id(github_id)
                .first()
                .exec(&mut db)
                .await
                .map_err(map_db_err)?
                .ok_or_else(|| AppError::database("profile missing after unique conflict"))?
        }
        Err(error) => return Err(map_db_err(error)),
    };

    invalidate_contributor_cache(state, github_id).await;
    Ok(Profile::from(profile))
}

pub async fn get_profile_by_id(
    state: &AppState,
    profile_id: Uuid,
) -> Result<Option<Profile>, AppError> {
    let mut db = require_db(&state.db)?;
    Ok(schema::Profile::filter_by_id(profile_id)
        .first()
        .exec(&mut db)
        .await
        .map_err(map_db_err)?
        .map(Profile::from))
}

pub async fn get_wallets_for_profile(
    state: &AppState,
    profile_id: Uuid,
) -> Result<Vec<Wallet>, AppError> {
    let mut db = require_db(&state.db)?;
    Ok(schema::Wallet::filter_by_profile_id(profile_id)
        .exec(&mut db)
        .await
        .map_err(map_db_err)?
        .into_iter()
        .map(Wallet::from)
        .collect())
}

pub async fn list_bounties_for_contributor(
    state: &AppState,
    profile_id: Uuid,
) -> Result<Vec<Bounty>, AppError> {
    let mut db = require_db(&state.db)?;
    Ok(schema::Bounty::filter_by_assignee_id(Some(profile_id))
        .exec(&mut db)
        .await
        .map_err(map_db_err)?
        .into_iter()
        .map(Bounty::from)
        .collect())
}

// Legacy name used by issue_assigned.rs webhook handler
pub use ensure_contributor as ensure_contributor_by_github_id;
pub use get_profile_by_github_id as get_contributor_by_github_id;
pub use get_profile_by_id as get_contributor_by_id;

// list_assignments_for_contributor — old callers expect Vec<(Assignment, Option<Issue>)>.
// Now returns Vec<(Bounty, Option<Bounty>)> where both items are the same bounty
// (the second slot previously held the Issue; it's now unified into Bounty itself).
pub async fn list_assignments_for_contributor(
    state: &AppState,
    profile_id: Uuid,
) -> Result<Vec<(Bounty, Option<Bounty>)>, AppError> {
    let bounties = list_bounties_for_contributor(state, profile_id).await?;
    Ok(bounties.into_iter().map(|b| (b.clone(), Some(b))).collect())
}
