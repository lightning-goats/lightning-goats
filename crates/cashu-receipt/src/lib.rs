#![forbid(unsafe_code)]
//! Versioned private receipt data. Structural validation is not payment authority:
//! core import must additionally authenticate the wallet peer, epoch and limits.
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

pub mod acknowledgment;
pub mod ingress;
pub mod public_request;
pub mod receive_status;
pub mod treasury;
pub mod treasury_rpc;

pub const MAX_RECEIPT_BYTES: usize = 8192;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReceiptData {
    pub schema_version: u32,
    pub wallet_instance: Uuid,
    pub wallet_epoch: Uuid,
    pub acquisition_id: Uuid,
    pub transport: String,
    pub source_ref: String,
    pub source_author: Option<String>,
    pub mint_id: String,
    pub unit: String,
    pub gross_sats: u64,
    pub receive_fee_sats: u64,
    pub net_sats: u64,
    pub acquisition_evidence_digest: String,
    pub policy_version: String,
    pub credit_pool: String,
}

/// Immutable validated structure. Deliberately no Debug or Deserialize implementation.
#[derive(Clone)]
pub struct WireReceipt(ReceiptData);
impl WireReceipt {
    pub fn decode(bytes: &[u8]) -> Result<Self> {
        ensure!(
            bytes.len() <= MAX_RECEIPT_BYTES,
            "receipt exceeds message bound"
        );
        let data: ReceiptData = serde_json::from_slice(bytes)
            .map_err(|_| anyhow::anyhow!("invalid receipt message"))?;
        Self::validate(data)
    }
    pub fn validate(data: ReceiptData) -> Result<Self> {
        ensure!(data.schema_version == 1, "unsupported receipt schema");
        ensure!(
            !data.wallet_instance.is_nil()
                && !data.wallet_epoch.is_nil()
                && !data.acquisition_id.is_nil(),
            "receipt identity missing"
        );
        ensure!(
            data.unit == "sat" && data.credit_pool == "herd",
            "unsupported receipt destination/unit"
        );
        ensure!(
            data.net_sats > 0
                && data.gross_sats <= i64::MAX as u64
                && data.net_sats.checked_add(data.receive_fee_sats) == Some(data.gross_sats),
            "receipt conservation failure"
        );
        for value in [&data.mint_id, &data.policy_version] {
            ensure!(
                !value.is_empty() && value.len() <= 128 && !value.chars().any(char::is_control),
                "invalid receipt policy binding"
            );
        }
        ensure!(
            !data.source_ref.is_empty()
                && data.source_ref.len() <= 256
                && !data.source_ref.chars().any(char::is_control),
            "invalid receipt source reference"
        );
        lightning_goats_nostr_event::validate_hex(&data.acquisition_evidence_digest, 32)?;
        match data.transport.as_str() {
            "cashu" => ensure!(
                data.source_author.is_none(),
                "ordinary Cashu cannot claim a Nostr author"
            ),
            "nutzap" => {
                lightning_goats_nostr_event::validate_hex(&data.source_ref, 32)?;
                let author = data
                    .source_author
                    .as_deref()
                    .ok_or_else(|| anyhow::anyhow!("nutzap author missing"))?;
                lightning_goats_nostr_event::validate_public_key(author)?;
            }
            _ => anyhow::bail!("unsupported receipt transport"),
        }
        Ok(Self(data))
    }
    pub fn data(&self) -> &ReceiptData {
        &self.0
    }
    pub fn encode(&self) -> Result<Vec<u8>> {
        let bytes = serde_json::to_vec(&self.0)?;
        ensure!(
            bytes.len() <= MAX_RECEIPT_BYTES,
            "receipt exceeds message bound"
        );
        Ok(bytes)
    }
    /// Same logical receipt has the same digest despite JSON field order on the wire.
    pub fn digest(&self) -> Result<[u8; 32]> {
        let canonical = serde_json::to_vec(&("lightning-goats/cashu-receipt/v1", &self.0))?;
        Ok(Sha256::digest(canonical).into())
    }
}
