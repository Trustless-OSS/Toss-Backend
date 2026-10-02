use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use super::schema;

fn ts(t: jiff::Timestamp) -> DateTime<Utc> {
    DateTime::from_timestamp(t.as_second(), t.subsec_nanosecond().unsigned_abs()).unwrap_or_else(
        || {
            debug_assert!(
                false,
                "timestamp out of chrono's representable range: {t:?}"
            );
            DateTime::UNIX_EPOCH
        },
    )
}

fn opt_ts(t: Option<jiff::Timestamp>) -> Option<DateTime<Utc>> {
    t.map(ts)
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct Repo {
    pub id: Uuid,
    pub github_repo_id: i64,
    pub github_install_id: Option<i64>,
    pub full_name: String,
    pub escrow_contract_id: Option<String>,

    #[schema(value_type = Option<String>)]
    #[serde(with = "rust_decimal::serde::str_option")]
    pub escrow_balance: Option<Decimal>,

    pub balance_synced_at: Option<DateTime<Utc>>,
    pub rewards: Vec<Reward>,
    pub created_at: DateTime<Utc>,
}

impl Repo {
    pub fn from_parts(repo: schema::Repository, rewards: Vec<schema::Reward>) -> Self {
        Self {
            id: repo.id,
            github_repo_id: repo.github_repo_id,
            github_install_id: repo.github_install_id,
            full_name: repo.full_name,
            escrow_contract_id: repo.escrow_contract_id,
            escrow_balance: repo.escrow_balance,
            balance_synced_at: opt_ts(repo.balance_synced_at),
            rewards: rewards.into_iter().map(Reward::from).collect(),
            created_at: ts(repo.created_at),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct Reward {
    pub id: Uuid,
    pub repo_id: Uuid,
    pub label: String,

    #[schema(value_type = String)]
    #[serde(with = "rust_decimal::serde::str")]
    pub amount: Decimal,

    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct Profile {
    pub id: Uuid,
    pub github_id: i64,
    pub username: String,
    pub full_name: Option<String>,
    pub email: Option<String>,
    pub avatar_url: Option<String>,
    pub bio: Option<String>,
    pub location: Option<String>,
    pub skills: Option<Vec<String>>,
    pub telegram: Option<String>,
    pub discord: Option<String>,
    pub twitter: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct Wallet {
    pub id: Uuid,
    pub profile_id: Uuid,
    pub chain: String,
    pub address: String,
    pub is_primary: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct Bounty {
    pub id: Uuid,
    pub repo_id: Uuid,
    pub reward_level_id: Option<Uuid>,
    pub milestone_index: Option<i32>,
    pub github_issue_id: i64,
    pub github_issue_number: i32,
    pub title: Option<String>,

    #[schema(value_type = Option<String>)]
    #[serde(with = "rust_decimal::serde::str_option")]
    pub reward_amount: Option<Decimal>,

    pub status: String,
    pub assignee_id: Option<Uuid>,

    pub assigned_at: Option<DateTime<Utc>>,
    pub merged_at: Option<DateTime<Utc>>,
    pub paid_at: Option<DateTime<Utc>>,

    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct EscrowFunder {
    pub id: Uuid,
    pub repo_id: Uuid,
    pub wallet_address: String,
    pub chain: String,

    #[schema(value_type = String)]
    #[serde(with = "rust_decimal::serde::str")]
    pub amount: Decimal,

    pub tx_hash: String,
    pub funded_at: DateTime<Utc>,
    pub profile_id: Option<Uuid>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct Notification {
    pub id: Uuid,
    pub profile_id: Uuid,
    pub kind: String,
    pub title: String,
    pub body: Option<String>,
    pub ref_id: Option<Uuid>,
    pub dedupe_key: String,
    pub email_status: String,
    pub email_sent_at: Option<DateTime<Utc>>,
    pub is_read: bool,
    pub created_at: DateTime<Utc>,
}

impl From<schema::Reward> for Reward {
    fn from(v: schema::Reward) -> Self {
        Self {
            id: v.id,
            repo_id: v.repo_id,
            label: v.label,
            amount: v.amount,
            updated_at: ts(v.updated_at),
        }
    }
}

impl From<schema::Repository> for Repo {
    fn from(v: schema::Repository) -> Self {
        Self::from_parts(v, vec![])
    }
}

impl From<schema::Profile> for Profile {
    fn from(v: schema::Profile) -> Self {
        Self {
            id: v.id,
            github_id: v.github_id,
            username: v.username,
            full_name: v.full_name,
            email: v.email,
            avatar_url: v.avatar_url,
            bio: v.bio,
            location: v.location,
            skills: v.skills,
            telegram: v.telegram,
            discord: v.discord,
            twitter: v.twitter,
            created_at: ts(v.created_at),
            updated_at: ts(v.updated_at),
        }
    }
}

impl From<schema::Wallet> for Wallet {
    fn from(v: schema::Wallet) -> Self {
        Self {
            id: v.id,
            profile_id: v.profile_id,
            chain: v.chain,
            address: v.address,
            is_primary: v.is_primary,
            created_at: ts(v.created_at),
            updated_at: ts(v.updated_at),
        }
    }
}

impl From<schema::Bounty> for Bounty {
    fn from(v: schema::Bounty) -> Self {
        Self {
            id: v.id,
            repo_id: v.repo_id,
            reward_level_id: v.reward_level_id,
            milestone_index: v.milestone_index,
            github_issue_id: v.github_issue_id,
            github_issue_number: v.github_issue_number,
            title: v.title,
            reward_amount: v.reward_amount,
            status: v.status,
            assignee_id: v.assignee_id,
            assigned_at: opt_ts(v.assigned_at),
            merged_at: opt_ts(v.merged_at),
            paid_at: opt_ts(v.paid_at),
            created_at: ts(v.created_at),
        }
    }
}

impl From<schema::Notification> for Notification {
    fn from(v: schema::Notification) -> Self {
        Self {
            id: v.id,
            profile_id: v.profile_id,
            kind: v.kind,
            title: v.title,
            body: v.body,
            ref_id: v.ref_id,
            dedupe_key: v.dedupe_key,
            email_status: v.email_status,
            email_sent_at: opt_ts(v.email_sent_at),
            is_read: v.is_read,
            created_at: ts(v.created_at),
        }
    }
}

impl From<schema::EscrowFunder> for EscrowFunder {
    fn from(v: schema::EscrowFunder) -> Self {
        Self {
            id: v.id,
            repo_id: v.repo_id,
            wallet_address: v.wallet_address,
            chain: v.chain,
            amount: v.amount,
            tx_hash: v.tx_hash,
            funded_at: ts(v.funded_at),
            profile_id: v.profile_id,
        }
    }
}
