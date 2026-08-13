//! Domain and API-composition types built from database entities.

use serde::{Deserialize, Serialize};

use super::entities::{Assignment, Contributor, Issue};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IssueWithRelations {
    #[serde(flatten)]
    pub issue: Issue,
    pub assignments: Vec<AssignmentWithContributor>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssignmentWithContributor {
    #[serde(flatten)]
    pub assignment: Assignment,
    pub contributors: Option<Contributor>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Difficulty {
    Low,
    Medium,
    High,
    Manual,
}

#[derive(Debug, Clone, Default)]
pub struct ParsedLabels {
    pub is_rewarded: bool,
    pub difficulty: Option<Difficulty>,
}
