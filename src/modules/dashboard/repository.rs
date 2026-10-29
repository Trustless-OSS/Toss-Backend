//! Data access for the dashboard & leaderboard.
//!
//! All reads come from `Bounty` rows (the authoritative state) joined with
//! `Profile` (contributor username) and `Repository` (the `owner/name` slug).
//! We follow the codebase convention (see `repo::repository::list_repos_for_user`)
//! of fetching rows and filtering/joining in Rust rather than SQL joins, because
//! Toasty's query API here exposes only single-field `filter_by_*` helpers.

use std::collections::HashMap;

use rust_decimal::Decimal;
use uuid::Uuid;

use crate::{
    error::{map_db_err, require_db, AppError},
    shared::models::schema,
    state::AppState,
};

/// The fields of a bounty the dashboard derives everything from, already joined
/// to its repo slug and assignee username. Decoupled from the Toasty model so
/// the derivation logic in `service` stays DB-free and unit-testable.
#[derive(Debug, Clone)]
pub struct EnrichedBounty {
    pub id: Uuid,
    pub repo_slug: String,
    pub issue_number: i32,
    pub issue_title: Option<String>,
    pub reward_amount: Option<Decimal>,
    pub status: String,
    pub contributor: Option<String>,
    pub milestone_index: Option<i32>,
    pub created_at: jiff::Timestamp,
    pub assigned_at: Option<jiff::Timestamp>,
    pub paid_at: Option<jiff::Timestamp>,
    pub payout_tx_hash: Option<String>,
}

/// Repo ids a maintainer owns, resolved from their GitHub id via `RepoMaintainer`.
/// Empty vec means "no repos" (new maintainer / unknown user) — callers return empty data.
pub async fn maintainer_repo_ids(state: &AppState, github_id: i64) -> Result<Vec<Uuid>, AppError> {
    let mut db = require_db(&state.db)?;

    let Some(profile) = schema::Profile::filter_by_github_id(github_id)
        .first()
        .exec(&mut db)
        .await
        .map_err(map_db_err)?
    else {
        return Ok(vec![]);
    };

    let rows = schema::RepoMaintainer::filter_by_profile_id(profile.id)
        .exec(&mut db)
        .await
        .map_err(map_db_err)?;

    Ok(rows.into_iter().map(|row| row.repo_id).collect())
}

/// Enriched bounties for a set of repos (maintainer-scoped feed).
pub async fn enriched_bounties_for_repos(
    state: &AppState,
    repo_ids: &[Uuid],
) -> Result<Vec<EnrichedBounty>, AppError> {
    if repo_ids.is_empty() {
        return Ok(vec![]);
    }
    let mut db = require_db(&state.db)?;

    let mut out = Vec::new();
    for &repo_id in repo_ids {
        let slug = match schema::Repository::filter_by_id(repo_id)
            .first()
            .exec(&mut db)
            .await
            .map_err(map_db_err)?
        {
            Some(repo) => repo.full_name,
            None => continue,
        };

        let bounties = schema::Bounty::filter_by_repo_id(repo_id)
            .exec(&mut db)
            .await
            .map_err(map_db_err)?;

        for bounty in bounties {
            let contributor = resolve_username(&mut db, bounty.assignee_id).await?;
            out.push(enrich(bounty, slug.clone(), contributor));
        }
    }
    Ok(out)
}

/// Every enriched bounty on the platform (public leaderboard).
pub async fn all_enriched_bounties(state: &AppState) -> Result<Vec<EnrichedBounty>, AppError> {
    let mut db = require_db(&state.db)?;

    let repos = schema::Repository::all()
        .exec(&mut db)
        .await
        .map_err(map_db_err)?;
    let slug_by_repo: HashMap<Uuid, String> =
        repos.into_iter().map(|r| (r.id, r.full_name)).collect();

    let bounties = schema::Bounty::all()
        .exec(&mut db)
        .await
        .map_err(map_db_err)?;

    let mut out = Vec::with_capacity(bounties.len());
    for bounty in bounties {
        let Some(slug) = slug_by_repo.get(&bounty.repo_id).cloned() else {
            continue; // orphan bounty with no repo row — skip
        };
        let contributor = resolve_username(&mut db, bounty.assignee_id).await?;
        out.push(enrich(bounty, slug, contributor));
    }
    Ok(out)
}

fn enrich(
    bounty: schema::Bounty,
    repo_slug: String,
    contributor: Option<String>,
) -> EnrichedBounty {
    EnrichedBounty {
        id: bounty.id,
        repo_slug,
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
    }
}

async fn resolve_username(
    db: &mut toasty::Db,
    profile_id: Option<Uuid>,
) -> Result<Option<String>, AppError> {
    let Some(id) = profile_id else {
        return Ok(None);
    };
    Ok(schema::Profile::filter_by_id(id)
        .first()
        .exec(db)
        .await
        .map_err(map_db_err)?
        .map(|p| p.username))
}
