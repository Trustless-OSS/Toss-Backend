use uuid::Uuid;

use super::Assignment;

#[derive(Debug, toasty::Model)]
#[table = "contributors"]
pub struct Contributor {
    #[key]
    #[auto]
    pub id: Uuid,

    #[unique]
    pub github_user_id: i64,

    pub github_username: String,
    pub stellar_wallet: Option<String>,

    #[default(String::from("stellar"))]
    pub payout_chain: String,

    pub payout_address: Option<String>,

    #[default(jiff::Timestamp::now())]
    pub created_at: jiff::Timestamp,

    #[has_many]
    pub assignments: toasty::Deferred<Vec<Assignment>>,
}
