use jiff::Timestamp;
use rust_decimal::Decimal;
use uuid::Uuid;

use super::{repositories::Repository, Bounty};

#[derive(Debug, toasty::Model)]
#[table = "reward_levels"]
#[unique(repo_id, label)]
pub struct Reward {
    #[key]
    #[auto]
    pub id: Uuid,

    #[index]
    pub repo_id: Uuid,
    #[belongs_to(key = repo_id, references = id)]
    pub repo: toasty::Deferred<Repository>,

    pub label: String,
    pub amount: Decimal,

    #[default(Timestamp::now())]
    pub created_at: Timestamp,

    #[default(Timestamp::now())]
    pub updated_at: Timestamp,

    #[has_many(pair = reward_level)]
    pub bounties: toasty::Deferred<Vec<Bounty>>,
}
