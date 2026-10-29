//! Dashboard & leaderboard module.
//!
//! Powers the maintainer dashboard (activity feed, funds-movement time series,
//! summary metrics, maintainer-scoped leaderboard) and the public, cached,
//! platform-wide leaderboard.
//!
//! The activity feed and all aggregates are **derived from `Bounty` rows**
//! (status + the `assigned_at` / `merged_at` / `paid_at` / `created_at`
//! timestamps) joined with `Profile` (contributor username) and `Repository`
//! (the `owner/name` slug). The `Activity` and `EscrowFunder` tables exist in
//! the schema but are not yet populated by the write path, so we reconstruct
//! events from the authoritative bounty state instead.

pub mod handlers;
pub mod model;
pub mod repository;
pub mod routes;
pub mod service;
