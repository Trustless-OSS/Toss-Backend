use jiff::Timestamp;
use rust_decimal::Decimal;
use uuid::Uuid;

#[derive(Debug, toasty::Model)]
#[table = "repositories"]
pub struct Repositories {
    #[key]
    #[auto]
    pub id: Uuid,

    #[unique]
    #[index]
    pub github_repo_id: i64,

    pub github_install_id: Option<i64>,

    #[unique]
    pub full_name: String,

    pub escrow_contract_id: Option<String>,
    pub escrow_balance: Option<Decimal>,
    pub balance_synced_at: Option<Timestamp>,

    #[default(Timestamp::now())]
    pub created_at: Timestamp,
}
