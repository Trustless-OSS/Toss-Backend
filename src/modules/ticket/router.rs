use axum::{
    routing::{get, post},
    Router,
};

use crate::state::AppState;

use super::handler;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/v1/tickets", post(handler::create_ticket))
        .route(
            "/api/v1/tickets/{ticketId}",
            get(handler::get_ticket).patch(handler::patch_ticket),
        )
        .route(
            "/api/v1/tickets/{ticketId}/resolve",
            post(handler::resolve_ticket),
        )
}
