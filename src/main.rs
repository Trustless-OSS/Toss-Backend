use std::net::SocketAddr;

use tokio::net::TcpListener;
use tracing::info;

use toss_backend::{
    app, config::Config, dev, infra, lifecycle, run_migrations, state::AppState, telemetry,
};

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

    let shutdown_state = state.clone();
    let shutdown = async move {
        lifecycle::shutdown_signal().await;
        lifecycle::begin_shutdown();
        // Stop claiming new webhook jobs immediately; already-claimed jobs
        // keep running and are drained below once HTTP has stopped.
        shutdown_state.queue.begin_shutdown();
    };

    axum::serve(listener, app::build_app(state.clone()))
        .with_graceful_shutdown(shutdown)
        .await?;

    info!("HTTP layer drained; waiting for in-flight webhook jobs");
    state
        .queue
        .wait_for_idle(std::time::Duration::from_secs(30))
        .await;

    Ok(())
}
