//! Toasty ORM models — source of truth for the application schema.
//!
//! These structs drive Toasty query APIs and (via `push_schema`) database schema.
//! Runtime API/cache DTOs live in [`super::entities`] and are mapped from these models.
//!
//! Docs: <https://tokio-rs.github.io/toasty/nightly/guide/>

mod assignment;
mod contributor;
mod issue;
mod repo;
mod rewards;

pub use assignment::Assignment;
pub use contributor::Contributor;
pub use issue::Issue;
pub use repo::Repo;
pub use rewards::Reward;
