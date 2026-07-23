use axum::{routing::get, Json, Router};
use serde::Serialize;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;

use crate::{routes, state::AppState};

#[derive(Serialize)]
struct RootResponse {
    service: &'static str,
    status: &'static str,
    message: &'static str,
}

async fn root_handler() -> Json<RootResponse> {
    Json(RootResponse {
        service: "trustless-oss-backend",
        status: "ok",
        message: "Trustless-OSS Rust backend is running.",
    })
}

pub fn build_app(state: AppState) -> Router {
    // Configure CORS to allow frontend requests
    let cors = CorsLayer::permissive();

    let router: Router<AppState> = Router::new()
        .route("/", get(root_handler))
        .merge(routes::router())
        .layer(cors)
        .layer(TraceLayer::new_for_http());

    router.with_state(state)
}
