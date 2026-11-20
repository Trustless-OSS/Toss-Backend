//! Dashboard & leaderboard module.
//!
//! Powers the maintainer dashboard (activity feed, funds-movement time series,
//! summary metrics, maintainer-scoped leaderboard) and the public, cached,
//! platform-wide leaderboard.
//!
//! The activity feed and all aggregates are **derived from `Bounty` rows**
//! (status + the `assigned_at` / `merged_at` / `paid_at` / `created_at`
//! timestamps) joined with `Profile` (contributor username) and `Repository`
//! (the `owner/name` slug), reconstructing events from authoritative bounty state.

pub mod handlers;
pub mod model;
pub mod repository;
pub mod routes;
pub mod service;
