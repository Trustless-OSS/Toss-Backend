use std::net::SocketAddr;

use tokio::net::TcpListener;
use tracing::info;
use utoipa::OpenApi;
use utoipa_swagger_ui::SwaggerUi;

use toss_backend::{
    app, config::Config, dev, docs::openapi::ApiDoc, infra, lifecycle, state::AppState, telemetry,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = Config::from_env().expect("Failed to load Config !!");
    telemetry::init(&config);
    info!(env = %config.node_env, port = config.port, "configuration loaded");

    let address = SocketAddr::from(([0, 0, 0, 0], config.port));
    let state = AppState::new(config).await?;

    let workers = infra::queue::start_workers(state.clone()).await?;
    info!("background workers started");

    let listener = TcpListener::bind(address).await?;
    dev::webhook_proxy::start_if_enabled(state.clone());

    info!(
        "Trustless-OSS backend module listening on http://{}",
        address
    );

    let app = app::build_app(state.clone())
        .merge(SwaggerUi::new("/swagger").url("/api/v1/doc/openapi.json", ApiDoc::openapi()));

    axum::serve(listener, app)
        .with_graceful_shutdown(lifecycle::shutdown_signal())
        .await?;

    workers.shutdown().await;
    state.queue.close().await;

    Ok(())
}
