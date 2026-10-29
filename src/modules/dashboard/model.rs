//! Dashboard & leaderboard response DTOs. Field names are the frontend contract
//! (camelCase) from `docs/DASHBOARD_BACKEND_REQUIREMENTS.md`.

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// One activity event. Derived from a `Bounty` row; see `service::activity_feed`.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ActivityRow {
    /// Stable unique id for the event (used as a React key).
    pub id: String,
    /// When the event happened (ISO 8601 UTC).
    pub at: DateTime<Utc>,
    /// Full repo slug `owner/name`.
    pub repo: String,
    /// GitHub issue number. `null` for pool/deposit events not tied to an issue.
    pub issue_number: Option<i32>,
    /// Issue title. `null` when `issueNumber` is `null`.
    pub issue_title: Option<String>,
    /// One of: rewarded, locked, released, assigned, unassigned, rejected.
    pub kind: ActivityKind,
    /// USDC amount (plain number). `null` for assigned/unassigned/rejected.
    #[schema(value_type = Option<String>)]
    #[serde(with = "rust_decimal::serde::str_option")]
    pub amount_usdc: Option<Decimal>,
    /// Contributor GitHub username. `null` when not tied to a person.
    pub contributor: Option<String>,
    /// Stellar transaction hash (hex). `null` when GitHub-only.
    pub tx_hash: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum ActivityKind {
    Rewarded,
    Locked,
    Released,
    Assigned,
    Unassigned,
    Rejected,
}

/// Monthly point for the funds-movement area chart. Chronological.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct FundsMovementPoint {
    /// Short month label, e.g. `Jul`.
    pub month: String,
    /// Sortable period, e.g. `2026-07` (avoids year ambiguity).
    pub period: String,
    /// Cumulative escrow TVL locked (USD) at that month.
    #[schema(value_type = String)]
    #[serde(with = "rust_decimal::serde::str")]
    pub tvl: Decimal,
    /// Cumulative payouts released (USD) at that month.
    #[schema(value_type = String)]
    #[serde(with = "rust_decimal::serde::str")]
    pub payouts: Decimal,
}

/// Maintainer dashboard summary metrics (section 1 aggregates, server-side).
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DashboardSummary {
    #[schema(value_type = String)]
    #[serde(with = "rust_decimal::serde::str")]
    pub funds_locked: Decimal,
    #[schema(value_type = String)]
    #[serde(with = "rust_decimal::serde::str")]
    pub funds_released: Decimal,
    pub issues_rewarded: i64,
    pub contributors: i64,
}

/// One leaderboard entry.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct LeaderboardEntry {
    /// Global 1-based rank across the whole board (not per-page).
    pub rank: i64,
    /// Contributor GitHub username.
    pub name: String,
    /// Total USDC earned (sum of released payouts). Primary sort key.
    #[schema(value_type = String)]
    #[serde(with = "rust_decimal::serde::str")]
    pub earned_usdc: Decimal,
    /// Total merged PRs (count of released payouts). Secondary sort key.
    pub merged_prs: i64,
    /// Total issues assigned.
    pub assigned: i64,
    /// Distinct repo slugs the contributor has earned in.
    pub repos: Vec<String>,
}

/// Paginated envelope matching the frontend's `{ items, page, pageSize, total, totalPages }`.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct LeaderboardPage {
    pub items: Vec<LeaderboardEntry>,
    pub page: i64,
    pub page_size: i64,
    pub total: i64,
    pub total_pages: i64,
}
