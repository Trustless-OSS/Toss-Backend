use axum::Router;

use crate::{
    modules::{bounty, contributor, escrow, github, notification, repo},
    state::AppState,
};

pub mod health;
pub mod queue;

pub fn router() -> Router<AppState> {
    Router::new()
        .merge(health::router())
        .merge(queue::router())
        .merge(github::routes::router())
        .merge(repo::routes::router())
        .merge(escrow::router::router())
        .merge(bounty::routes::router())
        .merge(contributor::routes::router())
        .merge(notification::routes::router())
}
