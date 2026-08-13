use rust_decimal::Decimal;
use uuid::Uuid;

use super::Issue;

#[derive(Debug, toasty::Model)]
#[table = "repos"]
pub struct Repo {
    #[key]
    #[auto]
    pub id: Uuid,

    #[unique]
    pub github_repo_id: i64,

    #[index]
    pub github_installation_id: Option<i64>,

    pub full_name: String,
    pub owner_github_id: i64,

    #[index]
    pub owner_username: String,

    pub owner_type: Option<String>,
    pub installer_github_id: Option<i64>,

    #[default(false)]
    pub is_fork: bool,

    #[default(false)]
    pub is_private: bool,

    pub escrow_contract_id: Option<String>,
    pub escrow_funder_wallet: Option<String>,

    #[default(Decimal::ZERO)]
    pub escrow_balance: Decimal,

    #[default(Decimal::ZERO)]
    pub reward_low: Decimal,

    #[default(Decimal::ZERO)]
    pub reward_medium: Decimal,

    #[default(Decimal::ZERO)]
    pub reward_high: Decimal,

    #[default(jiff::Timestamp::now())]
    pub created_at: jiff::Timestamp,

    #[has_many]
    pub issues: toasty::Deferred<Vec<Issue>>,
}
