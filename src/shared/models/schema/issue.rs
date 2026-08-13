use rust_decimal::Decimal;
use uuid::Uuid;

use super::{Assignment, Repo};

#[derive(Debug, toasty::Model)]
#[table = "issues"]
#[unique(repo_id, github_issue_id)]
pub struct Issue {
    #[key]
    #[auto]
    pub id: Uuid,

    #[index]
    pub repo_id: Uuid,

    #[belongs_to]
    pub repo: toasty::Deferred<Repo>,

    pub github_issue_id: i64,

    #[index]
    pub github_issue_number: i32,

    pub title: String,

    pub reward_amount: Decimal,
    pub difficulty_label: Option<String>,
    pub milestone_index: Option<i32>,

    #[default(String::from("pending"))]
    pub status: String,

    #[default(jiff::Timestamp::now())]
    pub created_at: jiff::Timestamp,

    #[has_many]
    pub assignments: toasty::Deferred<Vec<Assignment>>,
}
