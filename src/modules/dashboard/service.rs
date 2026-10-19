//! Derivation logic for the dashboard & leaderboard.
//!
//! Pure functions over `EnrichedBounty` turn authoritative bounty state into the
//! event feed and the aggregates the frontend expects. Keeping them DB-free makes
//! them unit-testable (see the self-check at the bottom).

use std::collections::BTreeMap;

use chrono::{DateTime, Datelike, Utc};
use rust_decimal::Decimal;

use super::{
    model::{
        ActivityKind, ActivityRow, DashboardSummary, FundsMovementPoint, LeaderboardEntry,
        LeaderboardPage,
    },
    repository::EnrichedBounty,
};

const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

fn to_utc(t: jiff::Timestamp) -> DateTime<Utc> {
    DateTime::from_timestamp(t.as_second(), t.subsec_nanosecond().unsigned_abs())
        .unwrap_or(DateTime::UNIX_EPOCH)
}

/// Build the activity feed, newest first. One bounty can emit several events
/// (rewarded, locked, assigned, released, rejected) depending on its state.
pub fn activity_feed(bounties: &[EnrichedBounty]) -> Vec<ActivityRow> {
    let mut rows = Vec::new();

    for eb in bounties {
        let issue = Some(eb.issue_number);
        let title = eb.issue_title.clone();
        let id = eb.id;

        // rewarded: a bounty amount was set on the issue.
        if let Some(amount) = eb.reward_amount {
            rows.push(ActivityRow {
                id: format!("rewarded_{id}"),
                at: to_utc(eb.created_at),
                repo: eb.repo_slug.clone(),
                issue_number: issue,
                issue_title: title.clone(),
                kind: ActivityKind::Rewarded,
                amount_usdc: Some(amount),
                contributor: None,
                tx_hash: None,
            });
        }

        // locked: funds escrowed (bounty pushed to an on-chain milestone).
        if eb.milestone_index.is_some() {
            rows.push(ActivityRow {
                id: format!("locked_{id}"),
                at: to_utc(eb.assigned_at.unwrap_or(eb.created_at)),
                repo: eb.repo_slug.clone(),
                issue_number: issue,
                issue_title: title.clone(),
                kind: ActivityKind::Locked,
                amount_usdc: eb.reward_amount,
                contributor: eb.contributor.clone(),
                tx_hash: None, // tx hashes are not persisted per-bounty yet
            });
        }

        // assigned: a contributor was assigned.
        if let Some(at) = eb.assigned_at {
            rows.push(ActivityRow {
                id: format!("assigned_{id}"),
                at: to_utc(at),
                repo: eb.repo_slug.clone(),
                issue_number: issue,
                issue_title: title.clone(),
                kind: ActivityKind::Assigned,
                amount_usdc: None,
                contributor: eb.contributor.clone(),
                tx_hash: None,
            });
        }

        // released: bounty paid out (counts as a merged PR + earnings).
        if let Some(at) = eb.paid_at {
            rows.push(ActivityRow {
                id: format!("released_{id}"),
                at: to_utc(at),
                repo: eb.repo_slug.clone(),
                issue_number: issue,
                issue_title: title.clone(),
                kind: ActivityKind::Released,
                amount_usdc: eb.reward_amount,
                contributor: eb.contributor.clone(),
                tx_hash: eb.payout_tx_hash.clone(),
            });
        }

        // rejected: a cancelled bounty that had an assignee is a rejected claim.
        if eb.status == "cancelled" && eb.contributor.is_some() {
            rows.push(ActivityRow {
                id: format!("rejected_{id}"),
                at: to_utc(eb.assigned_at.unwrap_or(eb.created_at)),
                repo: eb.repo_slug.clone(),
                issue_number: issue,
                issue_title: title.clone(),
                kind: ActivityKind::Rejected,
                amount_usdc: None,
                contributor: eb.contributor.clone(),
                tx_hash: None,
            });
        }
    }

    rows.sort_by_key(|r| std::cmp::Reverse(r.at));
    rows
}

