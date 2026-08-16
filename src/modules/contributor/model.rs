//! Contributor module API types.
//!
//! Database entities live in [`crate::shared::models::Contributor`].

use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectWalletBody {
    pub wallet: String,
    pub payout_chain: Option<String>,
    pub payout_address: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct OkResponse {
    pub ok: bool,
}

#[derive(Debug, Serialize)]
pub struct ContributorMeResponse {
    pub contributor: Option<serde_json::Value>,
}
