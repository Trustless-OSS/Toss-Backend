use jiff::Timestamp;
use serde_json::Value;
use uuid::Uuid;

use super::{Profile, Repository};

#[derive(Debug, toasty::Model)]
#[table = "activity"]
pub struct Activity {
    #[key]
    #[auto]
    pub id: Uuid,

    #[index]
    pub repo_id: Option<Uuid>,
    #[belongs_to(key = repo_id, references = id)]
    pub repo: toasty::Deferred<Option<Repository>>,

    #[index]
    pub actor_id: Option<Uuid>,
    #[belongs_to(key = actor_id, references = id)]
    pub actor: toasty::Deferred<Option<Profile>>,

    #[index]
    pub event_type: String,

    #[column(type = jsonb)]
    pub payload: Option<Value>,

    #[default(Timestamp::now())]
    pub created_at: Timestamp,
}
