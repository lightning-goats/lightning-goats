//! Exact, replayable terms shared by the payout journal, core and collector.
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Execution {
    pub allocation: crate::reward_funding::OperatorAllocation,
    pub address: String,
    pub invoice: String,
    pub payment_hash: String,
    pub amount_sat: u64,
    pub maximum_fee_sat: u64,
    pub lots: Vec<super::funding::RetainedAllocation>,
}

impl Execution {
    pub fn validate(&self) -> Result<u64> {
        let a = &self.allocation;
        ensure!(
            self.address == "sat@lightning-goats.com"
                && !a.request_id.is_nil()
                && a.collected_sats.checked_sub(a.member_reserved_sats) == Some(a.operator_sats)
                && self.amount_sat > 0
                && self.amount_sat.checked_add(self.maximum_fee_sat) == Some(a.operator_sats)
                && self.invoice.len() <= 8192
                && !self.lots.is_empty(),
            "invalid operator execution terms"
        );
        crate::nostr_event::validate_hex(&a.plan_hash, 32)?;
        let invoice: lightning_invoice::Bolt11Invoice =
            self.invoice.parse().context("invalid operator invoice")?;
        let description = match invoice.description() {
            lightning_invoice::Bolt11InvoiceDescriptionRef::Hash(h) => h.0.to_string(),
            lightning_invoice::Bolt11InvoiceDescriptionRef::Direct(s) => {
                hex::encode(Sha256::digest(s.to_string().as_bytes()))
            }
        };
        let verified = crate::bolt11::verify_payout(
            &self.invoice,
            self.amount_sat
                .checked_mul(1000)
                .context("operator amount overflow")?,
            &self.payment_hash,
            &description,
        )
        .map_err(|_| anyhow::anyhow!("operator invoice differs from frozen terms"))?;
        Ok(verified
            .expires_at()
            .context("invalid invoice expiry")?
            .as_secs())
    }

    pub fn digest(&self) -> Result<String> {
        Ok(hex::encode(Sha256::digest(serde_json::to_vec(self)?)))
    }

    pub fn payload(&self) -> serde_json::Value {
        let a = &self.allocation;
        serde_json::json!({"feed_id":a.request_id,"disposition_id":a.request_id,"invoice":self.invoice,
            "payment_hash":self.payment_hash,"amount_sat":self.amount_sat,"maximum_fee_sat":self.maximum_fee_sat,
            "retained_allocations":self.lots,"operator":{"plan_hash":a.plan_hash,"operator_sat":a.operator_sats,
            "member_reserved_sat":a.member_reserved_sats}})
    }
}
