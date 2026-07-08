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

    let address = SocketAddr::from(([0, 0, 0, 0], config.port));
    let state = AppState::new(config)?;
    infra::queue::start_workers(state.clone()).await;
    infra::queue::start_scheduler(state.clone());

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
