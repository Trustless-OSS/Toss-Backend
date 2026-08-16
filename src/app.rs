use axum::{routing::get, Json, Router};
use serde::Serialize;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;
use utoipa::ToSchema;

use crate::{routes, state::AppState};

#[derive(Serialize, ToSchema)]
pub(crate) struct RootResponse {
    pub service: &'static str,
    pub status: &'static str,
    pub message: &'static str,
}

#[utoipa::path(
    get,
    path = "/",
    tag = "Root",
    responses(
        (status = 200, description = "Service is running", body = RootResponse)
    )
)]
pub(crate) async fn root_handler() -> Json<RootResponse> {
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
