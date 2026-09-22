//! Toasty ORM models — source of truth for the application schema.
//!
//! These structs drive Toasty query APIs and (via `push_schema`) database schema.
//! Runtime API/cache DTOs live in [`super::entities`] and are mapped from these models.
//!
//! Docs: <https://tokio-rs.github.io/toasty/nightly/guide/>

mod assignment;
mod contributor;
// mod issue;
mod notification;
mod profile;
mod repo;
mod rewards;
mod wallet;
mod repo_maintainers;

pub use assignment::Assignment;
pub use contributor::Contributor;
// pub use issue::Issue;
pub use notification::Notification;
pub use profile::Profile;
pub use repo::Repositories;
pub use rewards::Reward;
pub use wallet::Wallet;
