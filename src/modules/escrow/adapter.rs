use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::AppError;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DeployInput {
    pub repo_id: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FundInput {
    pub escrow_id: Option<String>,
    pub amount: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ReleaseMilestoneInput {
    pub escrow_id: Option<String>,
    pub milestone_id: Option<String>,
    pub amount: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RefundInput {
    pub escrow_id: Option<String>,
    pub amount: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DisputeInput {
    pub escrow_id: Option<String>,
    pub issue_id: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EscrowOperationResult {
    pub reference: Option<String>,
    pub payload: Option<Value>,
}

pub trait EscrowAdapter: Send + Sync {
    async fn deploy(&self, input: DeployInput) -> Result<EscrowOperationResult, AppError>;
    async fn fund(&self, input: FundInput) -> Result<EscrowOperationResult, AppError>;
    async fn release_milestone(
        &self,
        input: ReleaseMilestoneInput,
    ) -> Result<EscrowOperationResult, AppError>;
    async fn refund(&self, input: RefundInput) -> Result<EscrowOperationResult, AppError>;
    async fn dispute(&self, input: DisputeInput) -> Result<EscrowOperationResult, AppError>;
}
