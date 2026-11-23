//! Wallet projection into the shared private receipt protocol.
use crate::journal::Receipt;
use anyhow::Result;
pub use lightning_goats_cashu_receipt::{MAX_RECEIPT_BYTES, ReceiptData, WireReceipt};
use uuid::Uuid;

pub fn from_private(wallet_instance: Uuid, receipt: &Receipt) -> Result<WireReceipt> {
    WireReceipt::validate(ReceiptData {
        schema_version: receipt.schema_version,
        wallet_instance,
        wallet_epoch: receipt.wallet_epoch,
        acquisition_id: receipt.acquisition_id,
        transport: receipt.transport.clone(),
        source_ref: receipt.source_ref.clone(),
        source_author: receipt.source_author.clone(),
        mint_id: receipt.mint_id.clone(),
        unit: "sat".into(),
        gross_sats: receipt.gross_sats,
        receive_fee_sats: receipt.receive_fee_sats,
        net_sats: receipt.net_sats,
        acquisition_evidence_digest: cdk::util::hex::encode(&receipt.evidence_digest),
        policy_version: receipt.policy_version.clone(),
        credit_pool: receipt.credit_pool.into(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> Receipt {
        Receipt {
            schema_version: 1,
            wallet_epoch: Uuid::new_v4(),
            acquisition_id: Uuid::new_v4(),
            mint_id: "fixture".into(),
            policy_version: "fixture-v1".into(),
            transport: "cashu".into(),
            source_ref: "http-fixture".into(),
            source_author: None,
            gross_sats: 8,
            receive_fee_sats: 1,
            net_sats: 7,
            evidence_digest: vec![7; 32],
            credit_pool: "herd",
        }
    }
    #[test]
    fn receipt_is_bounded_canonical_and_contains_no_spendable_outputs() -> Result<()> {
        let receipt = fixture();
        let wire = from_private(Uuid::new_v4(), &receipt)?;
        let encoded = wire.encode()?;
        let value: serde_json::Value = serde_json::from_slice(&encoded)?;
        assert!(value.get("proofs").is_none() && value.get("secret").is_none());
        assert_eq!(
            wire.digest()?,
            WireReceipt::decode(&serde_json::to_vec_pretty(&value)?)?.digest()?
        );
        let mut changed = wire.data().clone();
        changed.net_sats = 6;
        changed.receive_fee_sats = 2;
        assert_ne!(wire.digest()?, WireReceipt::validate(changed)?.digest()?);
        Ok(())
    }
    #[test]
    fn invalid_scope_conflicts_and_fake_attribution_reject() -> Result<()> {
        let wire = from_private(Uuid::new_v4(), &fixture())?;
        for field in [
            "schema", "instance", "unit", "pool", "amount", "author", "digest",
        ] {
            let mut data = wire.data().clone();
            match field {
                "schema" => data.schema_version = 2,
                "instance" => data.wallet_instance = Uuid::nil(),
                "unit" => data.unit = "usd".into(),
                "pool" => data.credit_pool = "other".into(),
                "amount" => data.net_sats = u64::MAX,
                "author" => {
                    data.source_author = Some(
                        "79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798".into(),
                    )
                }
                _ => data.acquisition_evidence_digest = "00".into(),
            }
            assert!(WireReceipt::validate(data).is_err());
        }
        let mut extra: serde_json::Value = serde_json::from_slice(&wire.encode()?)?;
        extra["proofs"] = serde_json::json!([]);
        assert!(WireReceipt::decode(&serde_json::to_vec(&extra)?).is_err());
        assert!(WireReceipt::decode(&vec![b' '; MAX_RECEIPT_BYTES + 1]).is_err());
        Ok(())
    }
}
