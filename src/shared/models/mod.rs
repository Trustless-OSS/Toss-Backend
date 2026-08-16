//! Shared data models used across modules.
//!
//! - [`entities`] — serde DTOs / cache types mapped from Toasty schema models
//! - [`domain`] — enums and composite types used in business logic / API responses
//! - [`schema`] — Toasty ORM models (schema + generated query APIs)
//!
//! Module-specific request/response types stay in each module's `model.rs`.

pub mod domain;
pub mod entities;
pub mod schema;

pub use domain::{Difficulty, ParsedLabels};
pub use entities::{Assignment, Contributor, Issue, Repo};
