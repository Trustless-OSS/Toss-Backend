use jiff::Timestamp;
use uuid::Uuid;

// kinds: assigned | paid | bounty_posted | mention | payout_failed
#[derive(Debug, toasty::Model)]
#[table = "notifications"]
pub struct Notification {
    #[key]
    #[auto]
    pub id: Uuid,

    #[index]
    pub profile_id: Uuid,

    pub kind: String,
    pub title: String,
    pub body: Option<String>,
    pub ref_id: Option<Uuid>,

    #[default(false)]
    pub is_read: bool,

    #[default(Timestamp::now())]
    pub created_at: Timestamp,
}