/// Section-1 aggregates, computed over the full feed (same rules as the UI).
pub fn summary(feed: &[ActivityRow]) -> DashboardSummary {
    let mut funds_locked = Decimal::ZERO;
    let mut funds_released = Decimal::ZERO;
    let mut issues_rewarded = 0i64;
    let mut contributors = std::collections::HashSet::new();

    for row in feed {
        match row.kind {
            ActivityKind::Locked => funds_locked += row.amount_usdc.unwrap_or(Decimal::ZERO),
            ActivityKind::Released => funds_released += row.amount_usdc.unwrap_or(Decimal::ZERO),
            ActivityKind::Rewarded => issues_rewarded += 1,
            _ => {}
        }
        if let Some(name) = &row.contributor {
            contributors.insert(name.clone());
        }
    }

    DashboardSummary {
        funds_locked,
        funds_released,
        issues_rewarded,
        contributors: contributors.len() as i64,
    }
}

/// Monthly cumulative TVL (locked) vs payouts (released), chronological.
pub fn funds_movement(feed: &[ActivityRow]) -> Vec<FundsMovementPoint> {
    // Per-month (period) deltas, then accumulate in chronological order.
    let mut locked_by_month: BTreeMap<String, Decimal> = BTreeMap::new();
    let mut released_by_month: BTreeMap<String, Decimal> = BTreeMap::new();

    for row in feed {
        let period = format!("{:04}-{:02}", row.at.year(), row.at.month());
        let amount = row.amount_usdc.unwrap_or(Decimal::ZERO);
        match row.kind {
            ActivityKind::Locked => *locked_by_month.entry(period).or_default() += amount,
            ActivityKind::Released => *released_by_month.entry(period).or_default() += amount,
            _ => {}
        }
    }

    // Union of all periods, sorted (BTreeMap keys are already sorted lexically = chronologically for YYYY-MM).
    let mut periods: Vec<String> = locked_by_month
        .keys()
        .chain(released_by_month.keys())
        .cloned()
        .collect();
    periods.sort();
    periods.dedup();

    let mut tvl = Decimal::ZERO;
    let mut payouts = Decimal::ZERO;
    periods
        .into_iter()
        .map(|period| {
            tvl += locked_by_month
                .get(&period)
                .copied()
                .unwrap_or(Decimal::ZERO);
            payouts += released_by_month
                .get(&period)
                .copied()
                .unwrap_or(Decimal::ZERO);
            let month = period
                .split('-')
                .nth(1)
                .and_then(|m| m.parse::<usize>().ok())
                .and_then(|m| MONTHS.get(m.wrapping_sub(1)).copied())
                .unwrap_or("")
                .to_string();
            FundsMovementPoint {
                month,
                period,
                tvl,
                payouts,
            }
        })
        .collect()
}

