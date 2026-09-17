use serde_json::Value;

use crate::{error::AppError, shared::constants::TESTNET_USDC, state::AppState};

fn horizon_base_url(state: &AppState) -> &'static str {
    if state.config.is_mainnet() {
        "https://horizon.stellar.org"
    } else {
        "https://horizon-testnet.stellar.org"
    }
}

fn usdc_issuer(state: &AppState) -> &str {
    let configured = state.config.token_address.trim();
    if configured.is_empty() {
        TESTNET_USDC
    } else {
        configured
    }
}

// [ryzen-xp] : Require funded Stellar account + USDC trustline before TW payout
pub async fn require_usdc_payout_account(state: &AppState, address: &str) -> Result<(), AppError> {
    let address = address.trim();
    if address.is_empty() {
        return Err(AppError::bad_request("Payout address is empty"));
    }
    if !address.starts_with('G') || address.len() != 56 {
        return Err(AppError::bad_request(format!(
            "Payout address {address} is not a valid Stellar public key"
        )));
    }

    let url = format!("{}/accounts/{}", horizon_base_url(state), address);
    let response =
        state.http_client.get(&url).send().await.map_err(|error| {
            AppError::internal(format!("Horizon account lookup failed: {error}"))
        })?;

    let status = response.status();
    let body = response
        .text()
        .await
        .map_err(|error| AppError::internal(format!("Horizon response read failed: {error}")))?;

    if status.as_u16() == 404 {
        let network = if state.config.is_mainnet() {
            "mainnet"
        } else {
            "testnet"
        };
        return Err(AppError::bad_request(format!(
            "Payout wallet {address} does not exist on Stellar {network}. \
             Create/fund that account and add a USDC trustline before merge, then reconnect the wallet."
        )));
    }

    if !status.is_success() {
        return Err(AppError::internal(format!(
            "Horizon account lookup for {address} failed ({status}): {body}"
        )));
    }

    let payload: Value = serde_json::from_str(&body).map_err(|error| {
        AppError::internal(format!("Horizon account JSON parse failed: {error}"))
    })?;

    let issuer = usdc_issuer(state);
    let has_usdc = payload
        .get("balances")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .any(|balance| {
            balance.get("asset_code").and_then(Value::as_str) == Some("USDC")
                && balance.get("asset_issuer").and_then(Value::as_str) == Some(issuer)
        });

    if !has_usdc {
        let network = if state.config.is_mainnet() {
            "mainnet"
        } else {
            "testnet"
        };
        return Err(AppError::bad_request(format!(
            "Payout wallet {address} exists on Stellar {network} but has no USDC trustline \
             for issuer {issuer}. Add the USDC trustline, then reconnect the wallet / retry payout."
        )));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn stellar_address_shape() {
        assert_eq!(
            "GDTNILC7HOK4QGBBUPYLWFDOCW66H5BAIL374PJFIS7SZKHR4QJCH43D".len(),
            56
        );
    }
}
