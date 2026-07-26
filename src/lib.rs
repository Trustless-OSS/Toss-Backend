#![allow(dead_code)]

pub mod app;
pub mod config;
pub mod dev;
pub mod error;
pub mod infra;
pub mod lifecycle;
pub mod middleware;
pub mod modules;
pub mod routes;
pub mod schema;
pub mod shared;
pub mod state;
pub mod telemetry;

use diesel_async::async_connection_wrapper::AsyncConnectionWrapper;
use diesel_async::{AsyncConnection, AsyncPgConnection};
use diesel_migrations::{embed_migrations, EmbeddedMigrations, MigrationHarness};

pub const MIGRATIONS: EmbeddedMigrations = embed_migrations!("./migrations");

/// Run pending Diesel migrations. `diesel_migrations` is synchronous, so we
/// wrap the async connection with `AsyncConnectionWrapper` and drive it on a
/// blocking task.
pub async fn run_migrations(database_url: &str) -> Result<(), Box<dyn std::error::Error>> {
    let async_connection = AsyncPgConnection::establish(database_url).await?;
    let mut wrapper: AsyncConnectionWrapper<AsyncPgConnection> = async_connection.into();

    tokio::task::spawn_blocking(move || {
        wrapper
            .run_pending_migrations(MIGRATIONS)
            .map(|_| ())
            .map_err(|error| error.to_string())
    })
    .await??;

    Ok(())
}
