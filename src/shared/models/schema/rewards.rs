use rust_decimal::Decimal;
use uuid::Uuid;

// unique constraint: (repo_id, label)
#[derive(Debug, toasty::Model)]
#[table = "reward_levels"]
#[unique(repo_id, label)]
pub struct Reward {
    #[key]
    #[auto]
    pub id: Uuid,

    #[index]
    pub repo_id: Uuid,

    pub label: String,
    pub amount: Decimal,

    #[default(jiff::Timestamp::now())]
    pub updated_at: jiff::Timestamp,
}
