use jiff::Timestamp;
use uuid::Uuid;

use super::Profile;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Chain {
    Stellar,
    Ethereum,
    Polygon,
    Arbitrum,
    Base,
}

impl Chain {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Stellar => "stellar",
            Self::Ethereum => "ethereum",
            Self::Polygon => "polygon",
            Self::Arbitrum => "arbitrum",
            Self::Base => "base",
        }
    }
}

#[derive(Debug, toasty::Model)]
#[table = "wallets"]
#[unique(profile_id, chain, address)]
pub struct Wallet {
    #[key]
    #[auto]
    pub id: Uuid,

    #[index]
    pub profile_id: Uuid,
    #[belongs_to(key = profile_id, references = id)]
    pub profile: toasty::Deferred<Profile>,

    pub chain: String,
    pub address: String,

    #[default(false)]
    pub is_primary: bool,

    #[default(Timestamp::now())]
    pub created_at: Timestamp,

    #[default(Timestamp::now())]
    pub updated_at: Timestamp,
}
