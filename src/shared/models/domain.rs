use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use super::entities::{Bounty, Profile};

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct BountyWithAssignee {
    #[serde(flatten)]
    pub bounty: Bounty,
    pub assignee: Option<Profile>,
}

// #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
// #[serde(rename_all = "lowercase")]
// pub enum Difficulty {
//     Low,
//     Medium,
//     High,
//     Manual,
// }

#[derive(Debug, Clone, Default)]
pub struct ParsedLabels {
    pub is_rewarded: bool,
    pub is_rejected: bool,
    pub is_manual: bool,
    /// Matched reward label (e.g., "low", "high", "epic", or custom). None if no match found.
    pub matched_level: Option<String>,
}
