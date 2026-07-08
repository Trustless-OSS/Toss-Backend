use sqlx::{postgres::PgPoolOptions, PgPool};

use crate::error::AppError;

pub fn connect_lazy(database_url: &str) -> Result<PgPool, AppError> {
    PgPoolOptions::new()
        .connect_lazy(database_url)
        .map_err(|error| AppError::database(error.to_string()))
}
