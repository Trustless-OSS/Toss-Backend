use axum::{
    routing::{get, post},
    Router,
};

use crate::state::AppState;

use super::handlers::*;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/v1/notifications", get(list_notifications))
        .route("/api/v1/notifications/unread-count", get(unread_count))
        .route("/api/v1/notifications/read-all", post(read_all))
        .route("/api/v1/notifications/:id/read", post(read_one))
}
