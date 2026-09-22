use jiff::Timestamp;
use uuid::Uuid;

#[derive(Debug, toasty::Model)]
pub struct Wallet {
    #[key]
    #[auto]
    pub id: Uuid,

    #[unique]
    #[index]
    pub profile_id: Uuid,

    pub chain: String,

    pub address: String,

    pub is_primary: bool,

    #[default(Timestamp::now())]
    pub created_at: Timestamp,

    #[default(Timestamp::now())]
    pub updated_at: Timestamp,
}
