use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

// Request DTO

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateEscrow {
    pub repo_id: Uuid,
    pub maintainer_wallet: String,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SubmitDeploy {
    pub repo_id: Uuid,
    pub signed_xdr: String,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct FundEscrow {
    pub repo_id: Uuid,
    pub amount: Decimal,
    pub funder_wallet: String,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SubmitFund {
    pub repo_id: Uuid,
    pub amount: Decimal,
    pub signed_xdr: String,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RefundEscrow {
    pub repo_id: Uuid,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CloseEscrow {
    pub repo_id: Uuid,
    pub maintainer_wallet: String,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SubmitClose {
    pub repo_id: Uuid,
    pub signed_xdr: String,
}

// Responce DTO

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
