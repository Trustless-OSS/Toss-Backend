use rust_decimal::Decimal;
use uuid::Uuid;

#[derive(Debug, toasty::Model)]
#[table = "rewards"]
#[unique(repo_id, label)]
pub struct Reward {
    #[key]
    #[auto]
    pub id: Uuid,

    #[index]
    pub repo_id: Uuid,

    pub label: String,

    #[default(Decimal::ZERO)]
    pub amount: Decimal,

    #[default(jiff::Timestamp::now())]
    pub updated_at: jiff::Timestamp,
}
