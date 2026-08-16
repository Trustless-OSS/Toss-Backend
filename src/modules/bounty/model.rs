//! Bounty module API types (milestones, retries).
//!
//! Database entities live in [`crate::shared::models::Issue`] and
//! [`crate::shared::models::Assignment`].

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Milestone {
    pub github_issue_id: i64,
    pub github_repo_id: i64,
    pub wallet: String,
    pub payout_chain: Option<String>,
    pub payout_address: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct MilestoneResponse {
    pub ok: bool,
    pub repo_full_name: String,
    pub issue_number: i32,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct RetryIssueResponse {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub step: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tx_hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<&'static str>,
}
