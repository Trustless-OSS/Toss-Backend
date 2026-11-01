use crate::{
    error::{AppError, ErrorResponse},
    middleware::auth::AuthedUser,
    state::AppState,
};
use aws_sdk_s3::presigning::PresigningConfig;
use axum::{extract::State, Json};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, time::Duration};
use utoipa::ToSchema;
use uuid::Uuid;

const MAX_UPLOAD_SIZE: u64 = 5 * 1024 * 1024 * 1024;
const URL_TTL_SECS: u64 = 300;

#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UploadRequest {
    pub content_type: String,
    pub size: u64,
    pub folder: Option<String>,
}

#[derive(Serialize, ToSchema)]
pub struct UploadResponse {
    pub key: String,
    pub upload_url: String,
    pub content_type: String,
    pub headers: BTreeMap<String, String>,
    pub expires_in: u64,
}

#[utoipa::path(
    post,
    path = "/api/v1/upload",
    tag = "Upload",
    request_body = UploadRequest,
    security(("bearer_auth" = [])),
    responses(
        (status = 200, description = "Short-lived direct upload URL", body = UploadResponse),
        (status = 400, description = "Invalid upload request", body = ErrorResponse),
        (status = 401, description = "Missing or invalid bearer token", body = ErrorResponse),
        (status = 500, description = "Failed to create upload URL", body = ErrorResponse)
    )
)]
pub async fn upload(
    State(state): State<AppState>,
    user: AuthedUser,
    Json(input): Json<UploadRequest>,
) -> Result<Json<UploadResponse>, AppError> {
    if input.size == 0 || input.size > MAX_UPLOAD_SIZE || input.content_type.is_empty() {
        return Err(AppError::bad_request("Invalid file size or content type"));
    }

    let folder = input.folder.as_deref().unwrap_or("uploads");
    if folder.is_empty()
        || !folder
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
    {
        return Err(AppError::bad_request("Invalid upload folder"));
    }

    let key = format!("{folder}/{}/{}", user.github_id, Uuid::new_v4());
    let presigned = state
        .s3_client
        .put_object()
        .bucket(&state.config.s3_bucket_name)
        .key(&key)
        .content_type(&input.content_type)
        .content_length(input.size as i64)
        .presigned(
            PresigningConfig::expires_in(Duration::from_secs(URL_TTL_SECS))
                .map_err(|_| AppError::internal("Failed to create upload URL"))?,
        )
        .await
        .map_err(|error| {
            tracing::error!(%error, "Failed to create presigned upload URL");
            AppError::internal("Failed to create upload URL")
        })?;

    Ok(Json(UploadResponse {
        key,
        upload_url: presigned.uri().to_owned(),
        content_type: input.content_type,
        headers: presigned
            .headers()
            .map(|(name, value)| (name.to_owned(), value.to_owned()))
            .collect(),
        expires_in: URL_TTL_SECS,
    }))
}

pub fn router() -> axum::Router<AppState> {
    use axum::routing::post;

    axum::Router::new().route("/api/v1/upload", post(upload))
}
