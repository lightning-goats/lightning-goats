//! Private read-only journal status. No keys, token bodies, source authors or requests.
use crate::service_config::Config;
use anyhow::{Result, ensure};
use rusqlite::{Connection, OpenFlags, params};
use serde::Serialize;
use std::{collections::BTreeMap, os::unix::fs::MetadataExt, time::Duration};
use uuid::Uuid;
mod health;
pub use health::{Funds, Health, MintFunds, Queue};

const DETAIL_LIMIT: usize = 64;
#[derive(Serialize)]
pub struct Status {
    pub wallet_instance: Uuid,
    pub wallet_epoch: Uuid,
    pub observed_wallet_instance: Uuid,
    pub observed_wallet_epoch: Uuid,
    pub registration_matches: bool,
    pub health: Health,
    pub recovery_required: bool,
    pub cdk_namespace_layout: Option<u32>,
    pub mint_namespaces: Vec<MintNamespace>,
    pub mint_namespaces_truncated: bool,
    pub incomplete_mint_namespaces: u64,
    pub state_counts: BTreeMap<String, u64>,
    pub acquisition_enabled: bool,
    pub reconciliation_enabled: bool,
    pub receipt_delivery_enabled: bool,
    pub treasury_intake_enabled: bool,
    pub treasury_admission_enabled: bool,
    pub treasury_execution_enabled: bool,
    pub treasury_split_execution_enabled: bool,
    pub treasury_reconciliation_enabled: bool,
    pub exhausted_acquisitions: u64,
    pub exhausted_deliveries: u64,
    pub operations: Vec<Operation>,
    pub operations_truncated: bool,
    pub relay_intake_enabled: bool,
    pub relay_scans: Vec<RelayScan>,
    pub publication_signing_enabled: bool,
    pub unsigned_publications: u64,
    pub signed_unpublished: u64,
    pub exhausted_publication_signing: u64,
    pub publication_delivery_enabled: bool,
    pub unrouted_publications: u64,
    pub pending_relay_deliveries: u64,
    pub exhausted_publication_routing: u64,
    pub exhausted_relay_deliveries: u64,
}
#[derive(Serialize)]
pub struct MintNamespace {
    pub mint_id: String,
    pub ready: bool,
    pub configured: bool,
}

#[derive(Serialize)]
pub struct RelayScan {
    pub scope: String,
    pub relay: String,
    pub lane: String,
    pub cursor: i64,
    pub horizon: i64,
    pub width: i64,
    pub outcome: String,
    pub next_attempt: i64,
    pub last_admitted: u16,
    pub last_rejected: u16,
}
#[derive(Serialize)]
pub struct Operation {
    pub acquisition_id: Uuid,
    pub mint_id: String,
    pub state: String,
    pub hold_reason: Option<String>,
    pub acquisition_attempts: u32,
    pub acquisition_next_attempt: Option<i64>,
    pub delivery_attempts: u32,
    pub delivery_next_attempt: Option<i64>,
}
impl Status {
    /// Safe while the runtime holds its writer lock. This connection cannot initialize state.
    pub fn read(config: &Config) -> Result<Self> {
        let now = i64::try_from(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_secs(),
        )?;
        Self::read_at(config, now)
    }

