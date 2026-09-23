use jiff::Timestamp;
use uuid::Uuid;

// unique constraint: (profile_id, chain, address)
#[derive(Debug, toasty::Model)]
#[table = "wallets"]
#[unique(profile_id, chain, address)]
pub struct Wallet {
    #[key]
    #[auto]
    pub id: Uuid,

    #[index]
    pub profile_id: Uuid,

    pub chain: String,
    pub address: String,

    #[default(false)]
    pub is_primary: bool,

    #[default(Timestamp::now())]
    pub created_at: Timestamp,

    #[default(Timestamp::now())]
    pub updated_at: Timestamp,
}
