use axum::{routing::post, Router};

use crate::{modules::escrow::handler, state::AppState};

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/api/escrow/create-unsigned",
            post(handler::create_escrow_unsigned),
        )
        .route("/api/escrow/submit-deploy", post(handler::submit_deploy))
        .route("/api/escrow/fund-unsigned", post(handler::fund_unsigned))
        .route("/api/escrow/submit-fund", post(handler::submit_fund))
        .route("/api/escrow/refund", post(handler::refund))
        .route("/api/escrow/close-unsigned", post(handler::close_unsigned))
        .route("/api/escrow/submit-close", post(handler::submit_close))
}
