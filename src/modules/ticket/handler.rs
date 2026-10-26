use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use uuid::Uuid;

use crate::{
    error::{AppError, ErrorResponse},
    middleware::auth::AuthedUser,
    state::AppState,
};

use super::{
    dtype::{CreateTicketRequest, PatchTicketRequest, TicketResponse},
    service,
};

#[utoipa::path(
    post,
    path = "/api/v1/tickets",
    tag = "Tickets",
    security(("bearer_auth" = [])),
    request_body = CreateTicketRequest,
    responses(
        (status = 201, description = "Ticket created", body = TicketResponse),
        (status = 400, description = "Invalid ticket data or missing GitHub username", body = ErrorResponse),
        (status = 401, description = "Missing or invalid bearer token", body = ErrorResponse),
        (status = 500, description = "Failed to create ticket", body = ErrorResponse)
    )
)]
pub async fn create_ticket(
    state: State<AppState>,
    user: AuthedUser,
    body: Json<CreateTicketRequest>,
) -> Result<(StatusCode, Json<TicketResponse>), AppError> {
    service::create_ticket(state, user, body)
        .await
        .map(|ticket| (StatusCode::CREATED, ticket))
}

#[utoipa::path(
    get,
    path = "/api/v1/tickets/{ticketId}",
    tag = "Tickets",
    security(("bearer_auth" = [])),
    params(("ticketId" = Uuid, Path, description = "Ticket UUID")),
    responses(
        (status = 200, description = "Ticket details", body = TicketResponse),
        (status = 401, description = "Missing or invalid bearer token", body = ErrorResponse),
        (status = 404, description = "Ticket or contributor profile not found", body = ErrorResponse),
        (status = 500, description = "Failed to retrieve ticket", body = ErrorResponse)
    )
)]
pub async fn get_ticket(
    state: State<AppState>,
    user: AuthedUser,
    ticket_id: Path<Uuid>,
) -> Result<Json<TicketResponse>, AppError> {
    service::get_ticket(state, user, ticket_id).await
}

#[utoipa::path(
    patch,
    path = "/api/v1/tickets/{ticketId}",
    tag = "Tickets",
    security(("bearer_auth" = [])),
    params(("ticketId" = Uuid, Path, description = "Ticket UUID")),
    request_body = PatchTicketRequest,
    responses(
        (status = 200, description = "Ticket updated", body = TicketResponse),
        (status = 400, description = "Invalid ticket data or ticket is resolved", body = ErrorResponse),
        (status = 401, description = "Missing or invalid bearer token", body = ErrorResponse),
        (status = 404, description = "Ticket or contributor profile not found", body = ErrorResponse),
        (status = 500, description = "Failed to update ticket", body = ErrorResponse)
    )
)]
pub async fn patch_ticket(
    state: State<AppState>,
    user: AuthedUser,
    ticket_id: Path<Uuid>,
    body: Json<PatchTicketRequest>,
) -> Result<Json<TicketResponse>, AppError> {
    service::patch_ticket(state, user, ticket_id, body).await
}

#[utoipa::path(
    post,
    path = "/api/v1/tickets/{ticketId}/resolve",
    tag = "Tickets",
    security(("bearer_auth" = [])),
    params(("ticketId" = Uuid, Path, description = "Ticket UUID")),
    responses(
        (status = 200, description = "Ticket marked resolved", body = TicketResponse),
        (status = 401, description = "Missing or invalid bearer token", body = ErrorResponse),
        (status = 404, description = "Ticket or contributor profile not found", body = ErrorResponse),
        (status = 500, description = "Failed to resolve ticket", body = ErrorResponse)
    )
)]
pub async fn resolve_ticket(
    state: State<AppState>,
    user: AuthedUser,
    ticket_id: Path<Uuid>,
) -> Result<Json<TicketResponse>, AppError> {
    service::resolve_ticket(state, user, ticket_id).await
}
