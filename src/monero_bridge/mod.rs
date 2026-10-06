//! Receive-only home bridge. It has no financial ledger or actuator capability.
//! The VPS must treat snapshots as private evidence, not as credit grants.
mod address;
pub mod provider;
mod web;

use anyhow::{Context, Result, bail};
use provider::{Created, Provider, Transaction};
use serde::{Deserialize, Serialize};
use sqlx::{Row, SqliteConnection, SqlitePool};
use std::{
    io::Read,
    net::SocketAddr,
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::sync::Semaphore;
use uuid::Uuid;

pub const UPSTREAM_PIN: &str = "cbf644ce025914857fdafa8e7e7aeacb849c6159";
pub const PROTOCOL: &str = "monero-bridge-v1";

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub listen: SocketAddr,
    pub callback_listen: SocketAddr,
    pub database_url: String,
    pub provider_url: String,
    pub network: String,
    /// SHA256 of the dedicated account-0 primary address; never a wallet key.
    pub account_scope: String,
    pub api_token_file: PathBuf,
    pub sync_report_file: PathBuf,
    pub sync_report_owner_uid: u32,
    pub create_enabled: bool,
    pub max_amount_atomic: u64,
    pub max_intents: u32,
    pub max_receipts: u32,
    pub max_creates_per_hour: u32,
    pub max_work: u32,
    pub max_http: u32,
    pub poll_seconds: u64,
    pub timeout_seconds: u64,
    pub freshness_seconds: u64,
    pub max_wallet_lag: u64,
}
impl Config {
    pub fn load(path: &Path) -> Result<Self> {
        let data = std::fs::read(path)?;
        if data.len() > 16_384 {
            bail!("bridge configuration too large");
        }
        let config: Self = toml::from_str(std::str::from_utf8(&data)?)?;
        config.validate()?;
        Ok(config)
    }
    pub fn validate(&self) -> Result<()> {
        let provider = provider::loopback_base(&self.provider_url)?;
        for listen in [self.listen, self.callback_listen] {
            if listen.ip().to_string() != "127.0.0.1"
                || listen.port() == 0
                || listen.port() == 5000
                || Some(listen.port()) == provider.port()
            {
                bail!(
                    "bridge listeners must be distinct explicit loopback ports, not weather/provider"
                );
            }
        }
        if self.listen == self.callback_listen {
            bail!("separate callback listener required");
        }
        if !matches!(self.network.as_str(), "mainnet" | "testnet" | "stagenet")
            || !hex_id(&self.account_scope)
            || self.max_amount_atomic == 0
            || self.max_amount_atomic > i64::MAX as u64
            || !(1..=100_000).contains(&self.max_intents)
            || !(1..=2048).contains(&self.max_receipts)
            || !(1..=10_000).contains(&self.max_creates_per_hour)
            || !(1..=16).contains(&self.max_work)
            || !(1..=64).contains(&self.max_http)
            || !(5..=3600).contains(&self.poll_seconds)
            || !(1..=30).contains(&self.timeout_seconds)
            || !(10..=300).contains(&self.freshness_seconds)
            || self.max_wallet_lag > 10
        {
            bail!("invalid bridge identity or bounded resource policy");
        }
        crate::sqlite::durable_options(&self.database_url)?;
        Ok(())
    }
    fn namespace(&self) -> String {
        format!(
            "{PROTOCOL}|{}|{}|{}|{UPSTREAM_PIN}",
            self.network, self.account_scope, self.provider_url
        )
    }
}

