//! One protected, original acquisition ownership check. No swap/melt replay authority.
use crate::{
    acquisition::{Coordinator, Permissions, Progress, Settings},
    journal::{HoldReason, Journal},
    service::Runtime,
    service_config::Config,
};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    io::Read,
    path::Path,
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use uuid::Uuid;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Approval {
    pub version: u8,
    pub wallet_instance: Uuid,
    pub wallet_epoch: Uuid,
    pub acquisition_id: Uuid,
    pub mint_id: String,
    pub config_sha256: String,
    pub reviewed_ownership_check: bool,
    #[serde(default)]
    pub reviewed_held_ownership_check: bool,
    #[serde(default)]
    pub held_reason: Option<HoldReason>,
    pub approved_at: u64,
    pub expires_at: u64,
}

#[derive(Serialize)]
pub struct Outcome {
    pub acquisition_id: Uuid,
    pub mint_id: String,
    pub state: &'static str,
    pub workers_started: bool,
    pub swap_or_melt_submitted: bool,
    pub receipt_exported: bool,
}

impl Approval {
    pub fn load(path: &Path) -> Result<Self> {
        crate::private_ipc::protected_config(path)?;
        let mut bytes = Vec::new();
        std::fs::File::open(path)?
            .take(4097)
            .read_to_end(&mut bytes)?;
        ensure!(
            !bytes.is_empty() && bytes.len() <= 4096,
            "ownership approval exceeds bound"
        );
        let approval: Self = serde_json::from_slice(&bytes)
            .map_err(|_| anyhow::anyhow!("invalid ownership approval"))?;
        approval.require_current()?;
        Ok(approval)
    }

    pub fn require_current(&self) -> Result<()> {
        self.validate_at(SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs())
    }

    fn validate_at(&self, now: u64) -> Result<()> {
        ensure!(
            self.version == 1
                && !self.wallet_instance.is_nil()
                && !self.wallet_epoch.is_nil()
                && !self.acquisition_id.is_nil()
                && self.reviewed_ownership_check
                && (self.reviewed_held_ownership_check || self.held_reason.is_none())
                && !self.mint_id.is_empty()
                && self.mint_id.len() <= 128
                && !self.mint_id.chars().any(char::is_control)
                && self.config_sha256.len() == 64
                && self
                    .config_sha256
                    .bytes()
                    .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
                && self.expires_at > self.approved_at
                && self
                    .expires_at
                    .checked_sub(self.approved_at)
                    .is_some_and(|n| n <= 86400)
                && now >= self.approved_at
                && now <= self.expires_at,
            "ownership approval is not current or valid"
        );
        Ok(())
    }

    pub fn load_config(&self, path: &Path) -> Result<Config> {
        crate::private_ipc::protected_config(path)?;
        let mut bytes = Vec::new();
        std::fs::File::open(path)?
            .take(65537)
            .read_to_end(&mut bytes)?;
        self.decode_config(&bytes)
    }

    fn decode_config(&self, bytes: &[u8]) -> Result<Config> {
        ensure!(
            !bytes.is_empty()
                && bytes.len() <= 65536
                && hex::encode(Sha256::digest(bytes)) == self.config_sha256,
            "protected configuration differs from ownership approval"
        );
        let config = Config::decode(bytes)?;
        ensure!(
            config.wallet_instance == self.wallet_instance
                && config.wallet_epoch == self.wallet_epoch,
            "ownership approval wallet registration mismatch"
        );
        ensure!(
            config
                .mints
                .iter()
                .any(|m| m.id == self.mint_id && m.bolt11_exit_qualified),
            "ownership mint is not configured and qualified"
        );
        Ok(config)
    }

    fn require_target(&self, journal: &Journal) -> Result<()> {
        ensure!(
            journal.mint_id(self.acquisition_id)? == self.mint_id,
            "ownership approval acquisition mint mismatch"
        );
        let state = journal.state(self.acquisition_id)?;
        let eligible = if let Some(reason) = self.approved_hold() {
            matches!(state.as_str(), "held_for_reconciliation" | "acquired")
                && journal.has_recheckable_ownership_hold(self.acquisition_id, reason)?
        } else {
            matches!(state.as_str(), "acquiring" | "acquired")
        };
        ensure!(
            eligible,
            "acquisition state requires a different recovery procedure"
        );
        Ok(())
    }

    fn approved_hold(&self) -> Option<HoldReason> {
        self.reviewed_held_ownership_check
            .then_some(self.held_reason.unwrap_or(HoldReason::MissingOwnedOutputs))
    }

