//! Recover one original treasury melt from the registered wallet.
use crate::{
    service::Runtime,
    service_config::{Config, read_protected},
    transport::MintTransport,
    treasury_recovery::Recovery,
};
use anyhow::{Result, ensure};
use cdk_http_client::Transport;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    path::Path,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use uuid::Uuid;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Purpose {
    ReconcileOriginalTreasuryMelt,
    CleanupOriginalTreasuryMelt,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Approval {
    version: u8,
    purpose: Purpose,
    wallet_instance: Uuid,
    wallet_epoch: Uuid,
    core_epoch: Uuid,
    intent_id: Uuid,
    mint_id: String,
    config_sha256: String,
    operation_evidence_digest: Option<String>,
    approved_at: u64,
    expires_at: u64,
}

#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum State {
    Pending,
    ReportRecorded,
    ReportAlreadyRetained,
    CleanupRecorded,
    CleanupAlreadyRetained,
}

#[derive(Serialize)]
pub struct Outcome {
    intent_id: Uuid,
    mint_id: String,
    state: State,
    operation_evidence_digest: Option<String>,
    workers_started: bool,
    payment_submitted: bool,
    strike_settlement_verified: bool,
    cleanup_attempted: bool,
}

impl Approval {
    pub fn load(path: &Path, expected: Purpose) -> Result<Self> {
        let bytes = read_protected(path)?;
        ensure!(bytes.len() <= 4096, "treasury approval exceeds size bound");
        let value: Self = serde_json::from_slice(&bytes)
            .map_err(|_| anyhow::anyhow!("invalid treasury reconciliation approval"))?;
        ensure!(
            value.purpose == expected,
            "treasury approval purpose differs"
        );
        value.require_current()?;
        Ok(value)
    }

    fn require_current(&self) -> Result<()> {
        let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
        ensure!(
            self.version == 1
                && match self.purpose {
                    Purpose::ReconcileOriginalTreasuryMelt =>
                        self.operation_evidence_digest.is_none(),
                    Purpose::CleanupOriginalTreasuryMelt => self
                        .operation_evidence_digest
                        .as_ref()
                        .is_some_and(|digest| lightning_goats_nostr_event::validate_hex(
                            digest, 32
                        )
                        .is_ok()),
                }
                && !self.wallet_instance.is_nil()
                && !self.wallet_epoch.is_nil()
                && !self.core_epoch.is_nil()
                && !self.intent_id.is_nil()
                && !self.mint_id.is_empty()
                && self.mint_id.len() <= 128
                && !self.mint_id.chars().any(char::is_control)
                && self
                    .expires_at
                    .checked_sub(self.approved_at)
                    .is_some_and(|age| (1..=86400).contains(&age))
                && (self.approved_at..=self.expires_at).contains(&now),
            "invalid or expired treasury reconciliation approval"
        );
        Ok(())
    }

    pub fn load_config(&self, path: &Path) -> Result<Config> {
        let bytes = read_protected(path)?;
        ensure!(
            hex::encode(Sha256::digest(&bytes)) == self.config_sha256,
            "wallet configuration differs from treasury reconciliation approval"
        );
        let config = Config::decode(&bytes)?;
        self.require_scope(&config)?;
        Ok(config)
    }

    fn require_scope(&self, config: &Config) -> Result<()> {
        ensure!(
            config.wallet_instance == self.wallet_instance
                && config.wallet_epoch == self.wallet_epoch
                && config
                    .treasury_intake
                    .as_ref()
                    .is_some_and(|intake| intake.core_epoch == self.core_epoch
                        && intake.policies.contains_key(&self.mint_id)),
            "treasury reconciliation registration differs"
        );
        Ok(())
    }

    pub async fn execute(&self, runtime: &Runtime) -> Result<Outcome> {
        let config = &runtime.config;
        self.require_scope(config)?;
        let binding = config
            .treasury_intake
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("treasury configuration missing"))?
            .bindings(config)?
            .remove(&self.mint_id)
            .ok_or_else(|| anyhow::anyhow!("treasury binding missing"))?;
        let mint = config
            .mints
            .iter()
            .find(|mint| mint.id == self.mint_id)
            .ok_or_else(|| anyhow::anyhow!("treasury mint missing"))?;
        let deadline = Duration::from_secs(config.deadline_seconds);
        let transport =
            MintTransport::new(mint.advertised_url.parse()?, deadline, config.body_bytes)?;
        let recovery = Recovery {
            transport: &transport,
            journal: runtime.journal.clone(),
            database: runtime
                .mint_databases
                .get(&self.mint_id)
                .ok_or_else(|| anyhow::anyhow!("treasury mint store missing"))?
                .clone(),
            writer: runtime.writer.clone(),
            binding: &binding,
            keys: &runtime.policy.keys,
            seed: &runtime.keys.seed,
        };
        self.require_current()?;
        let state = tokio::time::timeout(deadline, async {
            Ok::<_, anyhow::Error>(if let Some(digest) = &self.operation_evidence_digest {
                if recovery.cleanup(self.intent_id, Some(digest)).await? {
                    State::CleanupRecorded
                } else {
                    State::CleanupAlreadyRetained
                }
            } else {
                reconcile_original(&recovery, self.intent_id).await?
            })
        })
        .await??;
        let operation_evidence_digest = if state == State::Pending {
            None
        } else {
            Some(
                runtime
                    .journal
                    .lock()
                    .map_err(|_| anyhow::anyhow!("wallet journal unavailable"))?
                    .treasury_melt_report(&binding, self.intent_id)?
                    .operation_evidence_digest,
            )
        };
        Ok(Outcome {
            intent_id: self.intent_id,
            mint_id: self.mint_id.clone(),
            cleanup_attempted: state == State::CleanupRecorded,
            state,
            operation_evidence_digest,
            workers_started: false,
            payment_submitted: false,
            strike_settlement_verified: false,
        })
    }
}

/// Reuse the wallet's recovery-only path. It requires the original submitted
/// request and uses the retained quote. The completed report is immutable.
pub(crate) async fn reconcile_original<T: Transport>(
    recovery: &Recovery<'_, T>,
    id: Uuid,
) -> Result<State> {
    if recovery
        .journal
        .lock()
        .map_err(|_| anyhow::anyhow!("wallet journal unavailable"))?
        .completed_treasury_report(recovery.binding, id)?
        .is_some()
    {
        return Ok(State::ReportAlreadyRetained);
    }
    Ok(if recovery.reconcile(id).await?.is_some() {
        State::ReportRecorded
    } else {
        State::Pending
    })
}