    pub(crate) fn read_at(config: &Config, now: i64) -> Result<Self> {
        ensure!(now >= 0, "invalid status observation time");
        config.prepare()?;
        config.require_runtime_identity()?;
        // Report the fence without following it or interpreting its contents.
        // Status remains available for reconciliation while startup is blocked.
        let recovery_required =
            match std::fs::symlink_metadata(config.state_directory.join("CASHU_RECOVERY_REQUIRED"))
            {
                Ok(_) => true,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
                Err(error) => return Err(error.into()),
            };
        let path = config.state_directory.join("acquisition.sqlite");
        let metadata = std::fs::symlink_metadata(&path)?;
        ensure!(
            metadata.is_file()
                && metadata.nlink() == 1
                && metadata.uid() == config.runtime_uid
                && metadata.mode() & 0o077 == 0,
            "status requires a private regular journal"
        );
        let mut connection = Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_ONLY
                | OpenFlags::SQLITE_OPEN_NO_MUTEX
                | OpenFlags::SQLITE_OPEN_NOFOLLOW,
        )?;
        connection.busy_timeout(Duration::from_secs(1))?;
        connection.pragma_update(None, "query_only", true)?;
        let tx = connection.transaction()?;
        let epoch: String = tx.query_row(
            "SELECT epoch FROM wallet_epoch WHERE singleton=1",
            [],
            |r| r.get(0),
        )?;
        let instance: String = tx.query_row(
            "SELECT instance FROM runtime_registration WHERE singleton=1",
            [],
            |r| r.get(0),
        )?;
        let observed_wallet_epoch = Uuid::parse_str(&epoch)?;
        let observed_wallet_instance = Uuid::parse_str(&instance)?;
        ensure!(
            !observed_wallet_epoch.is_nil() && !observed_wallet_instance.is_nil(),
            "invalid retained wallet identity"
        );
        let registration_matches = observed_wallet_epoch == config.wallet_epoch
            && observed_wallet_instance == config.wallet_instance;
        let health = Health::read(&tx, config, now)?;
        let mut state_counts = BTreeMap::new();
        {
            let mut query = tx.prepare("SELECT state,COUNT(*) FROM acquisitions GROUP BY state")?;
            for row in query.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, u64>(1)?)))? {
                let (state, count) = row?;
                state_counts.insert(state, count);
            }
        }
        let has_layout: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='cdk_storage_layout')",[],|r|r.get(0))?;
        let mut cdk_namespace_layout = None;
        let mut mint_namespaces = Vec::new();
        let mut incomplete_mint_namespaces = 0;
        let mut namespace_count = 0_u64;
        if has_layout {
            cdk_namespace_layout = Some(tx.query_row(
                "SELECT version FROM cdk_storage_layout WHERE singleton=1",
                [],
                |r| r.get(0),
            )?);
            namespace_count =
                tx.query_row("SELECT COUNT(*) FROM cdk_mint_namespaces", [], |r| r.get(0))?;
            incomplete_mint_namespaces = tx.query_row(
                "SELECT COUNT(*) FROM cdk_mint_namespaces WHERE ready=0",
                [],
                |r| r.get(0),
            )?;
            let mut query = tx.prepare(
                "SELECT mint_id,mint_url,ready FROM cdk_mint_namespaces ORDER BY mint_id LIMIT 64",
            )?;
            let mut rows = query.query([])?;
            while let Some(row) = rows.next()? {
                let mint_id: String = row.get(0)?;
                let url: String = row.get(1)?;
                let configured = config
                    .mints
                    .iter()
                    .any(|m| m.id == mint_id && m.advertised_url == url);
                mint_namespaces.push(MintNamespace {
                    mint_id,
                    ready: row.get(2)?,
                    configured,
                });
            }
        }
        let delivery = config.receipt_delivery.as_ref();
        let signing = config.publication_signing.as_ref();
        let publishing = config.publication_delivery.as_ref();
        let publishing_budget = publishing.map_or(0, |c| c.maximum_attempts);
        let unrouted_publications = tx.query_row("SELECT COUNT(*) FROM nostr_publications p WHERE p.signed_event IS NOT NULL AND p.published=0 AND NOT EXISTS(SELECT 1 FROM publication_routes r WHERE r.publication_id=p.id)",[],|r|r.get(0))?;
        let pending_relay_deliveries = tx.query_row(
            "SELECT COUNT(*) FROM publication_delivery WHERE acknowledged=0",
            [],
            |r| r.get(0),
        )?;
        let exhausted_publication_routing = tx.query_row("SELECT COUNT(*) FROM nostr_publications p JOIN publication_routing_schedule s ON s.publication_id=p.id WHERE p.signed_event IS NOT NULL AND p.published=0 AND NOT EXISTS(SELECT 1 FROM publication_routes r WHERE r.publication_id=p.id) AND ?>0 AND s.attempts>=?",params![publishing_budget,publishing_budget],|r|r.get(0))?;
        let exhausted_relay_deliveries = tx.query_row("SELECT COUNT(*) FROM publication_delivery WHERE acknowledged=0 AND ?>0 AND attempts>=?",params![publishing_budget,publishing_budget],|r|r.get(0))?;
        let signing_budget = signing.map_or(0, |c| c.maximum_attempts);
        let unsigned_publications = tx.query_row(
            "SELECT COUNT(*) FROM nostr_publications WHERE signed_event IS NULL AND published=0",
            [],
            |r| r.get(0),
        )?;
        let signed_unpublished = tx.query_row("SELECT COUNT(*) FROM nostr_publications WHERE signed_event IS NOT NULL AND published=0",[],|r|r.get(0))?;
        let exhausted_publication_signing = tx.query_row("SELECT COUNT(*) FROM nostr_publications p JOIN publication_signing_schedule s ON s.publication_id=p.id WHERE p.signed_event IS NULL AND p.published=0 AND ?>0 AND s.attempts>=?",params![signing_budget,signing_budget],|r|r.get(0))?;
        let exhausted_acquisitions = tx.query_row("SELECT COUNT(*) FROM acquisitions a JOIN acquisition_schedule s ON s.acquisition_id=a.id WHERE a.state IN ('validated','acquisition_prepared','acquiring') AND ?>0 AND s.attempts>=?",params![config.execution.maximum_attempts,config.execution.maximum_attempts],|r|r.get(0))?;
        let delivery_budget = delivery.map_or(0, |c| c.maximum_attempts);
        let exhausted_deliveries = tx.query_row("SELECT COUNT(*) FROM acquisitions a JOIN receipt_delivery_schedule s ON s.acquisition_id=a.id WHERE a.state IN ('acquired','credit_export_pending') AND ?>0 AND s.attempts>=?",params![delivery_budget,delivery_budget],|r|r.get(0))?;
        let mut operations = Vec::new();
        {
            let mut query = tx.prepare("SELECT a.id,a.mint_id,a.state,h.reason,COALESCE(s.attempts,0),s.next_attempt,COALESCE(d.attempts,0),d.next_attempt FROM acquisitions a LEFT JOIN operation_holds h ON h.acquisition_id=a.id LEFT JOIN acquisition_schedule s ON s.acquisition_id=a.id LEFT JOIN receipt_delivery_schedule d ON d.acquisition_id=a.id WHERE a.state!='credited' ORDER BY CASE WHEN a.state='held_for_reconciliation' THEN 0 ELSE 1 END,a.rowid LIMIT ?")?;
            let mut rows = query.query([i64::try_from(DETAIL_LIMIT)?])?;
            while let Some(row) = rows.next()? {
                operations.push(Operation {
                    acquisition_id: Uuid::parse_str(&row.get::<_, String>(0)?)?,
                    mint_id: row.get(1)?,
                    state: row.get(2)?,
                    hold_reason: row.get(3)?,
                    acquisition_attempts: row.get(4)?,
                    acquisition_next_attempt: row.get(5)?,
                    delivery_attempts: row.get(6)?,
                    delivery_next_attempt: row.get(7)?,
                });
            }
        }
        let pending: u64 = tx.query_row(
            "SELECT COUNT(*) FROM acquisitions WHERE state!='credited'",
            [],
            |r| r.get(0),
        )?;
        let mut relay_scans = Vec::new();
        {
            let mut query = tx.prepare("SELECT hex(s.scope),s.relay,s.lane,s.cursor,s.horizon,s.width,s.outcome,s.next_attempt,COALESCE(r.admitted,0),COALESCE(r.rejected,0) FROM relay_scans s LEFT JOIN relay_scan_results r USING(scope,relay,lane) ORDER BY s.scope,s.relay,s.lane LIMIT 128")?;
            let mut rows = query.query([])?;
            while let Some(row) = rows.next()? {
                let relay: String = row.get(1)?;
                let mut public_relay = url::Url::parse(&relay)?;
                public_relay.set_query(None);
                relay_scans.push(RelayScan {
                    scope: row.get(0)?,
                    relay: public_relay.to_string(),
                    lane: row.get(2)?,
                    cursor: row.get(3)?,
                    horizon: row.get(4)?,
                    width: row.get(5)?,
                    outcome: row.get(6)?,
                    next_attempt: row.get(7)?,
                    last_admitted: row.get(8)?,
                    last_rejected: row.get(9)?,
                });
            }
        }
        tx.commit()?;
        Ok(Self {
            wallet_instance: config.wallet_instance,
            wallet_epoch: config.wallet_epoch,
            observed_wallet_instance,
            observed_wallet_epoch,
            registration_matches,
            health,
            recovery_required: recovery_required || !registration_matches,
            cdk_namespace_layout,
            mint_namespaces_truncated: namespace_count > mint_namespaces.len() as u64,
            mint_namespaces,
            incomplete_mint_namespaces,
            state_counts,
            treasury_execution_enabled: config.treasury_execution.execute,
            treasury_split_execution_enabled: config.treasury_execution.execute
                && config.treasury_execution.split.enabled,
            treasury_reconciliation_enabled: config.treasury_execution.reconcile,
            acquisition_enabled: config.execution.acquire,
            reconciliation_enabled: config.execution.reconcile,
            receipt_delivery_enabled: delivery.is_some_and(|c| c.enabled),
            treasury_intake_enabled: config.treasury_intake.as_ref().is_some_and(|c| c.enabled),
            treasury_admission_enabled: config
                .treasury_intake
                .as_ref()
                .is_some_and(|c| c.enabled && c.accept_new),
            exhausted_acquisitions,
            exhausted_deliveries,
            operations_truncated: pending > operations.len() as u64,
            operations,
            relay_intake_enabled: config.relay_intake.as_ref().is_some_and(|c| c.enabled),
            relay_scans,
            publication_signing_enabled: signing.is_some_and(|c| c.enabled()),
            unsigned_publications,
            signed_unpublished,
            exhausted_publication_signing,
            publication_delivery_enabled: publishing.is_some_and(|c| c.enabled()),
            unrouted_publications,
            pending_relay_deliveries,
            exhausted_publication_routing,
            exhausted_relay_deliveries,
        })
    }
}
