//! Purpose-limited NIP-46 broker. The payout UID can request only the exact
//! generated 9734 for its frozen invoice intent. No arbitrary events or publish.
use super::invoice::{Document, Intent, Request, UnsignedZap};
use crate::{nostr::NakClient, nostr_event::VerifiedEvent, private_ipc as ipc};
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
use tokio::{net::UnixListener, sync::Semaphore, time::timeout};

const MAX_FRAME: usize = 131072;
#[cfg(test)]
#[path = "signer_tests.rs"]
pub(crate) mod tests;
static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./signer-migrations");

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Policy {
    pub owner: String,
    pub client_pubkey: String,
    pub peer_uid: u32,
    pub relays: Vec<String>,
    pub max_net_sats: u64,
    pub max_new_per_hour: u16,
}
impl Policy {
    pub fn validate(&self) -> Result<()> {
        crate::nostr_event::validate_public_key(&self.owner)?;
        crate::nostr_event::validate_public_key(&self.client_pubkey)?;
        if self.peer_uid == 0
            || self.client_pubkey == self.owner
            || self.relays != super::invoice::receipt_relays(&self.relays)?
            || self.max_net_sats > i64::MAX as u64 / 1000
            || self.max_new_per_hour > 3600
        {
            bail!("invalid outgoing signer policy");
        }
        Ok(())
    }
    fn intent(&self, intent: &Intent) -> Result<()> {
        if intent.0.owner != self.owner
            || intent.0.relays != self.relays
            || intent.0.net_sats > self.max_net_sats
            || !intent.uses_nostr()
        {
            bail!("invoice intent is outside signer purpose or current policy");
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub enabled: bool,
    pub state_dir: PathBuf,
    pub socket: PathBuf,
    pub group_gid: u32,
    pub policy: Policy,
    #[serde(deserialize_with = "nostr_settings")]
    pub nostr: crate::config::NostrConfig,
}
fn nostr_settings<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> std::result::Result<crate::config::NostrConfig, D::Error> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Fields {
        nak_path: PathBuf,
        nak_config_path: PathBuf,
        bunker_pubkey: String,
        relays: Vec<String>,
        bunker_relays: Option<Vec<String>>,
    }
    let f = Fields::deserialize(deserializer)?;
    Ok(crate::config::NostrConfig {
        nak_path: f.nak_path,
        nak_config_path: f.nak_config_path,
        bunker_pubkey: f.bunker_pubkey,
        relays: f.relays,
        bunker_relays: f.bunker_relays,
    })
}
impl Config {
    /// Fresh empty purpose binding only. Never loads nak/client credentials.
    pub async fn initialize_state(&self, path: &Path) -> Result<()> {
        self.validate()?;
        ipc::protected_config(path)?;
        if self.enabled
            || self.policy.max_net_sats != 0
            || self.policy.max_new_per_hour != 0
            || *self != Config::load(path)?
            || ipc::uid()? == self.policy.peer_uid
        {
            bail!(
                "initialization requires unchanged disabled zero-admission policy and its own UID"
            );
        }
        ipc::private_state(&self.state_dir)?;
        let fresh = crate::state_bootstrap::NewJournal::begin(
            &self.state_dir,
            "signer.db",
            ipc::singleton("cyberherd-signer", &self.state_dir)?,
        )?;
        Store::connect(&fresh.url()?, self.policy.clone())
            .await?
            .close_after_initialization()
            .await;
        if *self != Config::load(path)? {
            bail!("initialization configuration changed");
        }
        fresh.publish().await
    }

