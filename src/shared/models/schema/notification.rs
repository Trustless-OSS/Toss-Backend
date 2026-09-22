use jiff::Timestamp;
use uuid::Uuid;

#[derive(toasty::Model, Debug)]
#[table = "notification"]
pub struct Notification {
    #[key]
    #[auto]
    pub id: Uuid,

    #[index]
    pub profile_id: Uuid,

    pub kind: String,

    pub title: String,

    pub body: String,

    pub ref_id: Uuid,

    #[default(false)]
    pub is_read: bool,

    #[default(Timestamp::now())]
    pub created_at: Timestamp,
}
