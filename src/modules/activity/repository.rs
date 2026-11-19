//! Maintainer-scoped data access for the activity ledger and contributor leaderboard.

use std::collections::HashMap;

use rust_decimal::Decimal;
use serde_json::Value;
use uuid::Uuid;

use crate::{
    error::{map_db_err, require_db, AppError},
    modules::dashboard::repository::EnrichedBounty,
    shared::models::schema,
    state::AppState,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MaintainerRepo {
    pub id: Uuid,
    pub full_name: String,
}

/// Normalized representation of a row from the `activity` event store.
#[derive(Debug, Clone)]
pub struct RawActivityEvent {
    pub id: Uuid,
    pub full_name: String,
    pub event_type: String,
    pub payload: Option<Value>,
    pub actor_username: Option<String>,
    pub occurred_at: jiff::Timestamp,
}

/// Normalized representation of a pool deposit row from `escrow_funders`.
#[derive(Debug, Clone)]
pub struct RawPoolDeposit {
    pub id: Uuid,
    pub full_name: String,
    pub amount: Decimal,
    pub tx_hash: String,
    pub funded_at: jiff::Timestamp,
    pub contributor: Option<String>,
}

/// Per-repository source rows loaded for a maintainer.
///
/// When `activity_rows` is non-empty, those authoritative event-store rows are
/// used directly; otherwise events are derived from `bounties` + `pool_deposits`.
#[derive(Debug, Clone, Default)]
pub struct RepoEventBatch {
    pub activity_rows: Vec<RawActivityEvent>,
    pub bounties: Vec<EnrichedBounty>,
    pub pool_deposits: Vec<RawPoolDeposit>,
}

/// Resolve all repositories maintained by `github_id` via `RepoMaintainer`.
pub async fn maintainer_repos(
    state: &AppState,
    github_id: i64,
) -> Result<Vec<MaintainerRepo>, AppError> {
    let mut db = require_db(&state.db)?;

    let Some(profile) = schema::Profile::filter_by_github_id(github_id)
        .first()
        .exec(&mut db)
        .await
        .map_err(map_db_err)?
    else {
        return Ok(vec![]);
    };

    let links = schema::RepoMaintainer::filter_by_profile_id(profile.id)
        .exec(&mut db)
        .await
        .map_err(map_db_err)?;

    let mut repos = Vec::with_capacity(links.len());
    for link in links {
        if let Some(repo) = schema::Repository::filter_by_id(link.repo_id)
            .first()
            .exec(&mut db)
            .await
            .map_err(map_db_err)?
        {
            repos.push(MaintainerRepo {
                id: repo.id,
                full_name: repo.full_name,
            });
        }
    }

    Ok(repos)
}

/// Load raw event batches restricted strictly to repositories maintained by `github_id`.
///
/// If `repo_filter` is provided and does not belong to the caller, returns an
/// empty list without querying any other maintainer's rows.
pub async fn load_maintainer_event_batches(
    state: &AppState,
    github_id: i64,
    repo_filter: Option<&str>,
) -> Result<Vec<RepoEventBatch>, AppError> {
    let mut repos = maintainer_repos(state, github_id).await?;
    if let Some(target_repo) = repo_filter {
        repos.retain(|repo| repo.full_name == target_repo);
    }
    if repos.is_empty() {
        return Ok(vec![]);
    }

    let mut db = require_db(&state.db)?;
    let mut username_cache: HashMap<Uuid, Option<String>> = HashMap::new();
    let mut batches = Vec::with_capacity(repos.len());

    for repo in repos {
        let activity_records = schema::Activity::filter_by_repo_id(Some(repo.id))
            .exec(&mut db)
            .await
            .map_err(map_db_err)?;

        if !activity_records.is_empty() {
            let mut activity_rows = Vec::with_capacity(activity_records.len());
            for row in activity_records {
                let actor_username =
                    resolve_username_cached(&mut db, row.actor_id, &mut username_cache).await?;
                activity_rows.push(RawActivityEvent {
                    id: row.id,
                    full_name: repo.full_name.clone(),
                    event_type: row.event_type,
                    payload: row.payload,
                    actor_username,
                    occurred_at: row.created_at,
                });
            }
            batches.push(RepoEventBatch {
                activity_rows,
                bounties: vec![],
                pool_deposits: vec![],
            });
            continue;
        }

        // Fallback when the `activity` table has not yet been populated for this repo:
        // derive events from authoritative `bounties` + `escrow_funders` rows.
        let bounty_records = schema::Bounty::filter_by_repo_id(repo.id)
            .exec(&mut db)
            .await
            .map_err(map_db_err)?;

        let mut bounties = Vec::with_capacity(bounty_records.len());
        for bounty in bounty_records {
            let contributor =
                resolve_username_cached(&mut db, bounty.assignee_id, &mut username_cache).await?;
            bounties.push(EnrichedBounty {
                id: bounty.id,
                repo_slug: repo.full_name.clone(),
                issue_number: bounty.github_issue_number,
                issue_title: bounty.title,
                reward_amount: bounty.reward_amount,
                status: bounty.status,
                contributor,
                milestone_index: bounty.milestone_index,
                created_at: bounty.created_at,
                assigned_at: bounty.assigned_at,
                paid_at: bounty.paid_at,
                payout_tx_hash: bounty.payout_tx_hash,
            });
        }

        let deposit_records = schema::EscrowFunder::filter_by_repo_id(repo.id)
            .exec(&mut db)
            .await
            .map_err(map_db_err)?;

        let mut pool_deposits = Vec::with_capacity(deposit_records.len());
        for deposit in deposit_records {
            let contributor =
                resolve_username_cached(&mut db, deposit.profile_id, &mut username_cache).await?;
            pool_deposits.push(RawPoolDeposit {
                id: deposit.id,
                full_name: repo.full_name.clone(),
                amount: deposit.amount,
                tx_hash: deposit.tx_hash,
                funded_at: deposit.funded_at,
                contributor,
            });
        }

        batches.push(RepoEventBatch {
            activity_rows: vec![],
            bounties,
            pool_deposits,
        });
    }

    Ok(batches)
}

async fn resolve_username_cached(
    db: &mut toasty::Db,
    profile_id: Option<Uuid>,
    cache: &mut HashMap<Uuid, Option<String>>,
) -> Result<Option<String>, AppError> {
    let Some(id) = profile_id else {
        return Ok(None);
    };
    if let Some(cached) = cache.get(&id) {
        return Ok(cached.clone());
    }
    let username = schema::Profile::filter_by_id(id)
        .first()
        .exec(db)
        .await
        .map_err(map_db_err)?
        .map(|profile| profile.username);
    cache.insert(id, username.clone());
    Ok(username)
}
