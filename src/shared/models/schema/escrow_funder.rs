use jiff::Timestamp;
use rust_decimal::Decimal;
use uuid::Uuid;

#[derive(Debug, toasty::Model)]
#[table = "escrow_funders"]
pub struct EscrowFunder {
    #[key]
    #[auto]
    pub id: Uuid,

    #[index]
    pub repo_id: Uuid,

    pub wallet_address: String,

    #[default(String::from("stellar"))]
    pub chain: String,

    pub amount: Decimal,

    // blockchain tx id — idempotency guard
    #[unique]
    pub tx_hash: String,

    #[default(Timestamp::now())]
    pub funded_at: Timestamp,

    // null for anonymous funders
    #[index]
    pub profile_id: Option<Uuid>,
}
