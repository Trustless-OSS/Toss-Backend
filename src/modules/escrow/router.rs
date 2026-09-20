use axum::{routing::post, Router};

use crate::{modules::escrow::handler, state::AppState};

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/api/v1/escrow/create-unsigned",
            post(handler::create_escrow_unsigned),
        )
        .route("/api/v1/escrow/submit-deploy", post(handler::submit_deploy))
        .route("/api/v1/escrow/fund-unsigned", post(handler::fund_unsigned))
        .route("/api/v1/escrow/submit-fund", post(handler::submit_fund))
        .route("/api/v1/escrow/refund", post(handler::refund))
        .route(
            "/api/v1/escrow/close-unsigned",
            post(handler::close_unsigned),
        )
        .route("/api/v1/escrow/submit-close", post(handler::submit_close))
}
