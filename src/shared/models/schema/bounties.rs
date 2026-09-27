use jiff::Timestamp;
use rust_decimal::Decimal;
use uuid::Uuid;

use super::{Profile, Repository, Reward};

#[derive(Debug, toasty::Model)]
#[table = "bounties"]
#[unique(repo_id, github_issue_number)]
pub struct Bounty {
    #[key]
    #[auto]
    pub id: Uuid,

    #[index]
    pub repo_id: Uuid,
    #[belongs_to(key = repo_id, references = id)]
    pub repo: toasty::Deferred<Repository>,

    #[index]
    pub reward_level_id: Option<Uuid>,
    #[belongs_to(key = reward_level_id, references = id)]
    pub reward_level: toasty::Deferred<Option<Reward>>,

    pub milestone_index: Option<i32>,

    #[unique]
    pub github_issue_id: i64,

    #[index]
    pub github_issue_number: i32,

    pub title: Option<String>,
    pub reward_amount: Option<Decimal>,

    #[default(String::from("open"))]
    #[index]
    pub status: String,

    #[index]
    pub assignee_id: Option<Uuid>,
    #[belongs_to(key = assignee_id, references = id)]
    pub assignee: toasty::Deferred<Option<Profile>>,

    pub assigned_at: Option<Timestamp>,
    pub merged_at: Option<Timestamp>,
    pub paid_at: Option<Timestamp>,

    #[default(Timestamp::now())]
    pub created_at: Timestamp,
}
