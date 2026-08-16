use tracing::info;

use crate::error::AppError;

/// SQL migrations from `toasty/` (history.toml + migrations/*.sql), baked into the binary.
static MIGRATIONS: toasty::migration::MigrationSet = toasty::embed_migrations!("toasty");

/// Normalize common Postgres URL schemes to what Toasty accepts.
///
/// Accepts `postgresql://`, `postgres://`, and also `postgresql+…` / `postgres+…`
/// driver-style URLs (e.g. from some ORMs) by rewriting them to `postgresql://`.
pub fn normalize_database_url(url: &str) -> String {
    let trimmed = url.trim();
    if let Some(rest) = trimmed.strip_prefix("postgres://") {
        format!("postgresql://{rest}")
    } else if let Some(rest) = trimmed.strip_prefix("postgresql+") {
        // postgresql+asyncpg://… → postgresql://…
        if let Some((_, after_scheme)) = rest.split_once("://") {
            format!("postgresql://{after_scheme}")
        } else {
            trimmed.to_string()
        }
    } else if let Some(rest) = trimmed.strip_prefix("postgres+") {
        if let Some((_, after_scheme)) = rest.split_once("://") {
            format!("postgresql://{after_scheme}")
        } else {
            trimmed.to_string()
        }
    } else {
        trimmed.to_string()
    }
}

pub async fn connect(database_url: &str) -> Result<toasty::Db, AppError> {
    let url = normalize_database_url(database_url);
    toasty::Db::builder()
        .models(toasty::models!(
            crate::shared::models::schema::Repo,
            crate::shared::models::schema::Contributor,
            crate::shared::models::schema::Issue,
            crate::shared::models::schema::Assignment,
        ))
        .connect(&url)
        .await
        .map_err(|error| AppError::database(error.to_string()))
}

/// Apply pending migrations from the embedded `toasty/` set.
///
/// Safe to call on every deploy/startup: already-applied IDs are skipped.
pub async fn apply_migrations(db: &toasty::Db) -> Result<(), AppError> {
    let report = MIGRATIONS
        .apply(db)
        .await
        .map_err(|error| AppError::database(error.to_string()))?;

    info!(
        applied = report.applied(),
        skipped = report.skipped(),
        "database migrations ready"
    );
    Ok(())
}
