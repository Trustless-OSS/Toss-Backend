use jiff::Timestamp;
use rust_decimal::Decimal;
use uuid::Uuid;

use super::{Profile, Repository};

#[derive(Debug, toasty::Model)]
#[table = "escrow_funders"]
pub struct EscrowFunder {
    #[key]
    #[auto]
    pub id: Uuid,

    #[index]
    pub repo_id: Uuid,
    #[belongs_to(key = repo_id, references = id)]
    pub repo: toasty::Deferred<Repository>,

    #[index]
    pub wallet_address: String,

    #[default(String::from("stellar"))]
    pub chain: String,

    pub amount: Decimal,

    #[unique]
    pub tx_hash: String,

    #[default(Timestamp::now())]
    pub funded_at: Timestamp,

    #[index]
    pub profile_id: Option<Uuid>,
    #[belongs_to(key = profile_id, references = id)]
    pub profile: toasty::Deferred<Option<Profile>>,
}
