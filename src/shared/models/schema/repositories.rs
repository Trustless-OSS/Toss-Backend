use super::{Activity, Bounty, EscrowFunder, RepoMaintainer, Reward};
use jiff::Timestamp;
use rust_decimal::Decimal;
use uuid::Uuid;

#[derive(Debug, toasty::Model)]
#[table = "repositories"]
pub struct Repository {
    #[key]
    #[auto]
    pub id: Uuid,

    #[unique]
    pub github_repo_id: i64,

    pub github_install_id: Option<i64>,

    #[unique]
    pub full_name: String,

    pub escrow_contract_id: Option<String>,
    pub escrow_balance: Option<Decimal>,
    pub balance_synced_at: Option<Timestamp>,

    #[default(0_i32)]
    pub balance_version: i32,

    #[default(Timestamp::now())]
    pub created_at: Timestamp,

    #[has_many(pair = repo)]
    pub bounties: toasty::Deferred<Vec<Bounty>>,

    #[has_many(pair = repo)]
    pub reward_levels: toasty::Deferred<Vec<Reward>>,

    #[has_many(pair = repo)]
    pub escrow_funders: toasty::Deferred<Vec<EscrowFunder>>,

    #[has_many(pair = repo)]
    pub maintainers: toasty::Deferred<Vec<RepoMaintainer>>,

    #[has_many(pair = repo)]
    pub activity: toasty::Deferred<Vec<Activity>>,
}
