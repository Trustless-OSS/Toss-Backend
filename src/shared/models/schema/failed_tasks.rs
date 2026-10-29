use jiff::Timestamp;
use serde_json::Value;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskStatus {
    Pending,
    Resolved,
    Dead,
}

impl TaskStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Resolved => "resolved",
            Self::Dead => "dead",
        }
    }
}

// Generic dead-letter/retry queue, deliberately polymorphic — task_type
// (sync | payout | notify | bounty) determines how ref_id + payload are
// interpreted. This genericness is intentional: a single job-queue table
// is the right call here, don't split it per task_type or try to FK ref_id
// (it can't point at one table).
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
    pub status: String,

    pub last_attempt_at: Option<Timestamp>,

    #[default(Timestamp::now())]
    pub created_at: Timestamp,
}
