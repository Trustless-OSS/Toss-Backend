use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ConnectWalletBody {
    pub wallet: String,
    pub payout_chain: Option<String>,
    pub payout_address: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct OkResponse {
    pub ok: bool,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ContributorMeResponse {
    #[schema(value_type = Option<serde_json::Value>)]
    pub contributor: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateProfileBody {
    pub full_name: Option<String>,
    pub bio: Option<String>,
    pub location: Option<String>,
    pub website: Option<String>,
    pub skills: Option<String>,
    pub telegram: Option<String>,
    pub discord: Option<String>,
    pub twitter: Option<String>,
}
