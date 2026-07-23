#![allow(dead_code)]

mod app;
mod config;
mod dev;
mod error;
mod infra;
mod lifecycle;
mod middleware;
mod modules;
mod routes;
mod schema;
mod shared;
mod state;
mod telemetry;

use std::net::SocketAddr;

use diesel_async::async_connection_wrapper::AsyncConnectionWrapper;
use diesel_async::{AsyncConnection, AsyncPgConnection};
use diesel_migrations::{embed_migrations, EmbeddedMigrations, MigrationHarness};
use tokio::net::TcpListener;
use tracing::info;

use crate::{config::Config, state::AppState};

pub const MIGRATIONS: EmbeddedMigrations = embed_migrations!("./migrations");

/// Run pending Diesel migrations. `diesel_migrations` is synchronous, so we
/// wrap the async connection with `AsyncConnectionWrapper` and drive it on a
/// blocking task.
async fn run_migrations(database_url: &str) -> Result<(), Box<dyn std::error::Error>> {
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

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = Config::from_env().expect("Failed to load Config !!");
    telemetry::init(&config);
    info!(env = %config.node_env, port = config.port, "configuration loaded");

    let address = SocketAddr::from(([0, 0, 0, 0], config.port));
    let database_url = config.database_url.clone();
    let state = AppState::new(config)?;

    // Run database migrations
    if state.db.is_some() {
        info!("running database migrations");
        run_migrations(&database_url)
            .await
            .expect("Failed to run database migrations");
        info!("database migrations completed");
    }

    infra::queue::start_workers(state.clone()).await;
    infra::queue::start_scheduler(state.clone());
    info!("background workers started");

    let listener = TcpListener::bind(address).await?;
    dev::webhook_proxy::start_if_enabled(state.clone());

    info!(
        "Trustless-OSS backend module listening on http://{}",
        address
    );

    axum::serve(listener, app::build_app(state))
        .with_graceful_shutdown(lifecycle::shutdown_signal())
        .await?;

    Ok(())
}
