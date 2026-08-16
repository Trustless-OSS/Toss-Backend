//! Contributor module API types.
//!
//! Database entities live in [`crate::shared::models::Contributor`].

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::shared::models::Issue;

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ConnectWalletBody {
    pub wallet: String,
    pub payout_chain: Option<String>,
    pub payout_address: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct OkResponse {
    pub ok: bool,
}

/// Profile returned by `GET /api/contributor/me` when the caller has a contributor record.
#[derive(Debug, Serialize, ToSchema)]
pub struct ContributorProfile {
    pub id: Uuid,
    pub github_user_id: i64,
    pub github_username: String,
    pub stellar_wallet: Option<String>,
    pub payout_chain: Option<String>,
    pub payout_address: Option<String>,
    pub created_at: Option<chrono::DateTime<chrono::Utc>>,
    pub assignments: Vec<ContributorAssignmentView>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ContributorAssignmentView {
    pub id: Uuid,
    pub issue_id: Uuid,
    pub contributor_id: Option<Uuid>,
    pub assigned_at: Option<chrono::DateTime<chrono::Utc>>,
    pub pr_number: Option<i32>,
    pub pr_merged_at: Option<chrono::DateTime<chrono::Utc>>,
    pub payout_status: String,
    pub completion_percentage: Option<rust_decimal::Decimal>,
    pub issues: Option<Issue>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ContributorMeResponse {
    #[schema(value_type = Option<ContributorProfile>)]
    pub contributor: Option<serde_json::Value>,
}
