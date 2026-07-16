use serde_json::Value;
use stellar_base::{crypto::DalekKeyPair, network::Network};
use stellar_xdr::{
    DecoratedSignature, Hash, Limits, MuxedAccount, Preconditions, ReadXdr, Signature,
    SignatureHint, Transaction, TransactionEnvelope, TransactionExt, TransactionSignaturePayload,
    TransactionSignaturePayloadTaggedTransaction, TransactionV0, VecM, WriteXdr,
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
    let signed_xdr = sign_xdr(unsigned_xdr, &keypair, &network)?;

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

fn sign_xdr(
    unsigned_xdr: &str,
    keypair: &DalekKeyPair,
    network: &Network,
) -> Result<String, AppError> {
    let mut envelope = TransactionEnvelope::from_xdr_base64(unsigned_xdr.trim(), Limits::none())
        .map_err(|error| AppError::stellar(format!("invalid unsigned XDR: {error}")))?;

    let payload = match &envelope {
        TransactionEnvelope::Tx(envelope) => TransactionSignaturePayload {
            network_id: network_hash(network)?,
            tagged_transaction: TransactionSignaturePayloadTaggedTransaction::Tx(
                envelope.tx.clone(),
            ),
        },
        TransactionEnvelope::TxV0(envelope) => TransactionSignaturePayload {
            network_id: network_hash(network)?,
            tagged_transaction: TransactionSignaturePayloadTaggedTransaction::Tx(
                transaction_from_v0(&envelope.tx),
            ),
        },
        TransactionEnvelope::TxFeeBump(envelope) => TransactionSignaturePayload {
            network_id: network_hash(network)?,
            tagged_transaction: TransactionSignaturePayloadTaggedTransaction::TxFeeBump(
                envelope.tx.clone(),
            ),
        },
    };

    let payload_xdr = payload
        .to_xdr(Limits::none())
        .map_err(|error| AppError::stellar(format!("failed to encode signing payload: {error}")))?;
    let signature = keypair.sign(&stellar_base::crypto::hash(&payload_xdr));
    let decorated_signature = DecoratedSignature {
        hint: signature_hint(keypair),
        signature: Signature::try_from(signature.to_bytes().to_vec())
            .map_err(|error| AppError::stellar(format!("failed to encode signature: {error}")))?,
    };

    match &mut envelope {
        TransactionEnvelope::Tx(envelope) => {
            append_signature(&mut envelope.signatures, decorated_signature)?;
        }
        TransactionEnvelope::TxV0(envelope) => {
            append_signature(&mut envelope.signatures, decorated_signature)?;
        }
        TransactionEnvelope::TxFeeBump(envelope) => {
            append_signature(&mut envelope.signatures, decorated_signature)?;
        }
    }

    envelope
        .to_xdr_base64(Limits::none())
        .map_err(|error| AppError::stellar(format!("failed to encode signed XDR: {error}")))
}

fn network_hash(network: &Network) -> Result<Hash, AppError> {
    network
        .network_id()
        .try_into()
        .map(Hash)
        .map_err(|_| AppError::stellar("invalid Stellar network hash"))
}

fn signature_hint(keypair: &DalekKeyPair) -> SignatureHint {
    let bytes = keypair.public_key().as_bytes().to_owned();
    SignatureHint::from([bytes[28], bytes[29], bytes[30], bytes[31]])
}

fn transaction_from_v0(transaction: &TransactionV0) -> Transaction {
    Transaction {
        source_account: MuxedAccount::Ed25519(transaction.source_account_ed25519.clone()),
        fee: transaction.fee,
        seq_num: transaction.seq_num.clone(),
        cond: transaction
            .time_bounds
            .clone()
            .map(Preconditions::Time)
            .unwrap_or(Preconditions::None),
        memo: transaction.memo.clone(),
        operations: transaction.operations.clone(),
        ext: TransactionExt::V0,
    }
}

fn append_signature(
    signatures: &mut VecM<DecoratedSignature, 20>,
    signature: DecoratedSignature,
) -> Result<(), AppError> {
    let mut values = signatures.to_vec();
    values.push(signature);
    *signatures = values
        .try_into()
        .map_err(|_| AppError::stellar("transaction has too many signatures"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signs_a_current_transaction_envelope() {
        let unsigned_xdr = "AAAAAgAAAACITTAVWHY+p9yczx3PmcK4HKSYQ8nysNxoLnxusInt3gAAAGQAAAAAAAAAewAAAAEAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAEAAAAAAAAABQAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
        let keypair = DalekKeyPair::from_secret_seed(
            "SD7X7LEHBNMUIKQGKPARG5TDJNBHKC346OUARHGZL5ITC6IJPXHILY36",
        )
        .expect("valid test secret");

        let signed_xdr = sign_xdr(unsigned_xdr, &keypair, &Network::new_test())
            .expect("transaction should be signed");
        let signed = TransactionEnvelope::from_xdr_base64(&signed_xdr, Limits::none())
            .expect("signed transaction should remain valid XDR");

        match signed {
            TransactionEnvelope::Tx(envelope) => assert_eq!(envelope.signatures.len(), 1),
            _ => panic!("expected a regular transaction envelope"),
        }
    }
}
