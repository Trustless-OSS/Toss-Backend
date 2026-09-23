use jiff::Timestamp;
use rust_decimal::Decimal;
use uuid::Uuid;

// unique constraint: (repo_id, github_issue_number)
// status lifecycle: open → assigned → merged → paid | cancelled
#[derive(Debug, toasty::Model)]
#[table = "bounties"]
#[unique(repo_id, github_issue_number)]
pub struct Bounty {
    #[key]
    #[auto]
    pub id: Uuid,

    #[index]
    pub repo_id: Uuid,

    pub reward_level_id: Option<Uuid>,

    // on-chain milestone slot index in the escrow contract; null before on-chain creation
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

    pub assigned_at: Option<Timestamp>,
    pub merged_at: Option<Timestamp>,
    pub paid_at: Option<Timestamp>,

    #[default(Timestamp::now())]
    pub created_at: Timestamp,
}
