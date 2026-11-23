//! Private core-selected destinations and source budgets, never a public wallet
//! RPC. Parsing these values does not authenticate their sender or authorize a melt.
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

pub const PURPOSE: &str = "cashu_treasury_settlement";
pub const MAX_SOURCES: usize = 128;
pub const MAX_TREASURY_BYTES: usize = 65_536;

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Destination {
    pub origin: String,
    pub account_id: Uuid,
    pub credential_fingerprint: String,
    pub network: String,
}
impl Destination {
    pub fn validate(&self) -> Result<()> {
        let url = url::Url::parse(&self.origin)?;
        ensure!(
            self.origin.len() <= 256
                && url.path() == "/"
                && url.as_str() == self.origin
                && url.username().is_empty()
                && url.password().is_none()
                && url.query().is_none()
                && url.fragment().is_none()
                && url.host_str().is_some()
                && (url.scheme() == "https"
                    || (url.scheme() == "http"
                        && url.host_str().is_some_and(|host| host
                            .parse::<std::net::IpAddr>()
                            .is_ok_and(|ip| ip.is_loopback())))),
            "invalid treasury provider origin"
        );
        ensure!(
            !self.account_id.is_nil() && self.network == "bitcoin",
            "invalid treasury destination scope"
        );
        lightning_goats_nostr_event::validate_hex(&self.credential_fingerprint, 32)?;
        Ok(())
    }
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Source {
    pub ledger_id: i64,
    pub acquisition_id: Uuid,
    pub receipt_digest: String,
    pub input_sats: u64,
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Intent {
    pub version: u32,
    pub purpose: String,
    pub id: Uuid,
    pub core_epoch: Uuid,
    pub wallet_instance: Uuid,
    pub wallet_epoch: Uuid,
    pub mint_id: String,
    pub treasury_policy: String,
    pub destination: Destination,
    pub input_sats: u64,
    pub deposit_sats: u64,
    pub maximum_fee_sats: u64,
    pub invoice_expiry_seconds: u64,
    pub sources: Vec<Source>,
}
impl Intent {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.version == 1
                && self.purpose == PURPOSE
                && !self.id.is_nil()
                && !self.core_epoch.is_nil()
                && !self.wallet_instance.is_nil()
                && !self.wallet_epoch.is_nil(),
            "invalid treasury identity/purpose"
        );
        self.destination.validate()?;
        for value in [&self.mint_id, &self.treasury_policy] {
            ensure!(
                !value.is_empty() && value.len() <= 128 && !value.chars().any(char::is_control),
                "invalid treasury policy"
            );
        }
        ensure!(
            self.deposit_sats > 0
                && self.deposit_sats <= i64::MAX as u64 / 1000
                && self.input_sats <= i64::MAX as u64
                && self.deposit_sats <= self.input_sats
                && self.maximum_fee_sats <= self.input_sats - self.deposit_sats
                && (1..=3600).contains(&self.invoice_expiry_seconds),
            "unaffordable treasury invoice/fee authority"
        );
        ensure!(
            !self.sources.is_empty() && self.sources.len() <= MAX_SOURCES,
            "invalid treasury source bound"
        );
        let mut previous = 0;
        let mut total = 0u64;
        let mut acquisitions = std::collections::BTreeSet::new();
        for source in &self.sources {
            ensure!(
                source.ledger_id > previous
                    && !source.acquisition_id.is_nil()
                    && source.input_sats > 0
                    && acquisitions.insert(source.acquisition_id),
                "invalid or duplicate treasury source"
            );
            lightning_goats_nostr_event::validate_hex(&source.receipt_digest, 32)?;
            total = total
                .checked_add(source.input_sats)
                .ok_or_else(|| anyhow::anyhow!("treasury input overflow"))?;
            previous = source.ledger_id;
        }
        ensure!(
            total == self.input_sats,
            "treasury source conservation failure"
        );
        Ok(())
    }
    /// Allocate a verified deposit, actual fees and recovered change in frozen
    /// ledger order. This arithmetic does not establish payment or ownership.
    /// Deposit consumes the earliest sources first, then fees; remaining value
    /// stays attributed to its original source as change.
    pub fn allocate_result(&self, fee_sats: u64, change_sats: u64) -> Result<Vec<SourceResult>> {
        self.validate()?;
        ensure!(
            fee_sats <= self.maximum_fee_sats
                && self
                    .deposit_sats
                    .checked_add(fee_sats)
                    .and_then(|n| n.checked_add(change_sats))
                    == Some(self.input_sats),
            "unverified or inconsistent treasury conservation"
        );
        let mut deposit_left = self.deposit_sats;
        let mut fee_left = fee_sats;
        let mut result = Vec::with_capacity(self.sources.len());
        for source in &self.sources {
            let deposit = source.input_sats.min(deposit_left);
            deposit_left -= deposit;
            let fee = (source.input_sats - deposit).min(fee_left);
            fee_left -= fee;
            result.push(SourceResult {
                ledger_id: source.ledger_id,
                input_sats: source.input_sats,
                deposit_sats: deposit,
                fee_sats: fee,
                change_sats: source.input_sats - deposit - fee,
            });
        }
        ensure!(
            deposit_left == 0 && fee_left == 0,
            "treasury allocation incomplete"
        );
        Ok(result)
    }
    pub fn digest(&self) -> Result<[u8; 32]> {
        self.validate()?;
        Ok(Sha256::digest(serde_json::to_vec(&(
            "lightning-goats/cashu-treasury/v1",
            self,
        ))?)
        .into())
    }
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Issued {
    pub intent: Intent,
    pub receive_request_id: Uuid,
    pub payment_hash: String,
    pub invoice: String,
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SourceResult {
    pub ledger_id: i64,
    pub input_sats: u64,
    pub deposit_sats: u64,
    pub fee_sats: u64,
    pub change_sats: u64,
}

/// A trusted wallet's terminal ownership report, not mint/Strike authority by
/// itself. The core additionally authenticates the peer and its own deposit.
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct MeltData {
    pub version: u32,
    pub intent_id: Uuid,
    pub intent_digest: String,
    pub quote_id: String,
    pub operation_evidence_digest: String,
    pub sources: Vec<SourceResult>,
}
impl MeltData {
    pub fn validate(&self, intent: &Intent) -> Result<()> {
        ensure!(
            self.version == 1
                && self.intent_id == intent.id
                && self.intent_digest == hex_digest(intent.digest()?)
                && !self.quote_id.is_empty()
                && self.quote_id.len() <= 256
                && !self.quote_id.chars().any(char::is_control),
            "melt report intent/quote mismatch"
        );
        lightning_goats_nostr_event::validate_hex(&self.operation_evidence_digest, 32)?;
        ensure!(
            self.sources.len() == intent.sources.len(),
            "melt source count mismatch"
        );
        let mut totals = [0u64; 3];
        for (actual, expected) in self.sources.iter().zip(&intent.sources) {
            ensure!(
                actual.ledger_id == expected.ledger_id
                    && actual.input_sats == expected.input_sats
                    && actual
                        .deposit_sats
                        .checked_add(actual.fee_sats)
                        .and_then(|n| n.checked_add(actual.change_sats))
                        == Some(actual.input_sats),
                "melt source conservation mismatch"
            );
            for (total, n) in
                totals
                    .iter_mut()
                    .zip([actual.deposit_sats, actual.fee_sats, actual.change_sats])
            {
                *total = total
                    .checked_add(n)
                    .ok_or_else(|| anyhow::anyhow!("melt total overflow"))?;
            }
        }
        ensure!(
            totals[0] == intent.deposit_sats
                && totals[1] <= intent.maximum_fee_sats
                && totals[0]
                    .checked_add(totals[1])
                    .and_then(|n| n.checked_add(totals[2]))
                    == Some(intent.input_sats),
            "melt report exceeds frozen deposit/fee authority"
        );
        ensure!(
            self.sources == intent.allocate_result(totals[1], totals[2])?,
            "melt report changes deterministic source allocation"
        );
        Ok(())
    }
    pub fn digest(&self) -> Result<[u8; 32]> {
        Ok(Sha256::digest(serde_json::to_vec(&(
            "lightning-goats/cashu-melt-result/v1",
            self,
        ))?)
        .into())
    }
}

pub fn hex_digest(digest: [u8; 32]) -> String {
    // No cryptography or protocol encoding is implemented here; this is the
    // fixed hexadecimal representation of a SHA-256 digest for private JSON.
    use std::fmt::Write as _;
    let mut encoded = String::with_capacity(64);
    for byte in digest {
        // Formatting into an owned String has no fallible backing writer.
        let _ = write!(&mut encoded, "{byte:02x}");
    }
    encoded
}

#[cfg(test)]
mod allocation_tests {
    use super::*;

    fn intent() -> Intent {
        Intent {
            version: 1,
            purpose: PURPOSE.into(),
            id: Uuid::from_u128(1),
            core_epoch: Uuid::from_u128(2),
            wallet_instance: Uuid::from_u128(3),
            wallet_epoch: Uuid::from_u128(4),
            mint_id: "fixture".into(),
            treasury_policy: "fixture-v1".into(),
            destination: Destination {
                origin: "https://api.example.com/".into(),
                account_id: Uuid::from_u128(5),
                credential_fingerprint: "ab".repeat(32),
                network: "bitcoin".into(),
            },
            input_sats: 10,
            deposit_sats: 6,
            maximum_fee_sats: 2,
            invoice_expiry_seconds: 300,
            sources: vec![
                Source {
                    ledger_id: 1,
                    acquisition_id: Uuid::from_u128(6),
                    receipt_digest: "cd".repeat(32),
                    input_sats: 7,
                },
                Source {
                    ledger_id: 2,
                    acquisition_id: Uuid::from_u128(7),
                    receipt_digest: "ef".repeat(32),
                    input_sats: 3,
                },
            ],
        }
    }

    #[test]
    fn conservation_does_not_allow_reassigning_deposit_coverage_between_donors() {
        let intent = intent();
        let mut report = MeltData {
            version: 1,
            intent_id: intent.id,
            intent_digest: hex_digest(intent.digest().unwrap()),
            quote_id: "quote".into(),
            operation_evidence_digest: "aa".repeat(32),
            sources: intent.allocate_result(2, 2).unwrap(),
        };
        assert_eq!(report.sources[0].deposit_sats, 6);
        assert_eq!(report.sources[0].fee_sats, 1);
        assert_eq!(report.sources[1].change_sats, 2);
        report.validate(&intent).unwrap();
        // Both per-source and total equations still balance, but this would
        // assign coverage to a different donor and change later feed selection.
        report.sources[0].deposit_sats -= 1;
        report.sources[0].change_sats += 1;
        report.sources[1].deposit_sats += 1;
        report.sources[1].change_sats -= 1;
        assert!(report.validate(&intent).is_err());
    }

    #[test]
    fn every_small_fee_change_partition_is_checked_without_rounding_or_writeoff() {
        let mut intent = intent();
        for deposit in 1..=10 {
            intent.deposit_sats = deposit;
            intent.maximum_fee_sats = 10 - deposit;
            for fee in 0..=10 - deposit {
                let change = 10 - deposit - fee;
                let result = intent.allocate_result(fee, change).unwrap();
                assert_eq!(result.iter().map(|s| s.deposit_sats).sum::<u64>(), deposit);
                assert_eq!(result.iter().map(|s| s.fee_sats).sum::<u64>(), fee);
                assert_eq!(result.iter().map(|s| s.change_sats).sum::<u64>(), change);
                assert!(
                    result
                        .iter()
                        .all(|s| s.deposit_sats + s.fee_sats + s.change_sats == s.input_sats)
                );
                assert!(intent.allocate_result(fee, change + 1).is_err());
            }
        }
        assert!(intent.allocate_result(u64::MAX, 0).is_err());
        assert!(intent.allocate_result(0, u64::MAX).is_err());
    }
}
