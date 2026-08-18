use axum::{
    http::{
        header::{ACCEPT, AUTHORIZATION, CONTENT_TYPE},
        HeaderValue, Method,
    },
    routing::get,
    Json, Router,
};
use serde::Serialize;
use tower_http::{cors::CorsLayer, trace::TraceLayer};
use utoipa::ToSchema;

use crate::{config::parse_cors_allowed_origins, routes, state::AppState};

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
    let raw_allowed_origins = std::env::var("CORS_ALLOWED_ORIGINS")
        .unwrap_or_else(|_| std::env::var("APP_URL").unwrap_or_default());
    let allowed_origins: Vec<HeaderValue> = parse_cors_allowed_origins(&raw_allowed_origins)
        .iter()
        .filter_map(|origin| origin.parse().ok())
        .collect();

    let cors = CorsLayer::new()
        .allow_origin(allowed_origins)
        .allow_methods([
            Method::GET,
            Method::POST,
            Method::PUT,
            Method::PATCH,
            Method::DELETE,
            Method::OPTIONS,
        ])
        .allow_headers([CONTENT_TYPE, AUTHORIZATION, ACCEPT])
        .allow_credentials(true);

    let router: Router<AppState> = Router::new()
        .route("/", get(root_handler))
        .merge(routes::router())
        .layer(cors)
        .layer(TraceLayer::new_for_http());

    router.with_state(state)
}
