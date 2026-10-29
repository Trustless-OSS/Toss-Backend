use axum::{extract::Path, http::StatusCode, Json};
use chrono::{DateTime, Utc};
use serde::Serialize;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::{
    error::{AppError, ErrorResponse},
    middleware::auth::AuthedUser,
    shared::models::entities::Notification,
    state::AppState,
};

use super::repository::Repository;

#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct NotificationResponse {
    pub id: Uuid,
    pub kind: String,
    pub title: String,
    pub body: Option<String>,
    pub is_read: bool,
    pub ref_id: Option<Uuid>,
    pub data: Option<serde_json::Value>,
    pub created_at: DateTime<Utc>,
}

impl From<Notification> for NotificationResponse {
    fn from(n: Notification) -> Self {
        Self {
            id: n.id,
            kind: n.kind,
            title: n.title,
            body: n.body,
            is_read: n.is_read,
            ref_id: n.ref_id,
            data: n.data,
            created_at: n.created_at,
        }
    }
}

#[derive(Debug, Serialize, ToSchema)]
pub struct UnreadCountResponse {
    pub count: i64,
}

#[utoipa::path(
    get,
    path = "/api/v1/notifications",
    tag = "Notifications",
    security(("bearer_auth" = [])),
    params(
        ("limit" = i64, Query, description = "Number of notifications to return", example = 20),
        ("offset" = i64, Query, description = "Pagination offset", example = 0),
    ),
    responses(
        (status = 200, description = "List of notifications", body = Vec<NotificationResponse>),
        (status = 401, description = "Missing or invalid bearer token", body = ErrorResponse),
        (status = 500, description = "Database error", body = ErrorResponse)
    )
)]
pub(crate) async fn list_notifications(
    axum::extract::State(state): axum::extract::State<AppState>,
    user: AuthedUser,
    axum::extract::Query(params): axum::extract::Query<ListParams>,
) -> Result<Json<Vec<NotificationResponse>>, AppError> {
    use crate::modules::contributor::repository::get_profile_by_github_id;

    let profile = match get_profile_by_github_id(&state, user.github_id)
        .await
        .unwrap_or(None)
    {
        Some(p) => p,
        None => return Ok(Json(vec![])), // Return empty list if profile not found
    };

    let limit = params.limit.unwrap_or(20).clamp(1, 100);
    let offset = params.offset.unwrap_or(0).max(0);

    let notifications = Repository::list(&state, profile.id, false, limit, offset).await?;

    Ok(Json(
        notifications
            .into_iter()
            .map(NotificationResponse::from)
            .collect(),
    ))
}

#[derive(serde::Deserialize)]
pub struct ListParams {
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

#[utoipa::path(
    get,
    path = "/api/v1/notifications/unread-count",
    tag = "Notifications",
    security(("bearer_auth" = [])),
    responses(
        (status = 200, description = "Number of unread notifications", body = UnreadCountResponse),
        (status = 401, description = "Missing or invalid bearer token", body = ErrorResponse),
        (status = 500, description = "Database error", body = ErrorResponse)
    )
)]
pub(crate) async fn unread_count(
    axum::extract::State(state): axum::extract::State<AppState>,
    user: AuthedUser,
) -> Result<Json<UnreadCountResponse>, AppError> {
    use crate::modules::contributor::repository::get_profile_by_github_id;

    let profile = match get_profile_by_github_id(&state, user.github_id)
        .await
        .unwrap_or(None)
    {
        Some(p) => p,
        None => return Ok(Json(UnreadCountResponse { count: 0 })), // Return 0 if profile not found
    };

    let count = Repository::unread_count(&state, profile.id).await?;

    Ok(Json(UnreadCountResponse { count }))
}

#[utoipa::path(
    post,
    path = "/api/v1/notifications/read-all",
    tag = "Notifications",
    security(("bearer_auth" = [])),
    responses(
        (status = 200, description = "All notifications marked as read"),
        (status = 401, description = "Missing or invalid bearer token", body = ErrorResponse),
        (status = 500, description = "Database error", body = ErrorResponse)
    )
)]
pub(crate) async fn read_all(
    axum::extract::State(state): axum::extract::State<AppState>,
    user: AuthedUser,
) -> Result<StatusCode, AppError> {
    use crate::modules::contributor::repository::get_profile_by_github_id;

    if let Some(profile) = get_profile_by_github_id(&state, user.github_id)
        .await
        .unwrap_or(None)
    {
        Repository::mark_read(&state, profile.id, None).await?;
    }

    Ok(StatusCode::OK)
}

#[utoipa::path(
    post,
    path = "/api/v1/notifications/{id}/read",
    tag = "Notifications",
    security(("bearer_auth" = [])),
    params(
        ("id" = Uuid, Path, description = "Notification ID"),
    ),
    responses(
        (status = 200, description = "Notification marked as read"),
        (status = 401, description = "Missing or invalid bearer token", body = ErrorResponse),
        (status = 404, description = "Notification not found or not owned by user", body = ErrorResponse),
        (status = 500, description = "Database error", body = ErrorResponse)
    )
)]
pub(crate) async fn read_one(
    axum::extract::State(state): axum::extract::State<AppState>,
    user: AuthedUser,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, AppError> {
    use crate::modules::contributor::repository::get_profile_by_github_id;

    let profile = match get_profile_by_github_id(&state, user.github_id)
        .await
        .unwrap_or(None)
    {
        Some(p) => p,
        None => return Err(AppError::not_found("Notification")),
    };

    // Verify notification belongs to this profile before marking read
    let notification = Repository::get(&state, id)
        .await?
        .ok_or_else(|| AppError::not_found("Notification"))?;

    if notification.profile_id != profile.id {
        return Err(AppError::forbidden(
            "Notification does not belong to this user",
        ));
    }

    Repository::mark_read(&state, profile.id, Some(id)).await?;

    Ok(StatusCode::OK)
}
