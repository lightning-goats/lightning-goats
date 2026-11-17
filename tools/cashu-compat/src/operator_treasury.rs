//! Recover one original treasury melt from the registered wallet.
use crate::{
    service::Runtime, service_config::Config, transport::MintTransport, treasury_recovery::Recovery,
};
use anyhow::{Result, ensure};
use cdk_http_client::Transport;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    io::Read,
    path::Path,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use uuid::Uuid;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Approval {
    version: u8,
    purpose: String,
    wallet_instance: Uuid,
    wallet_epoch: Uuid,
    core_epoch: Uuid,
    intent_id: Uuid,
    mint_id: String,
    config_sha256: String,
    approved_at: u64,
    expires_at: u64,
}

#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum State {
    Pending,
    ReportRecorded,
    ReportAlreadyRetained,
}

#[derive(Serialize)]
pub struct Outcome {
    intent_id: Uuid,
    mint_id: String,
    state: State,
    workers_started: bool,
    payment_submitted: bool,
    strike_settlement_verified: bool,
    cleanup_attempted: bool,
}

fn read_protected(path: &Path, limit: u64) -> Result<Vec<u8>> {
    crate::private_ipc::protected_config(path)?;
    let mut bytes = Vec::new();
    std::fs::File::open(path)?
        .take(limit + 1)
        .read_to_end(&mut bytes)?;
    ensure!(
        !bytes.is_empty() && bytes.len() as u64 <= limit,
        "invalid protected document size"
    );
    Ok(bytes)
}

impl Approval {
    pub fn load(path: &Path) -> Result<Self> {
        let value: Self = serde_json::from_slice(&read_protected(path, 4096)?)
            .map_err(|_| anyhow::anyhow!("invalid treasury reconciliation approval"))?;
        value.require_current()?;
        Ok(value)
    }

    fn require_current(&self) -> Result<()> {
        let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
        ensure!(
            self.version == 1
                && self.purpose == "reconcile_original_treasury_melt"
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
        let bytes = read_protected(path, 65536)?;
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

    pub async fn reconcile(&self, runtime: &Runtime) -> Result<Outcome> {
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
        let state =
            tokio::time::timeout(deadline, reconcile_original(&recovery, self.intent_id)).await??;
        Ok(Outcome {
            intent_id: self.intent_id,
            mint_id: self.mint_id.clone(),
            state,
            workers_started: false,
            payment_submitted: false,
            strike_settlement_verified: false,
            cleanup_attempted: false,
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
