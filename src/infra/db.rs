use crate::error::AppError;

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
    let db = toasty::Db::builder()
        .models(toasty::models!(
            crate::shared::models::schema::Repository,
            crate::shared::models::schema::Profile,
            crate::shared::models::schema::Wallet,
            crate::shared::models::schema::RepoMaintainer,
            crate::shared::models::schema::Reward,
            crate::shared::models::schema::Bounty,
            crate::shared::models::schema::EscrowFunder,
            crate::shared::models::schema::Notification,
            crate::shared::models::schema::Activity,
            crate::shared::models::schema::FailedTask,
        ))
        .connect(&url)
        .await
        .map_err(|error| AppError::database(error.to_string()))?;

    crate::infra::migrate::run_pending(&db).await?;

    Ok(db)
}
