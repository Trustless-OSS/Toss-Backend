use axum::{extract::State, Json};

use crate::{
    error::AppError,
    middleware::auth::AuthedUser,
    modules::contributor::repository::{ensure_contributor_with_email, get_profile_by_github_id},
    state::AppState,
};

use super::{
    dtype::{CreateTicketRequest, PatchTicketRequest, TicketResponse},
    repository,
};

pub async fn create_ticket(
    State(state): State<AppState>,
    user: AuthedUser,
    Json(mut ticket): Json<CreateTicketRequest>,
) -> Result<Json<TicketResponse>, AppError> {
    ticket.category = validate_required(ticket.category, "category", 100)?;
    ticket.subject = validate_required(ticket.subject, "subject", 200)?;
    ticket.message = validate_required(ticket.message, "message", 10_000)?;
    ticket.org = validate_optional(ticket.org, "org", 200)?;
    ticket.attachment_key = validate_optional(ticket.attachment_key, "attachmentKey", 1_024)?;

    let profile = get_profile_by_github_id(&state, user.github_id).await?;
    let profile_id = match profile {
        Some(profile) => profile.id,
        None => {
            let username = user
                .github_username
                .as_deref()
                .filter(|username| !username.trim().is_empty())
                .ok_or_else(|| {
                    AppError::bad_request("GitHub username is required to create a ticket")
                })?;
            ensure_contributor_with_email(&state, user.github_id, username, user.email.as_deref())
                .await?
                .id
        }
    };

    Ok(Json(repository::create(&state, profile_id, ticket).await?))
}

pub async fn get_ticket(
    State(state): State<AppState>,
    user: AuthedUser,
    ticket_id: axum::extract::Path<uuid::Uuid>,
) -> Result<Json<TicketResponse>, AppError> {
    let profile_id = profile_id(&state, &user).await?;
    let ticket = repository::get(&state, profile_id, ticket_id.0)
        .await?
        .ok_or_else(|| AppError::not_found("Ticket not found"))?;
    Ok(Json(ticket))
}

pub async fn patch_ticket(
    State(state): State<AppState>,
    user: AuthedUser,
    ticket_id: axum::extract::Path<uuid::Uuid>,
    Json(mut patch): Json<PatchTicketRequest>,
) -> Result<Json<TicketResponse>, AppError> {
    if patch.org.is_none()
        && patch.category.is_none()
        && patch.subject.is_none()
        && patch.message.is_none()
        && patch.attachment_key.is_none()
    {
        return Err(AppError::bad_request(
            "At least one ticket field must be provided",
        ));
    }

    patch.category = patch
        .category
        .map(|value| validate_required(value, "category", 100))
        .transpose()?;
    patch.subject = patch
        .subject
        .map(|value| validate_required(value, "subject", 200))
        .transpose()?;
    patch.message = patch
        .message
        .map(|value| validate_required(value, "message", 10_000))
        .transpose()?;
    patch.org = patch
        .org
        .map(|value| validate_required(value, "org", 200))
        .transpose()?;
    patch.attachment_key = patch
        .attachment_key
        .map(|value| validate_required(value, "attachmentKey", 1_024))
        .transpose()?;

    let profile_id = profile_id(&state, &user).await?;
    let ticket = repository::patch(&state, profile_id, ticket_id.0, patch)
        .await?
        .ok_or_else(|| AppError::not_found("Ticket not found"))?;
    Ok(Json(ticket))
}

pub async fn resolve_ticket(
    State(state): State<AppState>,
    user: AuthedUser,
    ticket_id: axum::extract::Path<uuid::Uuid>,
) -> Result<Json<TicketResponse>, AppError> {
    let profile_id = profile_id(&state, &user).await?;
    let ticket = repository::resolve(&state, profile_id, ticket_id.0)
        .await?
        .ok_or_else(|| AppError::not_found("Ticket not found"))?;
    Ok(Json(ticket))
}

async fn profile_id(state: &AppState, user: &AuthedUser) -> Result<uuid::Uuid, AppError> {
    get_profile_by_github_id(state, user.github_id)
        .await?
        .map(|profile| profile.id)
        .ok_or_else(|| AppError::not_found("Contributor profile not found"))
}

fn validate_required(value: String, field: &str, max_length: usize) -> Result<String, AppError> {
    let value = value.trim();
    if value.is_empty() {
        return Err(AppError::bad_request(format!("{field} cannot be empty")));
    }
    if value.chars().count() > max_length {
        return Err(AppError::bad_request(format!(
            "{field} must be at most {max_length} characters"
        )));
    }
    Ok(value.to_string())
}

fn validate_optional(
    value: Option<String>,
    field: &str,
    max_length: usize,
) -> Result<Option<String>, AppError> {
    value
        .map(|value| validate_required(value, field, max_length))
        .transpose()
}

#[cfg(test)]
mod tests {
    use super::{validate_optional, validate_required};

    #[test]
    fn required_ticket_fields_are_trimmed_and_must_fit_limits() {
        assert_eq!(
            validate_required("  Support  ".to_string(), "category", 10).unwrap(),
            "Support"
        );
        assert!(validate_required("  ".to_string(), "subject", 20).is_err());
        assert!(validate_required("too long".to_string(), "subject", 3).is_err());
    }

    #[test]
    fn optional_ticket_fields_are_trimmed_and_validated() {
        assert_eq!(
            validate_optional(Some(" repo ".to_string()), "org", 10).unwrap(),
            Some("repo".to_string())
        );
        assert!(validate_optional(Some(" ".to_string()), "org", 10).is_err());
        assert_eq!(validate_optional(None, "org", 10).unwrap(), None);
    }
}
