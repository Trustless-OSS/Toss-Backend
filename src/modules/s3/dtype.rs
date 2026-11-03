use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use utoipa::ToSchema;

pub const MAX_UPLOAD_SIZE: u64 = 5 * 1024 * 1024 * 1024;
pub const URL_TTL_SECS: u64 = 300;

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
