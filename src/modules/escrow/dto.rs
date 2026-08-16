use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

// ── Request bodies ────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateEscrowBody {
    pub repo_id: Uuid,
    pub maintainer_wallet: String,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SubmitDeployBody {
    pub repo_id: Uuid,
    pub signed_xdr: String,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct FundEscrowBody {
    pub repo_id: Uuid,
    pub amount: Decimal,
    pub funder_wallet: String,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SubmitFundBody {
    pub repo_id: Uuid,
    pub amount: Decimal,
    pub signed_xdr: String,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RefundEscrowBody {
    pub repo_id: Uuid,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CloseEscrowBody {
    pub repo_id: Uuid,
    pub maintainer_wallet: String,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SubmitCloseBody {
    pub repo_id: Uuid,
    pub signed_xdr: String,
}

// ── Response structs ──────────────────────────────────────────────────────────

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UnsignedTransactionResponse {
    pub unsigned_transaction: String,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ContractIdResponse {
    pub contract_id: String,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SubmitFundResponse {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub new_balance: Option<Decimal>,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RefundResponse {
    pub refunded_amount: Decimal,
    pub cancelled_issues: i64,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct OkResponse {
    pub ok: bool,
}
