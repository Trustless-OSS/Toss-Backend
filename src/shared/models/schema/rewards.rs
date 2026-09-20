use rust_decimal::Decimal;
use uuid::Uuid;

use super::Repo;

#[derive(Debug, toasty::Model)]
#[table = "rewards"]
#[unique(repo_id, label)]
pub struct Reward {
    #[key]
    #[auto]
    pub id: Uuid,

    #[index]
    pub repo_id: Uuid,

    #[belongs_to]
    pub repo: toasty::Deferred<Repo>,

    #[index]
    pub label: String,

    #[default(Decimal::ZERO)]
    pub amount: Decimal,

    #[default(jiff::Timestamp::now())]
    pub created_at: jiff::Timestamp,
}
