//! Activity ledger and contributor leaderboard module.
//!
//! Exposes maintainer-scoped endpoints:
//! - `GET /api/activity`: paginated activity ledger with `kind` and optional `repo` filters
//! - `GET /api/contributors/leaderboard`: ranked contributor leaderboard across caller's repos

pub mod dto;
pub mod handlers;
pub mod repository;
pub mod routes;
pub mod service;
