//! Paused wallet readiness with private, read-only SQLite opens.
use super::{Funds, open_private_readonly_store, open_readonly_journal, require_bound_wallet};
use crate::{
    journal::{Journal, cdk_storage::verify_mint_namespaces},
    service_config::Config,
    wallet::{mint_store_filename, require_mint_store_binding},
    writer_lock::{WriterLock, check_recovery_marker},
};
use anyhow::{Result, ensure};
use serde::Serialize;
use std::{fs::File, time::SystemTime};
use uuid::Uuid;

pub struct PausedRecovery {
    _writer: File,
    pub readiness: RecoveryReadiness,
}

#[derive(Serialize)]
pub struct RecoveryReadiness {
    mode: &'static str,
    wallet_instance: Uuid,
    wallet_epoch: Uuid,
    mint_namespaces: usize,
    journal_custody_sats: u64,
    reported_treasury_deposit_sats: u64,
    resume_authorized: bool,
}

impl PausedRecovery {
    /// Verify the existing identity and stores, then hold the ordinary writer
    /// lock without building the wallet, transports, listeners or workers.
    pub fn open(config: &Config) -> Result<Self> {
        ensure!(rusqlite::version() == "3.54.1", "unqualified SQLite engine");
        config.require_runtime_identity()?;
        let writer = WriterLock::lock_existing(&config.state_directory)?;
        let keys = config.load_keys()?;
        let journal = open_readonly_journal(config)?;
        journal.pragma_update(None, "trusted_schema", false)?;
        require_bound_wallet(&journal, config, &keys)?;
        Journal::verify_queued_receivers(&journal, &keys)?;
        drop(keys);

        let legacy = open_private_readonly_store(
            &config.state_directory.join("wallet.sqlite"),
            config.runtime_uid,
        )?;
        let cdk_work = "SELECT (SELECT COUNT(*) FROM proof)+(SELECT COUNT(*) FROM wallet_sagas)";
        let legacy_work: i64 = legacy.query_row(cdk_work, [], |row| row.get(0))?;
        ensure!(
            legacy_work == 0,
            "legacy shared CDK custody requires reconciliation"
        );
        let retained = verify_mint_namespaces(&journal, &config.mints)?;
        ensure!(
            config
                .mints
                .iter()
                .all(|mint| retained.contains_key(&mint.id)),
            "configured mint lacks a retained namespace"
        );
        for (mint_id, mint_url) in &retained {
            let store = open_private_readonly_store(
                &config.state_directory.join(mint_store_filename(mint_id)),
                config.runtime_uid,
            )?;
            store.pragma_update(None, "trusted_schema", false)?;
            require_mint_store_binding(&store, mint_id, mint_url)?;
            // Require the existing CDK custody and recovery tables without
            // invoking the CDK constructor or migration pipeline.
            store.prepare(cdk_work)?;
        }

        let now = i64::try_from(
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)?
                .as_secs(),
        )?;
        let funds = Funds::read(&journal, None, now)?;
        check_recovery_marker(&config.state_directory)?;
        Ok(Self {
            _writer: writer,
            readiness: RecoveryReadiness {
                mode: "recovery-paused",
                wallet_instance: config.wallet_instance,
                wallet_epoch: config.wallet_epoch,
                mint_namespaces: retained.len(),
                journal_custody_sats: funds.journal_custody_sats,
                reported_treasury_deposit_sats: funds.reported_treasury_deposit_sats,
                resume_authorized: false,
            },
        })
    }
}
