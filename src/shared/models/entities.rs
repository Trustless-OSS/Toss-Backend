//! Runtime / API / cache DTO types.
//!
//! These are serde-friendly views of Toasty schema models. Prefer mapping from
//! [`super::schema`] types via [`From`] rather than querying into these directly.

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::schema;

fn timestamp_to_chrono(ts: jiff::Timestamp) -> DateTime<Utc> {
    DateTime::from_timestamp(ts.as_second(), ts.subsec_nanosecond().unsigned_abs())
        .unwrap_or_else(|| DateTime::UNIX_EPOCH)
}

fn optional_timestamp(ts: Option<jiff::Timestamp>) -> Option<DateTime<Utc>> {
    ts.map(timestamp_to_chrono)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Repo {
    pub id: Uuid,
    pub github_repo_id: i64,
    pub github_installation_id: Option<i64>,
    pub full_name: String,
    pub owner_github_id: i64,
    pub owner_username: String,
    pub owner_type: Option<String>,
    pub installer_github_id: Option<i64>,
    pub is_fork: Option<bool>,
    pub is_private: Option<bool>,
    pub escrow_contract_id: Option<String>,
    pub escrow_funder_wallet: Option<String>,
    pub escrow_balance: Decimal,
    pub reward_low: Decimal,
    pub reward_medium: Decimal,
    pub reward_high: Decimal,
    pub created_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Contributor {
    pub id: Uuid,
    pub github_user_id: i64,
    pub github_username: String,
    pub stellar_wallet: Option<String>,
    pub payout_chain: Option<String>,
    pub payout_address: Option<String>,
    pub created_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Issue {
    pub id: Uuid,
    pub repo_id: Uuid,
    pub github_issue_id: i64,
    pub github_issue_number: i32,
    pub title: String,
    pub reward_amount: Decimal,
    pub difficulty_label: Option<String>,
    pub milestone_index: Option<i32>,
    pub status: String,
    pub created_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Assignment {
    pub id: Uuid,
    pub issue_id: Uuid,
    pub contributor_id: Option<Uuid>,
    pub assigned_at: Option<DateTime<Utc>>,
    pub pr_number: Option<i32>,
    pub pr_merged_at: Option<DateTime<Utc>>,
    pub payout_status: String,
    pub completion_percentage: Option<Decimal>,
}

impl From<schema::Repo> for Repo {
    fn from(value: schema::Repo) -> Self {
        Self {
            id: value.id,
            github_repo_id: value.github_repo_id,
            github_installation_id: value.github_installation_id,
            full_name: value.full_name,
            owner_github_id: value.owner_github_id,
            owner_username: value.owner_username,
            owner_type: value.owner_type,
            installer_github_id: value.installer_github_id,
            is_fork: Some(value.is_fork),
            is_private: Some(value.is_private),
            escrow_contract_id: value.escrow_contract_id,
            escrow_funder_wallet: value.escrow_funder_wallet,
            escrow_balance: value.escrow_balance,
            reward_low: value.reward_low,
            reward_medium: value.reward_medium,
            reward_high: value.reward_high,
            created_at: Some(timestamp_to_chrono(value.created_at)),
        }
    }
}

impl From<schema::Contributor> for Contributor {
    fn from(value: schema::Contributor) -> Self {
        Self {
            id: value.id,
            github_user_id: value.github_user_id,
            github_username: value.github_username,
            stellar_wallet: value.stellar_wallet,
            payout_chain: Some(value.payout_chain),
            payout_address: value.payout_address,
            created_at: Some(timestamp_to_chrono(value.created_at)),
        }
    }
}

impl From<schema::Issue> for Issue {
    fn from(value: schema::Issue) -> Self {
        Self {
            id: value.id,
            repo_id: value.repo_id,
            github_issue_id: value.github_issue_id,
            github_issue_number: value.github_issue_number,
            title: value.title,
            reward_amount: value.reward_amount,
            difficulty_label: value.difficulty_label,
            milestone_index: value.milestone_index,
            status: value.status,
            created_at: Some(timestamp_to_chrono(value.created_at)),
        }
    }
}

impl From<schema::Assignment> for Assignment {
    fn from(value: schema::Assignment) -> Self {
        Self {
            id: value.id,
            issue_id: value.issue_id,
            contributor_id: value.contributor_id,
            assigned_at: optional_timestamp(value.assigned_at),
            pr_number: value.pr_number,
            pr_merged_at: optional_timestamp(value.pr_merged_at),
            payout_status: value.payout_status,
            completion_percentage: value.completion_percentage,
        }
    }
}