    pub fn load(path: &Path) -> Result<Self> {
        let raw = std::fs::read_to_string(path)?;
        if raw.len() > 65536 {
            bail!("signer config exceeds limit");
        }
        let config: Self = toml::from_str(&raw)
            .map_err(|_| anyhow::anyhow!("invalid outgoing signer configuration fields"))?;
        config.validate()?;
        Ok(config)
    }
    pub fn validate(&self) -> Result<()> {
        self.policy.validate()?;
        ipc::socket_path(&self.socket)?;
        crate::nostr_event::validate_public_key(&self.nostr.bunker_pubkey)?;
        if !self.state_dir.is_absolute()
            || self.group_gid == 0
            || !self.nostr.nak_path.is_absolute()
            || self.nostr.nak_config_path != self.state_dir.join("nak")
            || self.nostr.bunker_pubkey != self.policy.owner
            || self.nostr.bunker_relays().is_empty()
            || self.nostr.bunker_relays().len() > 8
            || !self.nostr.relays.is_empty()
        {
            bail!(
                "signer requires isolated state, exact project identity and no publishing relays"
            );
        }
        for relay in self.nostr.bunker_relays() {
            crate::public_network::validate_destination(relay, "wss")?;
        }
        Ok(())
    }
    pub async fn run(self, path: &Path) -> Result<()> {
        self.validate()?;
        if !self.enabled {
            bail!("outgoing signer is disabled");
        }
        ipc::protected_config(path)?;
        ipc::private_state(&self.state_dir)?;
        if !self.enabled || Config::load(path)? != self || ipc::uid()? == self.policy.peer_uid {
            bail!("signer is disabled, changed or shares caller UID");
        }
        ipc::protected_config(&self.nostr.nak_path)?;
        let _singleton = ipc::singleton("cyberherd-signer", &self.state_dir)?;
        let db = self.state_dir.join("signer.db");
        let store = Store::connect(
            &format!(
                "sqlite://{}",
                db.to_str().context("state path must be UTF-8")?
            ),
            self.policy.clone(),
        )
        .await?;
        let key = zeroize::Zeroizing::new(
            crate::secrets::read_systemd_credential("nostr-client-key").await?,
        );
        validate_client_key(&self.policy, &key)?;
        let signer = NakClient::new(&self.nostr, key.to_string())?;
        ipc::runtime_group(
            self.socket.parent().context("missing socket parent")?,
            self.group_gid,
        )?;
        Server::bind(&self.socket, self.group_gid, self.policy.peer_uid)
            .await?
            .run(store, signer)
            .await
    }
}

fn validate_client_key(policy: &Policy, key: &str) -> Result<()> {
    crate::nostr::validate_client_credential(&policy.owner, &policy.client_pubkey, key)
}

#[derive(Clone)]
pub struct Store {
    pool: SqlitePool,
    policy: Policy,
}
impl Store {
    pub(crate) async fn close_after_initialization(self) {
        self.pool.close().await;
    }
    pub async fn connect(url: &str, policy: Policy) -> Result<Self> {
        policy.validate()?;
        let pool = crate::sqlite::connect_durable(url, 2).await?;
        let names: Vec<String> = sqlx::query_scalar(
            "SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%'",
        )
        .fetch_all(&pool)
        .await?;
        if names.iter().any(|name| {
            ![
                "_sqlx_migrations",
                "signer_binding",
                "signer_clock",
                "signer_intents",
                "signer_results",
            ]
            .contains(&name.as_str())
        }) {
            bail!("signer requires its own database");
        }
        MIGRATOR.run(&pool).await?;
        sqlx::query("INSERT INTO signer_binding(singleton,owner,peer_uid) VALUES(1,?,?) ON CONFLICT(singleton) DO NOTHING")
            .bind(&policy.owner).bind(i64::from(policy.peer_uid)).execute(&pool).await?;
        let matches: bool = sqlx::query_scalar(
            "SELECT owner=? AND peer_uid=? FROM signer_binding WHERE singleton=1",
        )
        .bind(&policy.owner)
        .bind(i64::from(policy.peer_uid))
        .fetch_one(&pool)
        .await?;
        if !matches {
            bail!("signer identity changed; explicit recovery required");
        }
        Ok(Self { pool, policy })
    }
    async fn prepare(&self, intent: &Intent, now: i64) -> Result<Option<String>> {
        self.policy.intent(intent)?;
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let json = serde_json::to_string(&intent.0)?;
        let saved: Option<String> =
            sqlx::query_scalar("SELECT document_json FROM signer_intents WHERE id=?")
                .bind(intent.id().to_string())
                .fetch_optional(&mut *tx)
                .await?;
        if let Some(saved) = saved {
            if saved != json {
                bail!("signing retry changed frozen intent");
            }
        } else {
            let previous: i64 =
                sqlx::query_scalar("SELECT observed_at FROM signer_clock WHERE singleton=1")
                    .fetch_one(&mut *tx)
                    .await?;
            if now < previous || now < intent.0.created_at || now - intent.0.created_at > 300 {
                bail!("new signing intent has stale/backward time");
            }
            let used: i64 =
                sqlx::query_scalar("SELECT COUNT(*) FROM signer_intents WHERE created_at>?")
                    .bind(now.saturating_sub(3600))
                    .fetch_one(&mut *tx)
                    .await?;
            if used >= i64::from(self.policy.max_new_per_hour) {
                bail!("signer admission rate exhausted");
            }
            sqlx::query("INSERT INTO signer_intents(id,document_json,created_at) VALUES(?,?,?)")
                .bind(intent.id().to_string())
                .bind(json)
                .bind(now)
                .execute(&mut *tx)
                .await?;
            sqlx::query("UPDATE signer_clock SET observed_at=? WHERE singleton=1")
                .bind(now)
                .execute(&mut *tx)
                .await?;
        }
        let signed: Option<String> =
            sqlx::query_scalar("SELECT event_json FROM signer_results WHERE id=?")
                .bind(intent.id().to_string())
                .fetch_optional(&mut *tx)
                .await?;
        if let Some(raw) = &signed {
            Request::restore(intent.clone(), Some(raw.clone()))?;
        }
        tx.commit().await?;
        Ok(signed)
    }
    async fn freeze(&self, intent: &Intent, event: VerifiedEvent) -> Result<String> {
        let raw = serde_json::to_string(event.event())?;
        Request::restore(intent.clone(), Some(raw.clone()))?;
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let saved: String =
            sqlx::query_scalar("SELECT document_json FROM signer_intents WHERE id=?")
                .bind(intent.id().to_string())
                .fetch_one(&mut *tx)
                .await?;
        if saved != serde_json::to_string(&intent.0)? {
            bail!("signing intent changed");
        }
        sqlx::query(
            "INSERT INTO signer_results(id,event_json) VALUES(?,?) ON CONFLICT(id) DO NOTHING",
        )
        .bind(intent.id().to_string())
        .bind(raw)
        .execute(&mut *tx)
        .await?;
        let raw: String = sqlx::query_scalar("SELECT event_json FROM signer_results WHERE id=?")
            .bind(intent.id().to_string())
            .fetch_one(&mut *tx)
            .await?;
        Request::restore(intent.clone(), Some(raw.clone()))?;
        tx.commit().await?;
        Ok(raw)
    }
    async fn sign<B: Backend>(&self, intent: Intent, backend: &B) -> Result<String> {
        if let Some(raw) = self
            .prepare(&intent, crate::cyberherd::calendar::now()?)
            .await?
        {
            return Ok(raw);
        }
        let unsigned = intent
            .unsigned_zap()?
            .context("ordinary invoice needs no Nostr signing")?;
        let event = backend.sign(unsigned).await?;
        self.freeze(&intent, event).await
    }
}
pub(crate) trait Backend: Send + Sync + 'static {
    fn sign(
        &self,
        event: UnsignedZap,
    ) -> impl std::future::Future<Output = Result<VerifiedEvent>> + Send;
}
impl Backend for NakClient {
    async fn sign(&self, event: UnsignedZap) -> Result<VerifiedEvent> {
        self.sign_protocol_event(event.kind, event.created_at, &event.content, event.tags)
            .await
    }
}
#[derive(Serialize, Deserialize)]
#[serde(tag = "method", rename_all = "snake_case", deny_unknown_fields)]
enum Call {
    SignPayout { intent: Document },
}
#[derive(Serialize, Deserialize)]
#[serde(tag = "result", rename_all = "snake_case", deny_unknown_fields)]
enum Reply {
    Signed { event_json: String },
    Rejected,
}

