use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
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

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Contributor {
    pub id: Uuid,
    pub github_user_id: i64,
    pub github_username: String,
    pub stellar_wallet: Option<String>,
    pub payout_chain: Option<String>,
    pub payout_address: Option<String>,
    pub created_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
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

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IssueWithRelations {
    #[serde(flatten)]
    pub issue: Issue,
    pub assignments: Vec<AssignmentWithContributor>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssignmentWithContributor {
    #[serde(flatten)]
    pub assignment: Assignment,
    pub contributors: Option<Contributor>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Difficulty {
    Low,
    Medium,
    High,
    Manual,
}

#[derive(Debug, Clone, Default)]
pub struct ParsedLabels {
    pub is_rewarded: bool,
    pub difficulty: Option<Difficulty>,
}
