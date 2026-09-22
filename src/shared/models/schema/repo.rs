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
    pub github_repo_id: i64,

    #[index]
    pub github_install_id: Option<i64>,

    pub full_name: String,

    #[default(false)]
    pub is_fork: bool,

    pub escrow_contract_id: Option<String>,

    #[default(Decimal::ZERO)]
    pub escrow_balance: Decimal,

    pub updated_at: Timestamp,

    #[default(Timestamp::now())]
    pub created_at: Timestamp,
}
