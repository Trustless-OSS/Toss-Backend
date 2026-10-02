use jiff::Timestamp;
use uuid::Uuid;

use super::Profile;

#[derive(Debug, toasty::Model)]
#[table = "notifications"]
pub struct Notification {
    #[key]
    #[auto]
    pub id: Uuid,

    #[index]
    pub profile_id: Uuid,
    #[belongs_to(key = profile_id, references = id)]
    pub profile: toasty::Deferred<Profile>,

    pub kind: String,
    pub title: String,
    pub body: Option<String>,
    pub ref_id: Option<Uuid>,

    #[unique]
    pub dedupe_key: String,

    #[default(String::from("not_requested"))]
    pub email_status: String,

    pub email_sent_at: Option<Timestamp>,

    #[default(false)]
    pub is_read: bool,

    #[default(Timestamp::now())]
    pub created_at: Timestamp,
}
