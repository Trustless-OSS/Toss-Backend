use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};

use crate::error::AppError;

pub const DEFAULT_PAGE: i64 = 1;
pub const DEFAULT_ACTIVITY_LIMIT: i64 = 20;
pub const DEFAULT_LEADERBOARD_LIMIT: i64 = 50;
pub const MAX_PAGE_LIMIT: i64 = 100;

/// Supported activity ledger event kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum ActivityEventKind {
    Rewarded,
    Locked,
    Released,
    Assigned,
    Unassigned,
    Rejected,
}

impl ActivityEventKind {
    pub fn parse_exact(raw: &str) -> Option<Self> {
        match raw {
            "rewarded" => Some(Self::Rewarded),
            "locked" => Some(Self::Locked),
            "released" => Some(Self::Released),
            "assigned" => Some(Self::Assigned),
            "unassigned" => Some(Self::Unassigned),
            "rejected" => Some(Self::Rejected),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Rewarded => "rewarded",
            Self::Locked => "locked",
            Self::Released => "released",
            Self::Assigned => "assigned",
            Self::Unassigned => "unassigned",
            Self::Rejected => "rejected",
        }
    }
}

/// Validated `kind` filter for `GET /api/activity`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActivityKindFilter {
    All,
    Kind(ActivityEventKind),
}

impl ActivityKindFilter {
    pub fn parse_exact(raw: &str) -> Result<Self, AppError> {
        if raw == "all" {
            return Ok(Self::All);
        }
        ActivityEventKind::parse_exact(raw)
            .map(Self::Kind)
            .ok_or_else(|| {
                AppError::bad_request(
                    "Invalid kind. Expected one of: all, rewarded, locked, released, assigned, unassigned, rejected",
                )
            })
    }

    pub fn matches(self, kind: ActivityEventKind) -> bool {
        match self {
            Self::All => true,
            Self::Kind(expected) => expected == kind,
        }
    }
}

/// Query parameters for `GET /api/activity`.
#[derive(Debug, Clone, Default, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct ActivityQuery {
    /// Event kind filter: all | rewarded | locked | released | assigned | unassigned | rejected. Defaults to `all`.
    pub kind: Option<String>,
    /// Optional repository filter in `owner/name` format.
    pub repo: Option<String>,
    /// 1-based page number. Defaults to 1.
    pub page: Option<i64>,
    /// Page size. Defaults to 20, max 100.
    pub limit: Option<i64>,
}

/// Validated query parameters for `GET /api/activity`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidatedActivityQuery {
    pub kind: ActivityKindFilter,
    pub repo: Option<String>,
    pub page: i64,
    pub limit: i64,
}

impl ActivityQuery {
    pub fn validate(self) -> Result<ValidatedActivityQuery, AppError> {
        let kind = match self.kind {
            Some(ref raw) => ActivityKindFilter::parse_exact(raw)?,
            None => ActivityKindFilter::All,
        };
        let repo = validate_repo_filter(self.repo)?;
        let (page, limit) = validate_page_limit(self.page, self.limit, DEFAULT_ACTIVITY_LIMIT)?;
        Ok(ValidatedActivityQuery {
            kind,
            repo,
            page,
            limit,
        })
    }
}

/// Query parameters for `GET /api/contributors/leaderboard`.
#[derive(Debug, Clone, Default, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct LeaderboardQuery {
    /// Optional repository filter in `owner/name` format.
    pub repo: Option<String>,
    /// 1-based page number. Defaults to 1.
    pub page: Option<i64>,
    /// Page size. Defaults to 50, max 100.
    pub limit: Option<i64>,
}

/// Validated query parameters for `GET /api/contributors/leaderboard`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidatedLeaderboardQuery {
    pub repo: Option<String>,
    pub page: i64,
    pub limit: i64,
}

impl LeaderboardQuery {
    pub fn validate(self) -> Result<ValidatedLeaderboardQuery, AppError> {
        let repo = validate_repo_filter(self.repo)?;
        let (page, limit) = validate_page_limit(self.page, self.limit, DEFAULT_LEADERBOARD_LIMIT)?;
        Ok(ValidatedLeaderboardQuery { repo, page, limit })
    }
}

fn validate_page_limit(
    page: Option<i64>,
    limit: Option<i64>,
    default_limit: i64,
) -> Result<(i64, i64), AppError> {
    let page = page.unwrap_or(DEFAULT_PAGE);
    if page < 1 {
        return Err(AppError::bad_request("page must be at least 1"));
    }

    let limit = limit.unwrap_or(default_limit);
    if !(1..=MAX_PAGE_LIMIT).contains(&limit) {
        return Err(AppError::bad_request(format!(
            "limit must be between 1 and {MAX_PAGE_LIMIT}"
        )));
    }

    Ok((page, limit))
}

fn validate_repo_filter(repo: Option<String>) -> Result<Option<String>, AppError> {
    let Some(raw) = repo else {
        return Ok(None);
    };
    let trimmed = raw.trim();
    let Some((owner, name)) = trimmed.split_once('/') else {
        return Err(AppError::bad_request("repo must be in owner/name format"));
    };
    if owner.is_empty()
        || name.is_empty()
        || name.contains('/')
        || owner.chars().any(char::is_whitespace)
        || name.chars().any(char::is_whitespace)
    {
        return Err(AppError::bad_request("repo must be in owner/name format"));
    }
    Ok(Some(trimmed.to_string()))
}

/// Single event entry returned by `GET /api/activity`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct ActivityLedgerItem {
    /// Stable unique identifier across pages.
    pub id: String,
    /// When the event occurred (ISO 8601 UTC).
    pub at: DateTime<Utc>,
    /// Repository full name (`owner/name`).
    pub repo: String,
    /// GitHub issue number. `null` for pool deposits not tied to an issue.
    pub issue_number: Option<i32>,
    /// GitHub issue title. `null` when `issue_number` is `null`.
    pub issue_title: Option<String>,
    /// Event kind: rewarded | locked | released | assigned | unassigned | rejected.
    pub kind: ActivityEventKind,
    /// USDC amount as decimal string. `null` when event has no monetary amount.
    #[schema(value_type = Option<String>)]
    #[serde(with = "rust_decimal::serde::str_option")]
    pub amount_usdc: Option<Decimal>,
    /// Contributor GitHub username. `null` when not tied to a contributor.
    pub contributor: Option<String>,
    /// Stellar transaction hash. `null` when no on-chain transaction exists.
    pub tx_hash: Option<String>,
}

/// Single contributor row returned by `GET /api/contributors/leaderboard`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct ContributorLeaderboardEntry {
    /// Global 1-based rank after sorting.
    pub rank: i64,
    /// Contributor GitHub username.
    pub github_username: String,
    /// Total earned USDC across released payouts.
    #[schema(value_type = String)]
    #[serde(with = "rust_decimal::serde::str")]
    pub earned_usdc: Decimal,
    /// Count of released payout events.
    pub merged_prs: i64,
    /// Count of assigned events.
    pub assigned: i64,
    /// Sorted unique `owner/name` repositories the contributor was active in.
    pub repos: Vec<String>,
}

/// Standard pagination envelope for activity ledger and leaderboard endpoints.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct PaginatedLedgerResponse<T: ToSchema> {
    pub data: Vec<T>,
    pub page: i64,
    pub limit: i64,
    pub total: i64,
    pub total_pages: i64,
}
