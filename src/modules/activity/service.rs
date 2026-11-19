//! Pure ledger mapping, filtering, sorting, pagination, and leaderboard aggregation.
//!
//! Kept DB-free so every edge case (filter matrix, pagination math, pool deposits,
//! tie-breaking, and cross-maintainer scoping) is directly unit-testable.

use std::collections::{BTreeSet, HashMap, HashSet};

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde_json::Value;
use utoipa::ToSchema;

use crate::modules::dashboard::repository::EnrichedBounty;

use super::{
    dto::{
        ActivityEventKind, ActivityLedgerItem, ContributorLeaderboardEntry,
        PaginatedLedgerResponse, ValidatedActivityQuery, ValidatedLeaderboardQuery,
    },
    repository::{RawActivityEvent, RawPoolDeposit, RepoEventBatch},
};

fn to_utc(ts: jiff::Timestamp) -> DateTime<Utc> {
    DateTime::from_timestamp(ts.as_second(), ts.subsec_nanosecond().unsigned_abs())
        .unwrap_or(DateTime::UNIX_EPOCH)
}

fn non_empty(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

fn parse_decimal_value(value: &Value) -> Option<Decimal> {
    match value {
        Value::String(s) => s.trim().parse::<Decimal>().ok(),
        Value::Number(n) => n.to_string().parse::<Decimal>().ok(),
        _ => None,
    }
}

/// Computes `max(1, ceil(total / limit))` safely without overflow, returning `1` even when `total == 0`.
pub fn compute_total_pages(total: i64, limit: i64) -> i64 {
    debug_assert!(limit >= 1, "limit must be validated >= 1");
    if total <= 0 {
        1
    } else {
        (total - 1) / limit + 1
    }
}

/// Slices `items` into the standard `{ data, page, limit, total, total_pages }` envelope.
pub fn paginate<T: ToSchema>(items: Vec<T>, page: i64, limit: i64) -> PaginatedLedgerResponse<T> {
    let total = items.len() as i64;
    let total_pages = compute_total_pages(total, limit);
    let offset = (page - 1)
        .checked_mul(limit)
        .and_then(|v| usize::try_from(v).ok())
        .unwrap_or(usize::MAX);

    let data = items
        .into_iter()
        .skip(offset)
        .take(limit as usize)
        .collect();

    PaginatedLedgerResponse {
        data,
        page,
        limit,
        total,
        total_pages,
    }
}

/// Map a DB row from the `activity` event store to the public API ledger item.
///
/// Returns `None` if `event_type` is not one of the six known ledger kinds (never invents kinds).
pub fn map_activity_event(raw: &RawActivityEvent) -> Option<ActivityLedgerItem> {
    let kind = ActivityEventKind::parse_exact(&raw.event_type)?;
    let payload = raw.payload.as_ref();

    let at = payload
        .and_then(|p| p.get("occurred_at"))
        .and_then(Value::as_str)
        .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
        .map(|dt| dt.with_timezone(&Utc))
        .unwrap_or_else(|| to_utc(raw.occurred_at));

    let issue_number = payload
        .and_then(|p| p.get("issue_number"))
        .and_then(Value::as_i64)
        .and_then(|n| i32::try_from(n).ok());

    let issue_title = if issue_number.is_some() {
        payload
            .and_then(|p| p.get("issue_title"))
            .and_then(Value::as_str)
            .map(str::to_string)
    } else {
        None
    };

    let amount_usdc = payload
        .and_then(|p| p.get("amount_usdc"))
        .and_then(parse_decimal_value);

    let contributor = match payload.and_then(|p| p.get("contributor")) {
        Some(Value::Null) => None,
        Some(Value::String(s)) => non_empty(Some(s)),
        _ => non_empty(raw.actor_username.as_deref()),
    };

    let tx_hash = payload
        .and_then(|p| p.get("tx_hash"))
        .and_then(Value::as_str)
        .and_then(|s| non_empty(Some(s)));

    Some(ActivityLedgerItem {
        id: raw.id.to_string(),
        at,
        repo: raw.full_name.clone(),
        issue_number,
        issue_title,
        kind,
        amount_usdc,
        contributor,
        tx_hash,
    })
}

/// Derive ledger events from authoritative `Bounty` and `EscrowFunder` rows when
/// the `activity` table has not yet been populated for a repository.
pub fn derive_events_from_bounties_and_deposits(
    bounties: &[EnrichedBounty],
    deposits: &[RawPoolDeposit],
) -> Vec<ActivityLedgerItem> {
    let mut items = Vec::new();

    for eb in bounties {
        let issue = Some(eb.issue_number);
        let title = eb.issue_title.clone();
        let id = eb.id;

        if let Some(amount) = eb.reward_amount {
            items.push(ActivityLedgerItem {
                id: format!("rewarded_{id}"),
                at: to_utc(eb.created_at),
                repo: eb.repo_slug.clone(),
                issue_number: issue,
                issue_title: title.clone(),
                kind: ActivityEventKind::Rewarded,
                amount_usdc: Some(amount),
                contributor: None,
                tx_hash: None,
            });
        }

        if eb.milestone_index.is_some() {
            items.push(ActivityLedgerItem {
                id: format!("locked_{id}"),
                at: to_utc(eb.assigned_at.unwrap_or(eb.created_at)),
                repo: eb.repo_slug.clone(),
                issue_number: issue,
                issue_title: title.clone(),
                kind: ActivityEventKind::Locked,
                amount_usdc: eb.reward_amount,
                contributor: eb.contributor.clone(),
                tx_hash: None,
            });
        }

        if let Some(at) = eb.assigned_at {
            items.push(ActivityLedgerItem {
                id: format!("assigned_{id}"),
                at: to_utc(at),
                repo: eb.repo_slug.clone(),
                issue_number: issue,
                issue_title: title.clone(),
                kind: ActivityEventKind::Assigned,
                amount_usdc: None,
                contributor: eb.contributor.clone(),
                tx_hash: None,
            });
        }

        if let Some(at) = eb.paid_at {
            items.push(ActivityLedgerItem {
                id: format!("released_{id}"),
                at: to_utc(at),
                repo: eb.repo_slug.clone(),
                issue_number: issue,
                issue_title: title.clone(),
                kind: ActivityEventKind::Released,
                amount_usdc: eb.reward_amount,
                contributor: eb.contributor.clone(),
                tx_hash: non_empty(eb.payout_tx_hash.as_deref()),
            });
        }

        if (eb.status == "cancelled" || eb.status == "rejected") && eb.contributor.is_some() {
            items.push(ActivityLedgerItem {
                id: format!("rejected_{id}"),
                at: to_utc(eb.assigned_at.unwrap_or(eb.created_at)),
                repo: eb.repo_slug.clone(),
                issue_number: issue,
                issue_title: title,
                kind: ActivityEventKind::Rejected,
                amount_usdc: None,
                contributor: eb.contributor.clone(),
                tx_hash: None,
            });
        }
    }

    // Pool deposits always appear as `locked` with null issue_number and issue_title.
    for dep in deposits {
        items.push(ActivityLedgerItem {
            id: format!("deposit_{}", dep.id),
            at: to_utc(dep.funded_at),
            repo: dep.full_name.clone(),
            issue_number: None,
            issue_title: None,
            kind: ActivityEventKind::Locked,
            amount_usdc: Some(dep.amount),
            contributor: non_empty(dep.contributor.as_deref()),
            tx_hash: non_empty(Some(&dep.tx_hash)),
        });
    }

    items
}

/// Convert repository batches into normalized ledger items.
pub fn flatten_batches(batches: &[RepoEventBatch]) -> Vec<ActivityLedgerItem> {
    let mut items = Vec::new();
    for batch in batches {
        if !batch.activity_rows.is_empty() {
            items.extend(batch.activity_rows.iter().filter_map(map_activity_event));
        } else {
            items.extend(derive_events_from_bounties_and_deposits(
                &batch.bounties,
                &batch.pool_deposits,
            ));
        }
    }
    items
}

/// Restrict events strictly to the caller's maintained repositories.
pub fn scope_events_to_repos(
    events: Vec<ActivityLedgerItem>,
    allowed_repos: &HashSet<String>,
) -> Vec<ActivityLedgerItem> {
    events
        .into_iter()
        .filter(|event| allowed_repos.contains(&event.repo))
        .collect()
}

/// Filter, deterministically sort newest-first, and paginate activity ledger events.
pub fn query_activity(
    events: Vec<ActivityLedgerItem>,
    query: &ValidatedActivityQuery,
) -> PaginatedLedgerResponse<ActivityLedgerItem> {
    let mut filtered: Vec<ActivityLedgerItem> = events
        .into_iter()
        .filter(|event| query.kind.matches(event.kind))
        .filter(|event| match query.repo.as_deref() {
            Some(target) => event.repo == target,
            None => true,
        })
        .collect();

    // Newest first (`at` descending); tie-break by `id` ascending for stable pagination.
    filtered.sort_by(|a, b| b.at.cmp(&a.at).then_with(|| a.id.cmp(&b.id)));

    paginate(filtered, query.page, query.limit)
}

/// Aggregate, rank, and paginate the maintainer-scoped contributor leaderboard.
///
/// - Includes users with at least one `assigned` or `released` event (unpaid assignees have `earned_usdc: 0`).
/// - Excludes users with neither `assigned` nor `released` activity.
/// - Sorts by `earned_usdc` desc -> `merged_prs` desc -> `github_username` asc.
/// - Assigns global 1-based `rank` after sort and sorts each contributor's `repos` list.
pub fn build_leaderboard(
    events: &[ActivityLedgerItem],
    query: &ValidatedLeaderboardQuery,
) -> PaginatedLedgerResponse<ContributorLeaderboardEntry> {
    struct Agg {
        earned_usdc: Decimal,
        merged_prs: i64,
        assigned: i64,
        repos: BTreeSet<String>,
    }

    let mut by_user: HashMap<String, Agg> = HashMap::new();

    for event in events {
        if let Some(target_repo) = query.repo.as_deref() {
            if event.repo != target_repo {
                continue;
            }
        }

        if !matches!(
            event.kind,
            ActivityEventKind::Assigned | ActivityEventKind::Released
        ) {
            continue;
        }

        let Some(username) = non_empty(event.contributor.as_deref()) else {
            continue;
        };

        let agg = by_user.entry(username).or_insert_with(|| Agg {
            earned_usdc: Decimal::ZERO,
            merged_prs: 0,
            assigned: 0,
            repos: BTreeSet::new(),
        });

        agg.repos.insert(event.repo.clone());

        match event.kind {
            ActivityEventKind::Released => {
                agg.merged_prs += 1;
                agg.earned_usdc += event.amount_usdc.unwrap_or(Decimal::ZERO);
            }
            ActivityEventKind::Assigned => {
                agg.assigned += 1;
            }
            _ => {}
        }
    }

    let mut entries: Vec<ContributorLeaderboardEntry> = by_user
        .into_iter()
        .map(|(github_username, agg)| ContributorLeaderboardEntry {
            rank: 0,
            github_username,
            earned_usdc: agg.earned_usdc,
            merged_prs: agg.merged_prs,
            assigned: agg.assigned,
            repos: agg.repos.into_iter().collect(),
        })
        .collect();

    entries.sort_by(|a, b| {
        b.earned_usdc
            .cmp(&a.earned_usdc)
            .then_with(|| b.merged_prs.cmp(&a.merged_prs))
            .then_with(|| a.github_username.cmp(&b.github_username))
    });

    for (idx, entry) in entries.iter_mut().enumerate() {
        entry.rank = idx as i64 + 1;
    }

    paginate(entries, query.page, query.limit)
}
