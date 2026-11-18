use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use super::entities::{Bounty, Profile};

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct BountyWithAssignee {
    #[serde(flatten)]
    pub bounty: Bounty,
    pub assignee: Option<Profile>,
}

#[derive(Debug, Clone, Default)]
pub struct ParsedLabels {
    pub is_rewarded: bool,
    pub is_rejected: bool,
    pub is_manual: bool,
    pub matched_level: Option<String>,
}
