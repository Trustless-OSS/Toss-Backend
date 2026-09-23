use jiff::Timestamp;
use serde_json::Value;
use uuid::Uuid;

// append-only — never updated or deleted
#[derive(Debug, toasty::Model)]
#[table = "activity"]
pub struct Activity {
    #[key]
    #[auto]
    pub id: Uuid,

    #[index]
    pub repo_id: Option<Uuid>,

    #[index]
    pub actor_id: Option<Uuid>,

    pub event_type: String,

    #[column(type = jsonb)]
    pub payload: Option<Value>,

    #[default(Timestamp::now())]
    pub created_at: Timestamp,
}
