//! Core acknowledgment of one immutable receipt. Transport must authenticate the core UID.
use crate::WireReceipt;
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub const MAX_ACK_BYTES: usize = 4096;

/// Preserve the core's original result representation across duplicate imports.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImportResult {
    pub acquisition_id: Uuid,
    pub receipt_digest: String,
    pub event_seq: u64,
    pub net_sats: u64,
    /// The core reserved this receipt for project Cashu custody when it granted credit.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub retained_cashu: bool,
    /// Original total pool credit after import, not this receipt's contribution.
    pub original_feed_credit_sats: u64,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Acknowledgment {
    pub version: u8,
    pub wallet_instance: Uuid,
    pub wallet_epoch: Uuid,
    pub core_epoch: Uuid,
    pub result: ImportResult,
}
impl Acknowledgment {
    pub fn validate_for(&self, receipt: &WireReceipt, core_epoch: Uuid) -> Result<()> {
        let data = receipt.data();
        let digest = receipt
            .digest()?
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        ensure!(
            self.version == 1
                && !core_epoch.is_nil()
                && self.core_epoch == core_epoch
                && self.wallet_instance == data.wallet_instance
                && self.wallet_epoch == data.wallet_epoch
                && self.result.acquisition_id == data.acquisition_id
                && self.result.receipt_digest == digest
                && self.result.net_sats == data.net_sats
                && self.result.event_seq > 0
                && self.result.event_seq <= i64::MAX as u64
                && self.result.original_feed_credit_sats <= i64::MAX as u64,
            "core acknowledgment does not match receipt and registration"
        );
        Ok(())
    }
    pub fn decode_for(bytes: &[u8], receipt: &WireReceipt, core_epoch: Uuid) -> Result<Self> {
        ensure!(
            !bytes.is_empty() && bytes.len() <= MAX_ACK_BYTES,
            "invalid acknowledgment length"
        );
        let ack: Self = serde_json::from_slice(bytes)
            .map_err(|_| anyhow::anyhow!("invalid acknowledgment message"))?;
        ack.validate_for(receipt, core_epoch)?;
        Ok(ack)
    }
    pub fn encode(&self) -> Result<Vec<u8>> {
        let bytes = serde_json::to_vec(self)?;
        ensure!(bytes.len() <= MAX_ACK_BYTES, "acknowledgment exceeds bound");
        Ok(bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ReceiptData;
    #[test]
    fn acknowledgment_binds_original_receipt_and_both_epochs() -> Result<()> {
        let receipt = WireReceipt::validate(ReceiptData {
            schema_version: 1,
            wallet_instance: Uuid::new_v4(),
            wallet_epoch: Uuid::new_v4(),
            acquisition_id: Uuid::new_v4(),
            transport: "cashu".into(),
            source_ref: "fixture".into(),
            source_author: None,
            mint_id: "fixture".into(),
            unit: "sat".into(),
            gross_sats: 8,
            receive_fee_sats: 1,
            net_sats: 7,
            acquisition_evidence_digest: "12".repeat(32),
            policy_version: "fixture-v1".into(),
            credit_pool: "herd".into(),
        })?;
        let epoch = Uuid::new_v4();
        let ack = Acknowledgment {
            version: 1,
            wallet_instance: receipt.data().wallet_instance,
            wallet_epoch: receipt.data().wallet_epoch,
            core_epoch: epoch,
            result: ImportResult {
                retained_cashu: false,
                acquisition_id: receipt.data().acquisition_id,
                receipt_digest: receipt
                    .digest()?
                    .iter()
                    .map(|byte| format!("{byte:02x}"))
                    .collect(),
                event_seq: 42,
                net_sats: 7,
                original_feed_credit_sats: 500,
            },
        };
        let bytes = ack.encode()?;
        Acknowledgment::decode_for(&bytes, &receipt, epoch)?;
        let original: serde_json::Value = serde_json::from_slice(&bytes)?;
        for field in [
            "version",
            "wallet_instance",
            "wallet_epoch",
            "core_epoch",
            "acquisition_id",
            "receipt_digest",
            "event_seq",
            "net_sats",
            "original_feed_credit_sats",
        ] {
            let mut value = original.clone();
            match field {
                "version" => value[field] = 2.into(),
                "wallet_instance" | "wallet_epoch" | "core_epoch" => {
                    value[field] = Uuid::new_v4().to_string().into()
                }
                "acquisition_id" => value["result"][field] = Uuid::new_v4().to_string().into(),
                "receipt_digest" => value["result"][field] = "ff".repeat(32).into(),
                "original_feed_credit_sats" => value["result"][field] = u64::MAX.into(),
                _ => value["result"][field] = 0.into(),
            }
            assert!(
                Acknowledgment::decode_for(&serde_json::to_vec(&value)?, &receipt, epoch).is_err(),
                "accepted {field}"
            );
        }
        assert!(Acknowledgment::decode_for(&vec![0; MAX_ACK_BYTES + 1], &receipt, epoch).is_err());
        assert!(Acknowledgment::decode_for(&bytes, &receipt, Uuid::nil()).is_err());
        let mut value = original;
        value["proofs"] = serde_json::json!([]);
        assert!(Acknowledgment::decode_for(&serde_json::to_vec(&value)?, &receipt, epoch).is_err());
        Ok(())
    }
}