/// Build the full ranked leaderboard, then slice the requested page.
/// `page`/`page_size` are already resolved (>= 1). Out-of-range page clamps to last.
pub fn leaderboard(bounties: &[EnrichedBounty], page: i64, page_size: i64) -> LeaderboardPage {
    struct Agg {
        earned: Decimal,
        merged_prs: i64,
        assigned: i64,
        repos: std::collections::BTreeSet<String>,
    }
    let mut by_name: std::collections::HashMap<String, Agg> = std::collections::HashMap::new();

    for eb in bounties {
        let Some(name) = eb.contributor.clone() else {
            continue;
        };
        let agg = by_name.entry(name).or_insert_with(|| Agg {
            earned: Decimal::ZERO,
            merged_prs: 0,
            assigned: 0,
            repos: std::collections::BTreeSet::new(),
        });
        if eb.assigned_at.is_some() {
            agg.assigned += 1;
        }
        if eb.paid_at.is_some() {
            agg.merged_prs += 1;
            agg.earned += eb.reward_amount.unwrap_or(Decimal::ZERO);
            agg.repos.insert(eb.repo_slug.clone());
        }
    }

    let mut entries: Vec<LeaderboardEntry> = by_name
        .into_iter()
        .map(|(name, agg)| LeaderboardEntry {
            rank: 0,
            name,
            earned_usdc: agg.earned,
            merged_prs: agg.merged_prs,
            assigned: agg.assigned,
            repos: agg.repos.into_iter().collect(),
        })
        .collect();

    // earnedUsdc desc, mergedPrs desc, name asc — same as the old client-side rule.
    entries.sort_by(|a, b| {
        b.earned_usdc
            .cmp(&a.earned_usdc)
            .then(b.merged_prs.cmp(&a.merged_prs))
            .then(a.name.cmp(&b.name))
    });
    for (i, entry) in entries.iter_mut().enumerate() {
        entry.rank = i as i64 + 1;
    }

    let total = entries.len() as i64;
    let total_pages = ((total + page_size - 1) / page_size).max(1);
    let page = page.clamp(1, total_pages);
    let start = ((page - 1) * page_size) as usize;
    let items = entries
        .into_iter()
        .skip(start)
        .take(page_size as usize)
        .collect();

    LeaderboardPage {
        items,
        page,
        page_size,
        total,
        total_pages,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[allow(clippy::too_many_arguments)]
    fn bounty(
        slug: &str,
        issue: i32,
        reward: Option<i64>,
        status: &str,
        contributor: Option<&str>,
        assigned: bool,
        milestone: bool,
        paid: bool,
    ) -> EnrichedBounty {
        let now = jiff::Timestamp::now();
        EnrichedBounty {
            id: Uuid::new_v4(),
            repo_slug: slug.to_string(),
            issue_number: issue,
            issue_title: Some(format!("issue {issue}")),
            reward_amount: reward.map(Decimal::from),
            status: status.to_string(),
            contributor: contributor.map(str::to_string),
            milestone_index: milestone.then_some(0),
            created_at: now,
            assigned_at: assigned.then(jiff::Timestamp::now),
            paid_at: paid.then(|| now),
            payout_tx_hash: paid.then(|| "abc123".to_string()),
        }
    }

    #[test]
    fn feed_summary_and_leaderboard_derive_correctly() {
        let data = vec![
            // paid bounty: rewarded + assigned + released; earns 220 for gaearon
            bounty(
                "o/contracts",
                9,
                Some(220),
                "paid",
                Some("gaearon"),
                true,
                true,
                true,
            ),
            // locked but unpaid: rewarded + locked + assigned for octocat
            bounty(
                "o/web",
                3,
                Some(180),
                "assigned",
                Some("octocat"),
                true,
                true,
                false,
            ),
            // open rewarded, no contributor
            bounty("o/web", 4, Some(50), "open", None, false, false, false),
        ];

        let feed = activity_feed(&data);
        // rewarded x3, locked x2, assigned x2, released x1 = 8 events
        assert_eq!(feed.len(), 8);
        // newest first
        assert!(feed.windows(2).all(|w| w[0].at >= w[1].at));

        // released rows carry the payout tx hash; non-released rows don't.
        let released = feed
            .iter()
            .find(|r| matches!(r.kind, ActivityKind::Released))
            .unwrap();
        assert_eq!(released.tx_hash.as_deref(), Some("abc123"));
        assert!(feed
            .iter()
            .filter(|r| !matches!(r.kind, ActivityKind::Released))
            .all(|r| r.tx_hash.is_none()));

        let s = summary(&feed);
        assert_eq!(s.issues_rewarded, 3);
        assert_eq!(s.funds_released, Decimal::from(220));
        assert_eq!(s.funds_locked, Decimal::from(220 + 180));
        assert_eq!(s.contributors, 2);

        let lb = leaderboard(&data, 1, 20);
        assert_eq!(lb.total, 2);
        assert_eq!(lb.items[0].name, "gaearon"); // highest earned
        assert_eq!(lb.items[0].rank, 1);
        assert_eq!(lb.items[0].earned_usdc, Decimal::from(220));
        assert_eq!(lb.items[0].merged_prs, 1);
        assert_eq!(lb.items[1].name, "octocat");
        assert_eq!(lb.items[1].earned_usdc, Decimal::ZERO);

        // empty page clamps, totals sane
        let fm = funds_movement(&feed);
        assert!(!fm.is_empty());
        let last = fm.last().unwrap();
        assert_eq!(last.tvl, Decimal::from(400));
        assert_eq!(last.payouts, Decimal::from(220));
    }
}
