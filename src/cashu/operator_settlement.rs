//! Root-reviewed local planning scope. Never accepts invoices or provider credentials.
use crate::{
    cashu::{receipt_authority::ReceiptPolicy, treasury_sweep},
    config::AppConfig,
};
use anyhow::{Result, ensure};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{io::Read, path::Path};
use uuid::Uuid;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Approval {
    pub version: u8,
    pub core_epoch: Uuid,
    pub wallet_instance: Uuid,
    pub wallet_epoch: Uuid,
    pub config_sha256: String,
    pub request: treasury_sweep::Request,
    pub reviewed_settlement_planning: bool,
    pub approved_at: i64,
    pub expires_at: i64,
}

impl Approval {
    pub fn load(path: &Path) -> Result<Self> {
        let decision: Self = load_approval(path)?;
        decision.require_current()?;
        Ok(decision)
    }
    pub fn require_current(&self) -> Result<()> {
        let now = crate::gateway::now_epoch()?;
        ensure!(
            self.version == 1
                && !self.core_epoch.is_nil()
                && !self.wallet_instance.is_nil()
                && !self.wallet_epoch.is_nil()
                && self.reviewed_settlement_planning
                && self.config_sha256.len() == 64
                && self
                    .config_sha256
                    .bytes()
                    .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
                && self.approved_at >= 0
                && self.expires_at > self.approved_at
                && self
                    .expires_at
                    .checked_sub(self.approved_at)
                    .is_some_and(|n| n <= 86400)
                && now >= self.approved_at
                && now <= self.expires_at,
            "settlement approval is not current or valid"
        );
        treasury_sweep::Policy {
            enabled: true,
            requests: vec![self.request.clone()],
            ..Default::default()
        }
        .validate_shape()
    }
    /// Hash and parse the same bounded bytes; never reopen a second config image.
    pub fn load_config(&self, path: &Path) -> Result<AppConfig> {
        load_config(path, &self.config_sha256)
    }
    pub fn require_registration(&self, registration: &ReceiptPolicy) -> Result<()> {
        ensure!(
            self.core_epoch == registration.core_epoch
                && self.wallet_instance == registration.wallet_instance
                && self.wallet_epoch == registration.wallet_epoch,
            "settlement approval wallet/core registration mismatch"
        );
        Ok(())
    }
}

pub(super) fn load_approval<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T> {
    crate::private_ipc::protected_config(path)?;
    let mut bytes = Vec::new();
    std::fs::File::open(path)?
        .take(4097)
        .read_to_end(&mut bytes)?;
    ensure!(
        !bytes.is_empty() && bytes.len() <= 4096,
        "settlement approval exceeds bound"
    );
    serde_json::from_slice(&bytes).map_err(|_| anyhow::anyhow!("invalid settlement approval"))
}

pub(super) fn load_config(path: &Path, expected_digest: &str) -> Result<AppConfig> {
    crate::private_ipc::protected_config(path)?;
    let mut bytes = Vec::new();
    std::fs::File::open(path)?
        .take(262145)
        .read_to_end(&mut bytes)?;
    ensure!(
        !bytes.is_empty()
            && bytes.len() <= 262144
            && hex::encode(Sha256::digest(&bytes)) == expected_digest,
        "protected configuration differs from settlement approval"
    );
    let config: AppConfig = toml::from_str(std::str::from_utf8(&bytes)?)
        .map_err(|_| anyhow::anyhow!("invalid protected settlement configuration"))?;
    config.validate()?;
    Ok(config)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn example() -> serde_json::Value {
        let now = crate::gateway::now_epoch().unwrap();
        serde_json::json!({"version":1,"core_epoch":Uuid::new_v4(),"wallet_instance":Uuid::new_v4(),
            "wallet_epoch":Uuid::new_v4(),"config_sha256":"a".repeat(64),
            "request":{"id":Uuid::new_v4(),"mint_id":"mint-a","maximum_input_sats":200},
            "reviewed_settlement_planning":true,"approved_at":now,"expires_at":now+300})
    }
    #[test]
    fn decision_requires_review_identity_time_and_bounded_request() {
        let good = example();
        assert!(
            serde_json::from_value::<Approval>(good.clone())
                .unwrap()
                .require_current()
                .is_ok()
        );
        for (field, value) in [
            ("version", serde_json::json!(2)),
            ("core_epoch", serde_json::json!(Uuid::nil())),
            ("wallet_instance", serde_json::json!(Uuid::nil())),
            ("wallet_epoch", serde_json::json!(Uuid::nil())),
            ("config_sha256", serde_json::json!("G".repeat(64))),
            ("reviewed_settlement_planning", serde_json::json!(false)),
            ("expires_at", serde_json::json!(0)),
            ("approved_at", serde_json::json!(i64::MAX)),
        ] {
            let mut value_row = good.clone();
            value_row[field] = value;
            assert!(
                serde_json::from_value::<Approval>(value_row)
                    .unwrap()
                    .require_current()
                    .is_err()
            );
        }
        for request in [
            serde_json::json!({"id":Uuid::nil(),"mint_id":"mint-a","maximum_input_sats":200}),
            serde_json::json!({"id":Uuid::new_v4(),"mint_id":"mint-a","maximum_input_sats":0}),
        ] {
            let mut value = good.clone();
            value["request"] = request;
            assert!(
                serde_json::from_value::<Approval>(value)
                    .unwrap()
                    .require_current()
                    .is_err()
            );
        }
        for field in ["invoice", "destination", "payment_hash"] {
            let mut value = good.clone();
            value[field] = serde_json::json!("forbidden");
            assert!(serde_json::from_value::<Approval>(value).is_err());
        }
    }
}
