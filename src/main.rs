#![allow(dead_code)]

mod app;
mod config;
mod error;
mod infra;
mod lifecycle;
mod middleware;
mod modules;
mod routes;
mod shared;
mod state;
mod telemetry;

use std::net::SocketAddr;

use tokio::net::TcpListener;
use tracing::info;

use crate::{config::Config, state::AppState};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = Config::from_env().expect("Failed to load Config !!");
    telemetry::init(&config);
    info!(env = %config.node_env, port = config.port, "configuration loaded");

    let address = SocketAddr::from(([0, 0, 0, 0], config.port));
    let state = AppState::new(config)?;

    // Run database migrations
    if let Some(pool) = state.db.as_ref() {
        info!("running database migrations");
        sqlx::migrate!("./migrations")
            .run(pool)
            .await
            .expect("Failed to run database migrations");
        info!("database migrations completed");
    }

    infra::queue::start_workers(state.clone()).await;
    infra::queue::start_scheduler(state.clone());
    info!("background workers started");

    let listener = TcpListener::bind(address).await?;

    info!(
        "Trustless-OSS backend module listening on http://{}",
        address
    );

    axum::serve(listener, app::build_app(state))
        .with_graceful_shutdown(lifecycle::shutdown_signal())
        .await?;

    Ok(())
}
