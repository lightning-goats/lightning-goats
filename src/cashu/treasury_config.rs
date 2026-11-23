//! Protected core startup configuration. No public input supplies treasury authority.
use super::{
    intake, receipt_server,
    treasury::Policy,
    treasury_client::Client,
    treasury_worker::{Controls, Worker},
};
use crate::ledger::LedgerStore;
use anyhow::{Result, ensure};
use serde::Deserialize;
use std::{path::PathBuf, sync::Arc, time::Duration};

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub socket: PathBuf,
    pub deadline_seconds: u64,
    #[serde(default)]
    pub controls: Controls,
    pub policy: Policy,
}
impl std::fmt::Debug for Config {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TreasuryConfig")
            .field("controls", &self.controls)
            .field("spending_enabled", &self.policy.enabled)
            .finish_non_exhaustive()
    }
}
impl Config {
    pub fn validate(
        &self,
        receipts: Option<&receipt_server::Config>,
        intake: Option<&intake::Config>,
    ) -> Result<()> {
        crate::private_ipc::socket_path(&self.socket)?;
        self.controls.validate()?;
        self.policy.validate()?;
        self.controls.sweep.validate(&self.policy)?;
        ensure!(
            (1..=60).contains(&self.deadline_seconds),
            "invalid treasury deadline"
        );
        let receipts = receipts
            .ok_or_else(|| anyhow::anyhow!("treasury requires protected wallet registration"))?;
        receipts.validate()?;
        ensure!(
            self.socket != receipts.socket,
            "treasury and receipt sockets must differ"
        );
        if let Some(intake) = intake {
            ensure!(
                self.socket != intake.socket
                    && intake.wallet_uid == receipts.wallet_uid
                    && intake.wallet_instance == receipts.wallet_instance
                    && intake.wallet_epoch == receipts.wallet_epoch,
                "treasury/intake wallet registration or socket conflicts"
            );
        }
        Ok(())
    }
    pub fn worker(
        &self,
        receipts: &receipt_server::Config,
        ledger: LedgerStore,
        collector: Option<&crate::config::BreezCollectorConfig>,
    ) -> Result<Option<Worker>> {
        self.validate(Some(receipts), None)?;
        if !(self.controls.plan
            || self.controls.sweep.enabled
            || self.controls.issue
            || self.controls.deliver
            || self.controls.inspect)
        {
            return Ok(None);
        }
        let uid = crate::private_ipc::uid()?;
        ensure!(
            uid > 0 && uid != receipts.wallet_uid,
            "treasury requires a separate non-root identity"
        );
        let registration = Arc::new(receipts.policy()?);
        let deadline = Duration::from_secs(self.deadline_seconds);
        let client = Client::new(self.socket.clone(), registration.clone(), deadline, 1)?;
        Ok(Some(Worker::new(
            ledger,
            collector
                .ok_or_else(|| {
                    anyhow::anyhow!("treasury conversion requires project mint invoicing")
                })?
                .clone(),
            registration,
            Arc::new(self.policy.clone()),
            client,
            self.controls.clone(),
            deadline,
        )?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};
    use uuid::Uuid;

    fn config() -> Value {
        json!({"socket":"/run/lightning-goats-cashu/treasury.sock","deadline_seconds":30,
            "policy":{"version":"treasury-v1","invoice_expiry_seconds":300,
                "destination":{"origin":"https://api.example.com/","account_id":Uuid::from_u128(1),
                    "credential_fingerprint":"ab".repeat(32),"network":"bitcoin"}}})
    }
    fn receipts() -> receipt_server::Config {
        receipt_server::Config {
            socket: "/run/lightning-goats/receipts.sock".into(),
            ipc_gid: 60002,
            wallet_uid: 60001,
            wallet_instance: Uuid::from_u128(2),
            wallet_epoch: Uuid::from_u128(3),
            core_epoch: Uuid::from_u128(4),
            import_enabled: false,
            receipts_per_24h: 0,
            unlimited_receipts_per_24h: false,
            credit_sats_per_24h: 0,
            maximum_custody_sats: 0,
            approved: vec![],
            deadline_seconds: 5,
            connections_per_second: 1,
        }
    }
    #[test]
    fn protected_registration_is_required_but_paused_import_does_not_disable_inspection()
    -> Result<()> {
        let mut value = config();
        let off: Config = serde_json::from_value(value.clone())?;
        assert!(
            !off.controls.sweep.enabled
                && !off.controls.plan
                && !off.controls.issue
                && !off.controls.deliver
                && !off.controls.inspect
                && !off.policy.enabled
        );
        assert!(off.validate(None, None).is_err());
        let receipts = receipts();
        off.validate(Some(&receipts), None)?;
        value["controls"] = json!({"inspect":true,"maximum_attempts":5});
        let inspect: Config = serde_json::from_value(value.clone())?;
        inspect.validate(Some(&receipts), None)?;
        assert!(inspect.policy.approved_mints.is_empty());
        let mut conflicting = inspect.clone();
        conflicting.socket = receipts.socket.clone();
        assert!(conflicting.validate(Some(&receipts), None).is_err());
        let mut intake = intake::Config {
            socket: "/run/lightning-goats-cashu/intake.sock".into(),
            wallet_uid: receipts.wallet_uid,
            wallet_instance: receipts.wallet_instance,
            wallet_epoch: receipts.wallet_epoch,
            receive_cashu: false,
            receive_nutzap: false,
            body_bytes: 1024,
            concurrent_requests: 1,
            deadline_seconds: 1,
        };
        inspect.validate(Some(&receipts), Some(&intake))?;
        intake.wallet_epoch = Uuid::from_u128(99);
        assert!(inspect.validate(Some(&receipts), Some(&intake)).is_err());
        value["policy"]["enabled"] = json!(true);
        assert!(
            serde_json::from_value::<Config>(value)?
                .validate(Some(&receipts), None)
                .is_err()
        );
        Ok(())
    }
    #[test]
    fn invoice_expiry_is_rejected_at_startup_outside_the_intent_contract() -> Result<()> {
        let receipts = receipts();
        for (seconds, accepted) in [
            (0, false),
            (1, true),
            (3_600, true),
            (3_601, false),
            (86_400, false),
        ] {
            let mut value = config();
            value["policy"]["invoice_expiry_seconds"] = json!(seconds);
            let parsed: Config = serde_json::from_value(value)?;
            assert_eq!(
                parsed.validate(Some(&receipts), None).is_ok(),
                accepted,
                "invoice lifetime {seconds}"
            );
        }
        Ok(())
    }
    #[test]
    fn enabled_financial_limits_and_unknown_configuration_are_rejected_offline() -> Result<()> {
        let mut value = config();
        value["policy"]["enabled"] = json!(true);
        value["policy"]["limits"] = json!({"minimum_deposit_sats":1,"maximum_deposit_sats":7,
            "maximum_input_sats":8,"maximum_fee_sats":1,"operations_per_24h":2,
            "input_sats_per_24h":16,"maximum_in_flight_sats":8});
        value["policy"]["approved_mints"] = json!({"fixture":{"minimum_deposit_sats":1,
            "maximum_deposit_sats":7,"maximum_input_sats":8,"maximum_fee_sats":1,"maximum_in_flight_sats":8}});
        let receipts = receipts();
        serde_json::from_value::<Config>(value.clone())?.validate(Some(&receipts), None)?;
        for (field, invalid) in [
            ("maximum_input_sats", json!(6)),
            ("operations_per_24h", json!(0)),
            ("maximum_fee_sats", json!(u64::MAX)),
            ("minimum_deposit_sats", json!(9)),
        ] {
            let mut changed = value.clone();
            changed["policy"]["limits"][field] = invalid;
            assert!(
                serde_json::from_value::<Config>(changed)?
                    .validate(Some(&receipts), None)
                    .is_err()
            );
        }
        value["policy"]["limits"]["unlimited_operations_per_24h"] = json!(true);
        assert!(
            serde_json::from_value::<Config>(value.clone())?
                .validate(Some(&receipts), None)
                .is_err()
        );
        value["policy"]["limits"]
            .as_object_mut()
            .unwrap()
            .remove("operations_per_24h");
        serde_json::from_value::<Config>(value.clone())?.validate(Some(&receipts), None)?;
        value["arbitrary_pay"] = json!(true);
        assert!(serde_json::from_value::<Config>(value).is_err());
        Ok(())
    }
}
