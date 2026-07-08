use serde_json::Value;
use stellar_base::{
    crypto::DalekKeyPair,
    network::Network,
    transaction::TransactionEnvelope,
    xdr::{XDRDeserialize, XDRSerialize},
};
use tracing::info;

use crate::{error::AppError, modules::escrow::trustless_work::client::tw_fetch, state::AppState};

pub async fn sign_and_send_transaction(
    state: &AppState,
    unsigned_xdr: &str,
    secret_override: Option<&str>,
) -> Result<Value, AppError> {
    let secret = secret_override
        .or(Some(state.config.platform_stellar_secret_key.as_str()))
        .ok_or_else(|| AppError::stellar("PLATFORM_STELLAR_SECRET_KEY is not configured"))?;

    let network = if state.config.is_mainnet() {
        Network::new_public()
    } else {
        Network::new_test()
    };

    let keypair = DalekKeyPair::from_secret_seed(secret)
        .map_err(|error| AppError::stellar(format!("invalid secret key: {error}")))?;

    let mut tx = TransactionEnvelope::from_xdr_base64(unsigned_xdr)
        .map_err(|error| AppError::stellar(format!("invalid unsigned XDR: {error}")))?;

    tx.sign(keypair.as_ref(), &network)
        .map_err(|error| AppError::stellar(format!("failed to sign transaction: {error}")))?;
    let signed_xdr = tx
        .xdr_base64()
        .map_err(|error| AppError::stellar(format!("failed to encode signed XDR: {error}")))?;

    let result = tw_fetch(
        state,
        "/helper/send-transaction",
        reqwest::Method::POST,
        Some(serde_json::json!({ "signedXdr": signed_xdr })),
    )
    .await?;

    info!(?result, "transaction submitted");
    Ok(result)
}
