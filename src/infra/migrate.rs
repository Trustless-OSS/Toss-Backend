use tracing::info;
use crate::error::AppError;
static EMBEDDED_MIGRATIONS: toasty::migration::MigrationSet = toasty::embed_migrations!("toasty");

pub async fn run_pending(db: &toasty::Db) -> Result<(), AppError> {
    info!("applying pending database migrations");

    let report = EMBEDDED_MIGRATIONS
        .apply(db)
        .await
        .map_err(|error| AppError::database(format!("failed to apply migrations: {error}")))?;

    info!(
        applied = report.applied(),
        skipped = report.skipped(),
        "database migrations applied"
    );

    Ok(())
}