pub struct Server {
    listener: UnixListener,
    peer_uid: u32,
}
impl Server {
    pub async fn bind(path: &Path, gid: u32, peer_uid: u32) -> Result<Self> {
        if peer_uid == 0 {
            bail!("signer caller must be non-root");
        }
        Ok(Self {
            listener: ipc::bind(path, gid).await?,
            peer_uid,
        })
    }
    pub async fn run(self, store: Store, signer: NakClient) -> Result<()> {
        self.run_with(store, signer).await
    }
    pub(crate) async fn run_with<B: Backend>(self, store: Store, backend: B) -> Result<()> {
        if self.peer_uid != store.policy.peer_uid {
            bail!("signer socket identity differs from journal policy");
        }
        let backend = Arc::new(backend);
        let work = Arc::new(Semaphore::new(1));
        loop {
            let (mut stream, _) = self.listener.accept().await?;
            if stream.peer_cred()?.uid() != self.peer_uid {
                continue;
            }
            let Ok(permit) = Arc::clone(&work).try_acquire_owned() else {
                continue;
            };
            let backend = Arc::clone(&backend);
            let store = store.clone();
            tokio::spawn(async move {
                let _permit = permit;
                let _ = timeout(Duration::from_secs(60), async {
                    let reply = match timeout(
                        Duration::from_secs(2),
                        ipc::read::<Call>(&mut stream, MAX_FRAME),
                    )
                    .await
                    {
                        Ok(Ok(Call::SignPayout { intent })) => match Intent::restore(intent) {
                            Ok(intent) => match store.sign(intent, backend.as_ref()).await {
                                Ok(event_json) => Reply::Signed { event_json },
                                Err(_) => Reply::Rejected,
                            },
                            Err(_) => Reply::Rejected,
                        },
                        _ => Reply::Rejected,
                    };
                    ipc::write(&mut stream, &reply, MAX_FRAME).await
                })
                .await;
            });
        }
    }
}
pub struct Client {
    socket: PathBuf,
    peer_uid: u32,
}
impl Client {
    pub fn new(socket: PathBuf, peer_uid: u32) -> Result<Self> {
        ipc::socket_path(&socket)?;
        if peer_uid == 0 {
            bail!("signer must be non-root");
        }
        Ok(Self { socket, peer_uid })
    }
    pub async fn sign(&self, intent: &Intent) -> Result<String> {
        let reply: Reply = ipc::request(
            &self.socket,
            self.peer_uid,
            &Call::SignPayout {
                intent: intent.0.clone(),
            },
            MAX_FRAME,
            65,
        )
        .await?;
        match reply {
            Reply::Signed { event_json } => {
                Request::restore(intent.clone(), Some(event_json.clone()))?;
                Ok(event_json)
            }
            Reply::Rejected => bail!("outgoing signer rejected or could not sign frozen intent"),
        }
    }
}
