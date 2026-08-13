use rust_decimal::Decimal;
use uuid::Uuid;

use super::{Contributor, Issue};

#[derive(Debug, toasty::Model)]
#[table = "assignments"]
pub struct Assignment {
    #[key]
    #[auto]
    pub id: Uuid,

    #[unique]
    pub issue_id: Uuid,

    #[belongs_to]
    pub issue: toasty::Deferred<Issue>,

    #[index]
    pub contributor_id: Option<Uuid>,

    #[belongs_to]
    pub contributor: toasty::Deferred<Option<Contributor>>,

    pub assigned_at: Option<jiff::Timestamp>,
    pub pr_number: Option<i32>,
    pub pr_merged_at: Option<jiff::Timestamp>,

    #[default(String::from("pending"))]
    pub payout_status: String,

    pub completion_percentage: Option<Decimal>,
}
