use jiff::Timestamp;
use serde_json::Value;
use uuid::Uuid;

// status: pending | resolved | dead
// task_type: sync | payout | notify | bounty
// ref_id is untyped — points to whichever entity the job was processing
#[derive(Debug, toasty::Model)]
#[table = "failed_tasks"]
pub struct FailedTask {
    #[key]
    #[auto]
    pub id: Uuid,

    #[index]
    pub task_type: String,

    pub ref_id: Option<Uuid>,

    #[column(type = jsonb)]
    pub payload: Value,

    pub error: Option<String>,

    #[default(1_i32)]
    pub attempts: i32,

    #[default(String::from("pending"))]
    #[index]
    pub status: String,

    pub last_attempt_at: Option<Timestamp>,

    #[default(Timestamp::now())]
    pub created_at: Timestamp,
}
