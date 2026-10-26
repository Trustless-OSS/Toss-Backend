use uuid::Uuid;

use crate::{
    error::{map_db_err, require_db, AppError},
    shared::models::schema,
    state::AppState,
};

use super::dtype::{CreateTicketRequest, PatchTicketRequest, TicketResponse};

pub async fn create(
    state: &AppState,
    profile_id: Uuid,
    ticket: CreateTicketRequest,
) -> Result<TicketResponse, AppError> {
    let mut db = require_db(&state.db)?;
    let ticket = toasty::create!(schema::Ticket {
        profile_id,
        org: ticket.org,
        category: ticket.category,
        subject: ticket.subject,
        message: ticket.message,
        attachment_key: ticket.attachment_key,
        bounty_id: ticket.bounty_id,
    })
    .exec(&mut db)
    .await
    .map_err(map_db_err)?;

    Ok(ticket.into())
}

pub async fn get(
    state: &AppState,
    profile_id: Uuid,
    ticket_id: Uuid,
) -> Result<Option<TicketResponse>, AppError> {
    let mut db = require_db(&state.db)?;
    let ticket = schema::Ticket::filter_by_id(ticket_id)
        .filter_by_profile_id(profile_id)
        .first()
        .exec(&mut db)
        .await
        .map_err(map_db_err)?;

    Ok(ticket.map(TicketResponse::from))
}

pub async fn patch(
    state: &AppState,
    profile_id: Uuid,
    ticket_id: Uuid,
    patch: PatchTicketRequest,
) -> Result<Option<TicketResponse>, AppError> {
    let mut db = require_db(&state.db)?;
    let Some(ticket) = schema::Ticket::filter_by_id(ticket_id)
        .filter_by_profile_id(profile_id)
        .first()
        .exec(&mut db)
        .await
        .map_err(map_db_err)?
    else {
        return Ok(None);
    };

    if ticket.status != "open" {
        return Err(AppError::bad_request("Only open tickets can be updated"));
    }

    toasty::update!(schema::Ticket::filter_by_id(ticket_id).filter_by_profile_id(profile_id).filter_by_status("open") {
        org: patch.org.or(ticket.org),
        category: patch.category.unwrap_or(ticket.category),
        subject: patch.subject.unwrap_or(ticket.subject),
        message: patch.message.unwrap_or(ticket.message),
        attachment_key: patch.attachment_key.or(ticket.attachment_key),
        updated_at: jiff::Timestamp::now(),
    })
    .exec(&mut db)
    .await
    .map_err(map_db_err)?;

    let updated = schema::Ticket::filter_by_id(ticket_id)
        .filter_by_profile_id(profile_id)
        .first()
        .exec(&mut db)
        .await
        .map_err(map_db_err)?
        .ok_or_else(|| AppError::database("ticket missing after update"))?;

    if updated.status != "open" {
        return Err(AppError::bad_request("Only open tickets can be updated"));
    }

    Ok(Some(updated.into()))
}

pub async fn resolve(
    state: &AppState,
    profile_id: Uuid,
    ticket_id: Uuid,
) -> Result<Option<TicketResponse>, AppError> {
    let mut db = require_db(&state.db)?;
    let Some(ticket) = schema::Ticket::filter_by_id(ticket_id)
        .filter_by_profile_id(profile_id)
        .first()
        .exec(&mut db)
        .await
        .map_err(map_db_err)?
    else {
        return Ok(None);
    };

    match ticket.status.as_str() {
        "resolved" => {}
        "open" => {
            toasty::update!(schema::Ticket::filter_by_id(ticket_id).filter_by_profile_id(profile_id).filter_by_status("open") {
            status: "resolved".to_string(),
            updated_at: jiff::Timestamp::now(),
        })
        .exec(&mut db)
        .await
        .map_err(map_db_err)?;
        }
        _ => {
            return Err(AppError::bad_request(
                "Ticket cannot be resolved from its current status",
            ));
        }
    }

    let resolved = schema::Ticket::filter_by_id(ticket_id)
        .filter_by_profile_id(profile_id)
        .first()
        .exec(&mut db)
        .await
        .map_err(map_db_err)?
        .ok_or_else(|| AppError::database("ticket missing after resolve"))?;

    Ok(Some(resolved.into()))
}
