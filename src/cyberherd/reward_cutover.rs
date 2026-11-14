//! Reward-only historical disposition. The offline operator boundary must derive
//! this inventory from a verified, quiesced core backup; relay input is not authority.
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use uuid::Uuid;

pub(crate) const COMMAND: &str = "initial-reward-cutover";
pub(crate) const PREFIX: &str = "initial-reward-cutover-feed:";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct HistoricalFeeding {
    pub request_id: Uuid,
    pub core_seq: u64,
    pub proof_hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Decision {
    pub id: Uuid,
    pub epoch: Uuid,
    pub through: u64,
    pub backup_manifest_sha256: String,
    pub decided_at: i64,
    pub historical: Vec<HistoricalFeeding>,
}
impl Decision {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            !self.id.is_nil() && !self.epoch.is_nil(),
            "cutover identity required"
        );
        ensure!(
            self.through <= i64::MAX as u64 && self.decided_at >= 0,
            "invalid cutover boundary"
        );
        ensure!(
            self.historical.len() <= 16384,
            "historical inventory exceeds cutover bound"
        );
        crate::nostr_event::validate_hex(&self.backup_manifest_sha256, 32)?;
        let mut previous = 0;
        let mut ids = BTreeSet::new();
        for item in &self.historical {
            ensure!(
                !item.request_id.is_nil() && ids.insert(item.request_id),
                "duplicate or nil historical feeding"
            );
            ensure!(
                item.core_seq > previous && item.core_seq <= self.through,
                "unordered or future historical feeding"
            );
            crate::nostr_event::validate_hex(&item.proof_hash, 32)?;
            previous = item.core_seq;
        }
        ensure!(
            serde_json::to_vec(self)?.len() <= 4 * 1024 * 1024,
            "cutover record exceeds bound"
        );
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Receipt {
    pub decision_id: Uuid,
    pub epoch: Uuid,
    pub through: u64,
    pub historical_feedings: usize,
    pub activation_authorized: bool,
}
impl Decision {
    pub(crate) fn receipt(&self) -> Receipt {
        Receipt {
            decision_id: self.id,
            epoch: self.epoch,
            through: self.through,
            historical_feedings: self.historical.len(),
            activation_authorized: false,
        }
    }
}
