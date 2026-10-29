use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::shared::models::schema;

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateTicketRequest {
    pub org: Option<String>,
    pub category: String,
    pub subject: String,
    pub message: String,
    pub attachment_key: Option<String>,
    pub bounty_id: Option<Uuid>,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PatchTicketRequest {
    pub org: Option<String>,
    pub category: Option<String>,
    pub subject: Option<String>,
    pub message: Option<String>,
    pub attachment_key: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct TicketResponse {
    pub id: Uuid,
    pub profile_id: Uuid,
    pub org: Option<String>,
    pub category: String,
    pub subject: String,
    pub message: String,
    pub attachment_key: Option<String>,
    pub bounty_id: Option<Uuid>,
    pub status: String,
    pub created_at: String,
    pub updated_at: String,
}

impl From<schema::Ticket> for TicketResponse {
    fn from(ticket: schema::Ticket) -> Self {
        Self {
            id: ticket.id,
            profile_id: ticket.profile_id,
            org: ticket.org,
            category: ticket.category,
            subject: ticket.subject,
            message: ticket.message,
            attachment_key: ticket.attachment_key,
            bounty_id: ticket.bounty_id,
            status: ticket.status,
            created_at: ticket.created_at.to_string(),
            updated_at: ticket.updated_at.to_string(),
        }
    }
}
