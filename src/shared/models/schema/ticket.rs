use jiff::Timestamp;
use uuid::Uuid;

use super::Profile;

#[derive(Debug, toasty::Model)]
#[table = "tickets"]
pub struct Ticket {
    #[key]
    #[auto]
    pub id: Uuid,

    #[index]
    pub profile_id: Uuid,
    #[belongs_to(key = profile_id, references = id)]
    pub profile: toasty::Deferred<Profile>,

    pub org: Option<String>,

    pub category: String,
    pub subject: String,
    pub message: String,
    pub attachment_key: Option<String>,

    pub bounty_id: Option<Uuid>,

    #[default(String::from("open"))]
    #[index]
    pub status: String,

    #[default(Timestamp::now())]
    pub created_at: Timestamp,
    #[default(Timestamp::now())]
    pub updated_at: Timestamp,
}