    /// Runtime owns the existing exclusive writer lease and startup/epoch fences.
    /// The single coordinator has no new-acquisition, replay or export permission.
    pub async fn reconcile(&self, runtime: &Runtime) -> Result<Outcome> {
        self.require_current()?;
        ensure!(
            runtime.config.wallet_instance == self.wallet_instance
                && runtime.config.wallet_epoch == self.wallet_epoch,
            "runtime differs from ownership approval"
        );
        {
            let journal = runtime
                .journal
                .lock()
                .map_err(|_| anyhow::anyhow!("wallet journal unavailable"))?;
            self.require_target(&journal)?;
        }
        let mint = runtime
            .config
            .mints
            .iter()
            .find(|m| m.id == self.mint_id && m.bolt11_exit_qualified)
            .ok_or_else(|| anyhow::anyhow!("ownership mint unavailable"))?;
        let coordinator = Coordinator::new(
            mint,
            runtime
                .mint_databases
                .get(&self.mint_id)
                .ok_or_else(|| anyhow::anyhow!("ownership store unavailable"))?
                .clone(),
            &runtime.keys.seed,
            runtime.journal.clone(),
            runtime.writer.clone(),
            Arc::new(runtime.policy.keys.clone()),
            Settings {
                timeout: Duration::from_secs(runtime.config.deadline_seconds),
                message_bytes: runtime.config.body_bytes,
                maximum_receive_fee_sats: 0,
            },
            Permissions::default(),
        )?;
        self.require_current()?;
        let progress = tokio::time::timeout(
            Duration::from_secs(runtime.config.deadline_seconds),
            async {
                if let Some(reason) = self.approved_hold() {
                    coordinator
                        .recheck_held_ownership(self.acquisition_id, reason)
                        .await
                } else {
                    coordinator.reconcile_owned(self.acquisition_id).await
                }
            },
        )
        .await;
        let state = match progress {
            Ok(Ok(Progress::Acquired(_))) => "acquired",
            Ok(Ok(Progress::Held(_))) => "held_for_reconciliation",
            Err(_) if self.reviewed_held_ownership_check => "held_for_reconciliation",
            Ok(Ok(Progress::Pending(_))) | Err(_) => "pending",
            Ok(Ok(Progress::Receipt(_))) => {
                anyhow::bail!("ownership check unexpectedly exported a receipt")
            }
            Ok(Err(error)) => return Err(error),
        };
        Ok(Outcome {
            acquisition_id: self.acquisition_id,
            mint_id: self.mint_id.clone(),
            state,
            workers_started: false,
            swap_or_melt_submitted: false,
            receipt_exported: false,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn example() -> serde_json::Value {
        serde_json::json!({"version":1,"wallet_instance":Uuid::new_v4(),"wallet_epoch":Uuid::new_v4(),
            "acquisition_id":Uuid::new_v4(),"mint_id":"mint-a","config_sha256":"a".repeat(64),
            "reviewed_ownership_check":true,"approved_at":100,"expires_at":200})
    }
    #[test]
    fn approval_is_purpose_bound_current_and_cannot_authorize_replacement() -> Result<()> {
        let good = example();
        let approval: Approval = serde_json::from_value(good.clone())?;
        approval.validate_at(100)?;
        approval.validate_at(200)?;
        assert!(approval.validate_at(99).is_err());
        assert!(approval.validate_at(201).is_err());
        for (field, value) in [
            ("version", serde_json::json!(2)),
            ("wallet_instance", serde_json::json!(Uuid::nil())),
            ("wallet_epoch", serde_json::json!(Uuid::nil())),
            ("acquisition_id", serde_json::json!(Uuid::nil())),
            ("mint_id", serde_json::json!("")),
            ("config_sha256", serde_json::json!("F".repeat(64))),
            ("reviewed_ownership_check", serde_json::json!(false)),
            ("expires_at", serde_json::json!(100000)),
        ] {
            let mut value_row = good.clone();
            value_row[field] = value;
            assert!(
                serde_json::from_value::<Approval>(value_row)?
                    .validate_at(150)
                    .is_err()
            );
        }
        for field in [
            "invoice",
            "destination",
            "original_swap_replay",
            "clear_hold",
            "seed",
        ] {
            let mut value = good.clone();
            value[field] = serde_json::json!("forbidden");
            assert!(serde_json::from_value::<Approval>(value).is_err());
        }
        assert!(approval.decode_config(b"{}").is_err());
        Ok(())
    }
    #[test]
    fn exact_configuration_and_registration_are_required() -> Result<()> {
        let mut approval: Approval = serde_json::from_value(example())?;
        let config = serde_json::json!({
            "version":1,"runtime_uid":1000,"core_uid":1001,"ipc_gid":1002,
            "wallet_instance":approval.wallet_instance,"wallet_epoch":approval.wallet_epoch,
            "state_directory":"/var/lib/cashu-test","intake_socket":"/run/cashu-test/intake.sock",
            "receiving_key":cdk::nuts::SecretKey::from_slice(&[3;32])?.public_key().to_hex(),
            "recipient":cdk::nuts::SecretKey::from_slice(&[4;32])?.public_key().x_only_public_key().to_string(),
            "descriptors":[],"mints":[{"id":"mint-a","advertised_url":"https://mint.example/Bitcoin",
                "policy_version":"v1","receive_cashu":false,"receive_nutzap":false,"bolt11_exit_qualified":true}],
            "keysets":[],"body_bytes":65536,"proof_count":64,"receipt_sats":10000,
            "deadline_seconds":5,"connections_per_second":8
        });
        let bytes = serde_json::to_vec(&config)?;
        approval.config_sha256 = hex::encode(Sha256::digest(&bytes));
        approval.decode_config(&bytes)?;
        let mut changed_bytes = bytes.clone();
        changed_bytes.push(b' ');
        assert!(approval.decode_config(&changed_bytes).is_err());
        approval.wallet_epoch = Uuid::new_v4();
        assert!(approval.decode_config(&bytes).is_err());
        approval.wallet_epoch = serde_json::from_value(config["wallet_epoch"].clone())?;
        approval.mint_id = "mint-b".into();
        assert!(approval.decode_config(&bytes).is_err());
        Ok(())
    }

    #[test]
    fn target_refuses_new_held_exported_and_other_mint_work() -> Result<()> {
        use crate::journal::{Admission, HoldReason, Transport as Rail, ValidatedIntake};
        use cdk::{
            Amount,
            nuts::{Id, Proof, PublicKey},
            secret::Secret,
        };
        let directory = crate::test_directory()?;
        let mut approval: Approval = serde_json::from_value(example())?;
        let mut journal = Journal::open(
            &directory.path().join("journal.sqlite"),
            approval.wallet_epoch,
            8,
        )?;
        let proof = Proof::new(
            Amount::from(8),
            "00deadbeef123456".parse::<Id>()?,
            Secret::new("operator-test-secret"),
            PublicKey::from_hex(
                "0279be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798",
            )?,
        );
        let admission = journal.admit(&ValidatedIntake {
            mint_id: "mint-a",
            policy_version: "v1",
            transport: Rail::Cashu,
            source_ref: "private-source",
            source_author: None,
            proofs: &[proof],
        })?;
        approval.acquisition_id = match admission {
            Admission::New(id) => id,
            _ => anyhow::bail!("expected new fixture"),
        };
        assert!(approval.require_target(&journal).is_err());
        journal.prepare(approval.acquisition_id, b"synthetic retained request")?;
        assert!(approval.require_target(&journal).is_err());
        journal.start_acquisition(approval.acquisition_id)?;
        approval.require_target(&journal)?;
        approval.reviewed_held_ownership_check = true;
        assert!(approval.require_target(&journal).is_err());
        approval.reviewed_held_ownership_check = false;
        approval.mint_id = "mint-b".into();
        assert!(approval.require_target(&journal).is_err());
        approval.mint_id = "mint-a".into();
        journal.hold(approval.acquisition_id, HoldReason::AmbiguousAcquisition)?;
        assert!(approval.require_target(&journal).is_err());
        approval.reviewed_held_ownership_check = true;
        assert!(approval.require_target(&journal).is_err());
        approval.held_reason = Some(HoldReason::AmbiguousAcquisition);
        approval.validate_at(100)?;
        approval.require_target(&journal)?;
        approval.reviewed_held_ownership_check = false;
        assert!(approval.validate_at(100).is_err());
        assert_eq!(
            journal.state(approval.acquisition_id)?,
            "held_for_reconciliation"
        );
        assert_eq!(
            journal
                .prepared_request(approval.acquisition_id)?
                .as_deref(),
            Some(b"synthetic retained request".as_slice())
        );
        Ok(())
    }
}