pub fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .min(i64::MAX as u64) as i64
}
fn hex_id(s: &str) -> bool {
    s.len() == 64
        && s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// Linux O_NOFOLLOW, plus fstat on the opened file (not a pre-open path check).
/// Reject sockets/FIFOs/symlinks and untrusted-writable or publicly-readable files.
pub fn protected_read(path: &Path, owner: Option<u32>, limit: usize) -> Result<Vec<u8>> {
    use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
    let file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(0x20000 | 0x800)
        .open(path)?;
    let meta = file.metadata()?;
    if !meta.is_file()
        || meta.mode() & 0o027 != 0
        || owner.is_some_and(|uid| meta.uid() != uid)
        || meta.len() > limit as u64
    {
        bail!("protected file ownership/mode/type rejected");
    }
    let mut bytes = Vec::new();
    file.take(limit as u64 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > limit {
        bail!("protected file exceeds limit");
    }
    Ok(bytes)
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SyncReport {
    pub version: String,
    pub network: String,
    pub account_scope: String,
    pub provider_url: String,
    pub checked_at: i64,
    pub wallet_height: u64,
    pub daemon_height: u64,
    pub synchronized: bool,
}
impl SyncReport {
    fn read(c: &Config, at: i64) -> Result<Self> {
        let bytes = protected_read(&c.sync_report_file, Some(c.sync_report_owner_uid), 4096)?;
        let r: Self = serde_json::from_slice(&bytes)?;
        if r.version != "monero-sync-v1"
            || r.network != c.network
            || r.account_scope != c.account_scope
            || r.provider_url != c.provider_url
            || !r.synchronized
            || r.checked_at <= 0
            || r.checked_at > at
            || at - r.checked_at > c.freshness_seconds as i64
            || r.wallet_height == 0
            || r.daemon_height > i64::MAX as u64
            || r.wallet_height > r.daemon_height
            || r.daemon_height - r.wallet_height > c.max_wallet_lag
        {
            bail!("fresh synchronized wallet evidence unavailable");
        }
        Ok(r)
    }
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CreateReceive {
    pub intent_id: Uuid,
    pub expected_atomic: u64,
    pub address_user: String,
    pub expires_at: i64,
}
impl CreateReceive {
    fn validate(&self) -> Result<()> {
        if self.intent_id.is_nil()
            || self.expected_atomic == 0
            || self.expected_atomic > i64::MAX as u64
            || !matches!(
                self.address_user.as_str(),
                "herd" | "dexter" | "rowan" | "cosmo" | "newton" | "nova"
            )
            || self.expires_at <= 0
        {
            bail!("invalid project receive intent");
        }
        Ok(())
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Observation {
    pub transaction: Transaction,
    /// Bridge time of first successful, synchronized full-state read, not tx time.
    pub first_seen_at: i64,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Snapshot {
    pub version: String,
    pub generation: String,
    pub intent_id: Uuid,
    pub network: String,
    pub account_scope: String,
    pub expected_atomic: u64,
    pub state: String,
    pub address: Option<String>,
    pub observed_at: Option<i64>,
    pub revision: u64,
    pub ready: bool,
    pub hold: Option<String>,
    pub receipts: Vec<Observation>,
}

#[derive(Clone)]
pub struct Bridge {
    pub(crate) config: Arc<Config>,
    pub(crate) token: Arc<zeroize::Zeroizing<String>>,
    pub(crate) http_slots: Arc<Semaphore>,
    pool: SqlitePool,
    provider: Provider,
    work_slots: Arc<Semaphore>,
    generation: String,
}

impl Bridge {
    pub async fn open(config: Config) -> Result<Self> {
        config.validate()?;
        let token = protected_read(&config.api_token_file, None, 65)?;
        let token = String::from_utf8(token)?.trim_end_matches('\n').to_owned();
        if !hex_id(&token) {
            bail!("API token must be 32 random bytes encoded as lowercase hex");
        }
        let pool = crate::sqlite::connect_durable(&config.database_url, 4).await?;
        let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
        let tables: Vec<String> = sqlx::query_scalar(
            "SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%'",
        )
        .fetch_all(&mut *tx)
        .await?;
        if tables
            .iter()
            .any(|n| !matches!(n.as_str(), "monero_bridge_meta" | "monero_bridge_intents"))
            || (!tables.is_empty() && !tables.iter().any(|n| n == "monero_bridge_meta"))
        {
            bail!("refusing a non-bridge database");
        }
        sqlx::raw_sql(include_str!("schema.sql"))
            .execute(&mut *tx)
            .await?;
        sqlx::query("INSERT OR IGNORE INTO monero_bridge_meta(id,namespace,generation,clock) VALUES(1,?,?,0)")
            .bind(config.namespace()).bind(Uuid::new_v4().to_string()).execute(&mut *tx).await?;
        let row = sqlx::query("SELECT namespace,generation FROM monero_bridge_meta WHERE id=1")
            .fetch_one(&mut *tx)
            .await?;
        if row.get::<String, _>("namespace") != config.namespace() {
            bail!("bridge store identity mismatch");
        }
        let generation = row.get("generation");
        tx.commit().await?;
        Ok(Self {
            provider: Provider::new(
                &config.provider_url,
                Duration::from_secs(config.timeout_seconds),
                1024 * 1024,
            )?,
            work_slots: Arc::new(Semaphore::new(config.max_work as usize)),
            http_slots: Arc::new(Semaphore::new(config.max_http as usize)),
            config: Arc::new(config),
            token: Arc::new(zeroize::Zeroizing::new(token)),
            pool,
            generation,
        })
    }
    async fn clock(c: &mut SqliteConnection, at: i64) -> Result<()> {
        let high: i64 = sqlx::query_scalar("SELECT clock FROM monero_bridge_meta WHERE id=1")
            .fetch_one(&mut *c)
            .await?;
        if at <= 0 || at < high {
            bail!("bridge clock regressed");
        }
        sqlx::query("UPDATE monero_bridge_meta SET clock=? WHERE id=1")
            .bind(at)
            .execute(&mut *c)
            .await?;
        Ok(())
    }

    /// Durable reservation happens before the only upstream POST. Replay never POSTs.
    pub async fn create(&self, request: CreateReceive) -> Result<Snapshot> {
        request.validate()?;
        let encoded = serde_json::to_string(&request)?;
        let id = request.intent_id.to_string();
        let _slot = self.work_slots.try_acquire().context("bridge work busy")?;
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        if let Some(old) =
            sqlx::query_scalar::<_, String>("SELECT request FROM monero_bridge_intents WHERE id=?")
                .bind(&id)
                .fetch_optional(&mut *tx)
                .await?
        {
            if old != encoded {
                bail!("receive intent conflicts with durable request");
            }
            tx.commit().await?;
            return self.status(request.intent_id).await;
        }
        let at = now();
        Self::clock(&mut tx, at).await?;
        if !self.config.create_enabled
            || request.expected_atomic > self.config.max_amount_atomic
            || request.expires_at <= at
            || request.expires_at - at > 86400
        {
            bail!("new receive creation disabled or outside policy");
        }
        SyncReport::read(&self.config, at)?;
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM monero_bridge_intents")
            .fetch_one(&mut *tx)
            .await?;
        let recent: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM monero_bridge_intents WHERE reserved_at>?")
                .bind(at.saturating_sub(3600))
                .fetch_one(&mut *tx)
                .await?;
        if count >= self.config.max_intents as i64
            || recent >= self.config.max_creates_per_hour as i64
        {
            bail!("receive creation capacity reached; history retained");
        }
        let callback_token = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
        sqlx::query("INSERT INTO monero_bridge_intents(id,request,reserved_at,callback_token) VALUES(?,?,?,?)")
            .bind(&id).bind(&encoded).bind(at).bind(&callback_token).execute(&mut *tx).await?;
        tx.commit().await?;
        let description = format!("lg-monero/{id}");
        let callback = format!(
            "http://{}/callbacks/{id}/{callback_token}",
            self.config.callback_listen
        );
        let response = self
            .provider
            .create(request.expected_atomic, &description, &callback)
            .await;
        match response {
            Ok(created)
                if created.amount == request.expected_atomic
                    && created.description == description
                    && address::valid(&created.address, &self.config.network)
                    && provider::time(&created.created_at).is_ok() =>
            {
                let result = sqlx::query("UPDATE monero_bridge_intents SET created=?,address=?,state='bound' WHERE id=? AND state='creating'")
                    .bind(serde_json::to_string(&created)?).bind(&created.address).bind(&id).execute(&self.pool).await;
                if result.is_err() {
                    self.creation_unknown(&id).await?;
                }
            }
            _ => self.creation_unknown(&id).await?,
        }
        drop(_slot);
        // A failed initial read preserves the binding; status withholds the address
        // until authoritative readback succeeds. Never repeat create to recover it.
        let _ = self.reconcile(request.intent_id).await;
        self.status(request.intent_id).await
    }
    async fn creation_unknown(&self, id: &str) -> Result<()> {
        sqlx::query("UPDATE monero_bridge_intents SET state='creation_unknown' WHERE id=? AND state='creating'")
            .bind(id).execute(&self.pool).await?;
        Ok(())
    }

    pub async fn status(&self, id: Uuid) -> Result<Snapshot> {
        let row = sqlx::query("SELECT * FROM monero_bridge_intents WHERE id=?")
            .bind(id.to_string())
            .fetch_optional(&self.pool)
            .await?
            .context("unknown project intent")?;
        let req: CreateReceive = serde_json::from_str(&row.get::<String, _>("request"))?;
        let observed_at: Option<i64> = row.get("observed_at");
        let hold: Option<String> = row.get("hold");
        let snapshot: String = row.get("snapshot");
        let at = now();
        let high: i64 = sqlx::query_scalar("SELECT clock FROM monero_bridge_meta WHERE id=1")
            .fetch_one(&self.pool)
            .await?;
        let report = SyncReport::read(&self.config, at).ok();
        let ready = hold.is_none()
            && row.get::<i64, _>("available") == 1
            && at >= high
            && observed_at
                .is_some_and(|t| t <= at && at - t <= self.config.freshness_seconds as i64)
            && report.is_some_and(|r| r.wallet_height >= row.get::<i64, _>("wallet_height") as u64);
        Ok(Snapshot {
            version: PROTOCOL.to_owned(),
            generation: self.generation.clone(),
            intent_id: id,
            network: self.config.network.clone(),
            account_scope: self.config.account_scope.clone(),
            expected_atomic: req.expected_atomic,
            state: row.get("state"),
            address: if observed_at.is_some() {
                row.get("address")
            } else {
                None
            },
            observed_at,
            revision: row.get::<i64, _>("revision") as u64,
            ready,
            hold,
            receipts: serde_json::from_str(&snapshot)?,
        })
    }

    /// The callback body is deliberately not parsed or retained. One known bit is
    /// enough: the independent worker reads authoritative state at bounded cadence.
    pub async fn notify(&self, id: Uuid, token: &str) -> Result<()> {
        let saved: Option<String> =
            sqlx::query_scalar("SELECT callback_token FROM monero_bridge_intents WHERE id=?")
                .bind(id.to_string())
                .fetch_optional(&self.pool)
                .await?;
        if !saved.is_some_and(|s| web::secret_eq(&s, token)) {
            bail!("unknown callback");
        }
        // Coalescing, no growing callback inbox or caller-controlled provider URL.
        sqlx::query("UPDATE monero_bridge_intents SET dirty=1 WHERE id=? AND dirty=0")
            .bind(id.to_string())
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Full-history refresh, with a durable lease fencing stale concurrent results.
    pub async fn reconcile(&self, id: Uuid) -> Result<()> {
        let _slot = self.work_slots.try_acquire().context("bridge work busy")?;
        let at = now();
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        Self::clock(&mut tx, at).await?;
        let row = sqlx::query("SELECT * FROM monero_bridge_intents WHERE id=?")
            .bind(id.to_string())
            .fetch_optional(&mut *tx)
            .await?
            .context("unknown intent")?;
        if row.get::<String, _>("state") != "bound"
            || row.get::<Option<String>, _>("hold").is_some()
            || row.get::<i64, _>("lease_until") > at
        {
            return Ok(());
        }
        let created: Created = serde_json::from_str(&row.get::<String, _>("created"))?;
        let old: Vec<Observation> = serde_json::from_str(&row.get::<String, _>("snapshot"))?;
        let prior_height: i64 = row.get("wallet_height");
        let lease = Uuid::new_v4().to_string();
        let deadline = at + self.config.timeout_seconds as i64 * 3 + 5;
        sqlx::query("UPDATE monero_bridge_intents SET lease=?,lease_until=?,last_attempt=?,dirty=0,available=0 WHERE id=?")
            .bind(&lease).bind(deadline).bind(at).bind(id.to_string()).execute(&mut *tx).await?;
        tx.commit().await?;
        let result = tokio::time::timeout(
            Duration::from_secs(self.config.timeout_seconds * 3),
            async {
                let first = SyncReport::read(&self.config, now())?;
                self.provider.health().await?;
                let received = self.provider.receive(&created.address).await?;
                let report = SyncReport::read(&self.config, now())?;
                if report.wallet_height < first.wallet_height
                    || report.checked_at < first.checked_at
                {
                    bail!("sync evidence changed during read");
                }
                Ok::<_, anyhow::Error>((received, report))
            },
        )
        .await;
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let current: String =
            sqlx::query_scalar("SELECT lease FROM monero_bridge_intents WHERE id=?")
                .bind(id.to_string())
                .fetch_one(&mut *tx)
                .await?;
        if current != lease || now() >= deadline {
            return Ok(());
        }
        Self::clock(&mut tx, now()).await?;
        let (mut received, report) = match result {
            Ok(Ok(value)) => value,
            _ => {
                sqlx::query("UPDATE monero_bridge_intents SET lease_until=0 WHERE id=?")
                    .bind(id.to_string())
                    .execute(&mut *tx)
                    .await?;
                tx.commit().await?;
                return Ok(());
            }
        };
        // MoneroPay refresh/read may advance beyond the last independent probe.
        // That is insufficient fresh evidence, not proof of a permanent conflict.
        // Wait for a newer report without exposing or committing this observation.
        if received
            .transactions
            .as_ref()
            .is_some_and(|entries| entries.iter().any(|t| t.height > report.wallet_height))
        {
            sqlx::query("UPDATE monero_bridge_intents SET lease_until=0 WHERE id=?")
                .bind(id.to_string())
                .execute(&mut *tx)
                .await?;
            tx.commit().await?;
            return Ok(());
        }
        let invalid = received
            .validate(
                &created,
                self.config.max_receipts as usize,
                report.wallet_height,
            )
            .is_err()
            || report.wallet_height < prior_height as u64;
        let observed = now();
        let mut observations = Vec::new();
        let mut conflict = invalid;
        let entries = received.transactions.unwrap_or_default();
        for previous in &old {
            match entries
                .iter()
                .find(|t| t.tx_hash == previous.transaction.tx_hash)
            {
                None => conflict = true,
                Some(t) => {
                    let p = &previous.transaction;
                    if p.amount != t.amount
                        || p.unlock_time != t.unlock_time
                        || (!p.locked && t.locked)
                        || t.confirmations < p.confirmations
                        || (p.height > 0 && p.height != t.height)
                    {
                        conflict = true;
                    }
                }
            }
        }
        if !conflict {
            for t in entries {
                let first_seen_at = old
                    .iter()
                    .find(|p| p.transaction.tx_hash == t.tx_hash)
                    .map(|p| p.first_seen_at)
                    .unwrap_or(observed);
                observations.push(Observation {
                    transaction: t,
                    first_seen_at,
                });
            }
            sqlx::query("UPDATE monero_bridge_intents SET snapshot=?,observed_at=?,wallet_height=?,revision=revision+1,available=1,lease_until=0 WHERE id=?")
                .bind(serde_json::to_string(&observations)?).bind(observed).bind(report.wallet_height as i64)
                .bind(id.to_string()).execute(&mut *tx).await?;
        } else {
            sqlx::query("UPDATE monero_bridge_intents SET hold='provider_history_conflict',available=0,lease_until=0,revision=revision+1 WHERE id=?")
                .bind(id.to_string()).execute(&mut *tx).await?;
        }
        tx.commit().await?;
        Ok(())
    }

    /// Fair oldest-attempt scheduling includes paid/expired mappings forever.
    /// Callbacks coalesce hints; they cannot continually jump ahead of old work.
    pub async fn poll_once(&self) -> Result<()> {
        let ids: Vec<String> = sqlx::query_scalar("SELECT id FROM monero_bridge_intents WHERE state='bound' AND hold IS NULL AND lease_until<=? AND last_attempt<=? ORDER BY last_attempt,id LIMIT 16")
            .bind(now()).bind(now()-self.config.poll_seconds as i64).fetch_all(&self.pool).await?;
        for id in ids {
            self.reconcile(Uuid::parse_str(&id)?).await?;
        }
        Ok(())
    }
}
