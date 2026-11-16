//! Separate, purpose-limited signing state. No wallet seed, project nsec or publication.
use crate::{
    publication::{Intent, Unsigned},
    writer_lock::WriterLock,
};
use anyhow::{Result, ensure};
use async_trait::async_trait;
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use serde::{Deserialize, Serialize};
use std::path::Path;
use uuid::Uuid;

#[cfg(test)]
mod cross_uid_fixture;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Policy {
    pub broker_instance: Uuid,
    pub broker_epoch: Uuid,
    pub wallet_instance: Uuid,
    pub wallet_epoch: Uuid,
    pub wallet_uid: u32,
    pub owner: String,
    pub receiving_key: String,
    pub mints: Vec<String>,
    pub relays: Vec<String>,
    #[serde(default)]
    pub announcements: bool,
    #[serde(default)]
    pub redemptions: bool,
    #[serde(default)]
    pub max_history_sats: u64,
    #[serde(default)]
    pub max_new_per_hour: u16,
    #[serde(default)]
    pub max_attempts: u16,
}
impl Policy {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            !self.broker_instance.is_nil()
                && !self.broker_epoch.is_nil()
                && !self.wallet_instance.is_nil()
                && !self.wallet_epoch.is_nil()
                && self.wallet_uid > 0,
            "invalid Cashu signing registration"
        );
        self.announcement(0).validate()?;
        ensure!(
            self.max_history_sats <= i64::MAX as u64
                && self.max_new_per_hour <= 3600
                && self.max_attempts <= 100,
            "invalid Cashu signing limits"
        );
        ensure!(
            !(self.announcements || self.redemptions)
                || (self.max_new_per_hour > 0 && self.max_attempts > 0),
            "enabled signing requires explicit budgets"
        );
        ensure!(
            !self.redemptions || self.max_history_sats > 0,
            "history signing requires explicit amount bound"
        );
        Ok(())
    }
    fn announcement(&self, created_at: u64) -> Intent {
        let mut mints = self.mints.clone();
        let mut relays = self.relays.clone();
        mints.sort();
        relays.sort();
        Intent::Announcement {
            owner: self.owner.clone(),
            created_at,
            receiving_key: self.receiving_key.clone(),
            mints,
            relays,
        }
    }
    fn accepts(&self, intent: &Intent) -> Result<()> {
        intent.validate()?;
        ensure!(intent.owner() == self.owner, "signing owner mismatch");
        match intent {
            Intent::Announcement { created_at, .. } => ensure!(
                *intent == self.announcement(*created_at),
                "announcement differs from configured receiving policy"
            ),
            Intent::Redemption { net_sats, .. } => ensure!(
                *net_sats <= self.max_history_sats,
                "history exceeds signing amount policy"
            ),
        }
        Ok(())
    }
    fn enabled(&self, intent: &Intent) -> bool {
        match intent {
            Intent::Announcement { .. } => self.announcements,
            Intent::Redemption { .. } => self.redemptions,
        }
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub version: u8,
    pub wallet_instance: Uuid,
    pub wallet_epoch: Uuid,
    pub operation_id: Uuid,
    pub intent: Intent,
}
impl Request {
    pub fn decode(bytes: &[u8]) -> Result<Self> {
        ensure!(
            !bytes.is_empty() && bytes.len() <= 65_536,
            "Cashu signing request exceeds bound"
        );
        let request: Self = serde_json::from_slice(bytes)
            .map_err(|_| anyhow::anyhow!("invalid Cashu signing request"))?;
        request.validate()?;
        Ok(request)
    }
    fn validate(&self) -> Result<()> {
        ensure!(
            self.version == 1
                && !self.operation_id.is_nil()
                && !self.wallet_instance.is_nil()
                && !self.wallet_epoch.is_nil(),
            "invalid Cashu signing request registration"
        );
        self.intent.validate()?;
        ensure!(
            serde_json::to_vec(self)?.len() <= 65_536,
            "Cashu signing request exceeds bound"
        );
        Ok(())
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Signed {
    pub operation_id: Uuid,
    pub ciphertext: Option<String>,
    pub event_json: String,
}
impl Signed {
    pub fn validate_for(&self, request: &Request) -> Result<()> {
        request.validate()?;
        ensure!(
            self.operation_id == request.operation_id,
            "Cashu signing result identity mismatch"
        );
        request
            .intent
            .verify_signed(self.ciphertext.as_deref(), &self.event_json)
            .map_err(|_| anyhow::anyhow!("invalid Cashu signed result"))?;
        Ok(())
    }
}
/// Implement with the existing NIP-46 boundary. The recipient for both crypto
/// operations must be this broker's pinned project owner, never an arbitrary peer.
#[async_trait]
pub trait Backend: Send + Sync {
    async fn encrypt_self(&self, owner: &str, plaintext: &str) -> Result<String>;
    async fn decrypt_self(&self, owner: &str, ciphertext: &str) -> Result<String>;
    async fn sign(&self, unsigned: Unsigned) -> Result<String>;
}

pub struct Broker {
    connection: Connection,
    policy: Policy,
    _writer: WriterLock,
}
impl Broker {
    pub fn initialize(directory: &Path, policy: Policy) -> Result<Self> {
        Self::open_inner(directory, policy, true)
    }
    pub fn open(directory: &Path, policy: Policy) -> Result<Self> {
        Self::open_inner(directory, policy, false)
    }
    fn open_inner(directory: &Path, policy: Policy, initialize: bool) -> Result<Self> {
        policy.validate()?;
        let uid = rustix::process::geteuid().as_raw();
        ensure!(
            uid > 0 && uid != policy.wallet_uid,
            "signer requires a separate non-root runtime identity"
        );
        crate::private_ipc::private_state(directory)?;
        let writer = WriterLock::acquire(directory)?;
        let path = directory.join("cashu-signing.sqlite");
        if initialize {
            for suffix in ["", "-wal", "-shm", "-journal"] {
                match std::fs::symlink_metadata(
                    directory.join(format!("cashu-signing.sqlite{suffix}")),
                ) {
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                    Err(e) => return Err(e.into()),
                    Ok(_) => anyhow::bail!(
                        "signing state already exists; reconcile before initialization"
                    ),
                }
            }
        } else {
            let metadata = std::fs::symlink_metadata(&path)?;
            ensure!(
                metadata.is_file() && metadata.len() > 0,
                "signing state missing; restore before startup"
            );
        }
        let _private_file = crate::writer_lock::private_state_file(&path)?;
        let mut connection = Connection::open(&path)?;
        connection.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL; PRAGMA foreign_keys=ON; PRAGMA busy_timeout=1000;")?;
        if initialize {
            connection.execute_batch("CREATE TABLE registration(singleton INTEGER PRIMARY KEY CHECK(singleton=1),binding BLOB NOT NULL,last_clock INTEGER NOT NULL DEFAULT 0); CREATE TABLE requests(id TEXT PRIMARY KEY,request BLOB NOT NULL,source_key TEXT NOT NULL UNIQUE,source_event TEXT UNIQUE,admitted_at INTEGER NOT NULL,attempts INTEGER NOT NULL DEFAULT 0,next_attempt INTEGER NOT NULL DEFAULT 0,ciphertext TEXT,verified INTEGER NOT NULL DEFAULT 0 CHECK(verified IN (0,1)),signed_event TEXT,CHECK(verified=0 OR ciphertext IS NOT NULL));")?;
        }
        let binding = serde_json::to_vec(&(
            policy.broker_instance,
            policy.broker_epoch,
            policy.wallet_instance,
            policy.wallet_epoch,
            policy.wallet_uid,
            &policy.owner,
        ))?;
        let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        if initialize {
            tx.execute(
                "INSERT INTO registration(singleton,binding) VALUES(1,?)",
                [&binding],
            )?;
        }
        let retained: Vec<u8> = tx.query_row(
            "SELECT binding FROM registration WHERE singleton=1",
            [],
            |r| r.get(0),
        )?;
        ensure!(
            retained == binding,
            "Cashu signing registration changed; restore or reconcile"
        );
        tx.commit()?;
        Ok(Self {
            connection,
            policy,
            _writer: writer,
        })
    }
    /// Serialized by &mut self. Every external attempt is charged durably first.
    /// Errors never expose backend diagnostics; incomplete work retains its identity.
    pub async fn sign<B: Backend>(
        &mut self,
        request: &Request,
        backend: &B,
        now: i64,
    ) -> Result<Signed> {
        let cached = self.claim(request, now)?;
        if let Some(result) = cached {
            return Ok(result);
        }
        let id = request.operation_id.to_string();
        let mut ciphertext: Option<String> = self.connection.query_row(
            "SELECT ciphertext FROM requests WHERE id=?",
            [&id],
            |r| r.get(0),
        )?;
        if let Some(plaintext) = request.intent.encryption_plaintext()? {
            if ciphertext.is_none() {
                let candidate = backend
                    .encrypt_self(&self.policy.owner, &plaintext)
                    .await
                    .map_err(|_| anyhow::anyhow!("Cashu history encryption unavailable"))?;
                request.intent.unsigned(Some(&candidate))?;
                // Persist even before decrypt verification so a lost verify response
                // never silently replaces an already received encryption result.
                self.connection.execute(
                    "UPDATE requests SET ciphertext=? WHERE id=? AND ciphertext IS NULL",
                    params![candidate, id],
                )?;
                ciphertext = Some(candidate);
            }
            let verified: bool = self.connection.query_row(
                "SELECT verified FROM requests WHERE id=?",
                [&id],
                |r| r.get(0),
            )?;
            if !verified {
                let decrypted = backend
                    .decrypt_self(&self.policy.owner, ciphertext.as_deref().unwrap())
                    .await
                    .map_err(|_| anyhow::anyhow!("Cashu history authentication unavailable"))?;
                ensure!(
                    decrypted == plaintext,
                    "Cashu history authentication mismatch"
                );
                self.connection
                    .execute("UPDATE requests SET verified=1 WHERE id=?", [&id])?;
            }
        }
        let unsigned = request.intent.unsigned(ciphertext.as_deref())?;
        let raw = backend
            .sign(unsigned)
            .await
            .map_err(|_| anyhow::anyhow!("Cashu event signing unavailable"))?;
        let result = Signed {
            operation_id: request.operation_id,
            ciphertext,
            event_json: raw,
        };
        result.validate_for(request)?;
        self.connection.execute(
            "UPDATE requests SET signed_event=? WHERE id=? AND signed_event IS NULL",
            params![result.event_json, id],
        )?;
        Ok(result)
    }
    fn claim(&mut self, request: &Request, now: i64) -> Result<Option<Signed>> {
        request.validate()?;
        ensure!(
            request.wallet_instance == self.policy.wallet_instance
                && request.wallet_epoch == self.policy.wallet_epoch,
            "Cashu signing wallet registration mismatch"
        );
        self.policy.accepts(&request.intent)?;
        ensure!(now >= 0, "invalid signing clock");
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let id = request.operation_id.to_string();
        let encoded = serde_json::to_vec(request)?;
        let previous: Option<Vec<u8>> = tx
            .query_row("SELECT request FROM requests WHERE id=?", [&id], |r| {
                r.get(0)
            })
            .optional()?;
        if let Some(previous) = previous {
            ensure!(previous == encoded, "Cashu signing operation changed");
            let (ciphertext, event_json): (Option<String>, Option<String>) = tx.query_row(
                "SELECT ciphertext,signed_event FROM requests WHERE id=?",
                [&id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?;
            if let Some(event_json) = event_json {
                let result = Signed {
                    operation_id: request.operation_id,
                    ciphertext,
                    event_json,
                };
                result.validate_for(request)?;
                return Ok(Some(result));
            }
        } else {
            ensure!(
                self.policy.enabled(&request.intent),
                "Cashu signing purpose disabled"
            );
            let timestamp = match request.intent {
                Intent::Announcement { created_at, .. } | Intent::Redemption { created_at, .. } => {
                    created_at
                }
            };
            ensure!(
                timestamp <= now.saturating_add(300) as u64,
                "Cashu signing timestamp is in the future"
            );
            let last: i64 = tx.query_row(
                "SELECT last_clock FROM registration WHERE singleton=1",
                [],
                |r| r.get(0),
            )?;
            ensure!(now >= last, "Cashu signing clock moved backward");
            let count: i64 = tx.query_row(
                "SELECT COUNT(*) FROM requests WHERE admitted_at>?",
                [now.saturating_sub(3600)],
                |r| r.get(0),
            )?;
            let pending: i64 = tx.query_row(
                "SELECT COUNT(*) FROM requests WHERE signed_event IS NULL",
                [],
                |r| r.get(0),
            )?;
            ensure!(
                count < i64::from(self.policy.max_new_per_hour) && pending < 1024,
                "Cashu signing admission budget exhausted"
            );
            let (source_key, source_event) = match &request.intent {
                Intent::Announcement { .. } => (format!("announcement:{id}"), None),
                Intent::Redemption {
                    acquisition_id,
                    source_event,
                    ..
                } => (format!("redemption:{acquisition_id}"), Some(source_event)),
            };
            tx.execute("INSERT INTO requests(id,request,source_key,source_event,admitted_at) VALUES(?,?,?,?,?)",params![id,encoded,source_key,source_event,now])?;
        }
        ensure!(
            self.policy.enabled(&request.intent),
            "Cashu signing purpose disabled"
        );
        let last: i64 = tx.query_row(
            "SELECT last_clock FROM registration WHERE singleton=1",
            [],
            |r| r.get(0),
        )?;
        ensure!(now >= last, "Cashu signing clock moved backward");
        let (attempts, due): (u16, i64) = tx.query_row(
            "SELECT attempts,next_attempt FROM requests WHERE id=?",
            [&id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        ensure!(
            attempts < self.policy.max_attempts && now >= due,
            "Cashu signing retry not authorized yet or exhausted"
        );
        // Includes enough time for the future bounded three-step NIP-46 adapter.
        let next = now
            .checked_add(180)
            .ok_or_else(|| anyhow::anyhow!("Cashu signing clock overflow"))?;
        tx.execute(
            "UPDATE requests SET attempts=attempts+1,next_attempt=? WHERE id=?",
            params![next, id],
        )?;
        tx.execute(
            "UPDATE registration SET last_clock=? WHERE singleton=1",
            [now],
        )?;
        tx.commit()?;
        Ok(None)
    }
}

pub mod ipc {
    use super::*;
    use std::{path::PathBuf, time::Duration};
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::UnixStream,
    };
    const MAX_REPLY: usize = 131_072;
    #[derive(Serialize, Deserialize)]
    #[serde(tag = "result", rename_all = "snake_case", deny_unknown_fields)]
    enum Reply {
        Signed { value: Signed },
        Rejected,
    }
    impl Broker {
        pub async fn serve<B: Backend>(
            &mut self,
            path: &Path,
            group: u32,
            backend: &B,
            shutdown: impl std::future::Future<Output = ()>,
        ) -> Result<()> {
            let listener = crate::private_ipc::bind(path, group).await?;
            tokio::pin!(shutdown);
            loop {
                let (mut stream, _) = tokio::select! { _=&mut shutdown=>return Ok(()),accepted=listener.accept()=>accepted? };
                tokio::select! { _=&mut shutdown=>return Ok(()), _=self.connection(&mut stream,backend)=>{} }
                tokio::select! { _=&mut shutdown=>return Ok(()), _=tokio::time::sleep(Duration::from_secs(1))=>{} }
            }
        }
        pub async fn connection<B: Backend>(
            &mut self,
            stream: &mut UnixStream,
            backend: &B,
        ) -> Result<()> {
            ensure!(
                stream.peer_cred()?.uid() == self.policy.wallet_uid,
                "Cashu signer peer not authorized"
            );
            let request = tokio::time::timeout(Duration::from_secs(5), async {
                let length = usize::try_from(stream.read_u32().await?)?;
                ensure!(
                    length > 0 && length <= 65_536,
                    "Cashu signer frame exceeds bound"
                );
                let mut bytes = vec![0; length];
                stream.read_exact(&mut bytes).await?;
                Request::decode(&bytes)
            })
            .await
            .map_err(|_| anyhow::anyhow!("Cashu signer frame deadline exceeded"))??;
            let now = i64::try_from(
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)?
                    .as_secs(),
            )?;
            let reply = match tokio::time::timeout(
                Duration::from_secs(150),
                self.sign(&request, backend, now),
            )
            .await
            {
                Ok(Ok(value)) => Reply::Signed { value },
                _ => Reply::Rejected,
            };
            let bytes = serde_json::to_vec(&reply)?;
            ensure!(bytes.len() <= MAX_REPLY, "Cashu signer reply exceeds bound");
            tokio::time::timeout(Duration::from_secs(5), async {
                stream.write_u32(u32::try_from(bytes.len())?).await?;
                stream.write_all(&bytes).await?;
                stream.flush().await?;
                Ok::<_, anyhow::Error>(())
            })
            .await
            .map_err(|_| anyhow::anyhow!("Cashu signer reply deadline exceeded"))?
        }
    }
    pub struct Client {
        path: PathBuf,
        signer_uid: u32,
    }
    impl Client {
        pub fn new(path: PathBuf, signer_uid: u32) -> Result<Self> {
            crate::private_ipc::socket_path(&path)?;
            ensure!(
                rustix::process::geteuid().as_raw() > 0
                    && signer_uid > 0
                    && signer_uid != rustix::process::geteuid().as_raw(),
                "Cashu signer must use a separate identity"
            );
            Ok(Self { path, signer_uid })
        }
        pub async fn deliver(
            &self,
            journal: &std::sync::Arc<std::sync::Mutex<crate::journal::Journal>>,
            operation: &str,
        ) -> Result<()> {
            let request = journal
                .lock()
                .map_err(|_| anyhow::anyhow!("wallet journal unavailable"))?
                .publication_signing_request(operation)?;
            let result = self.sign(&request).await?;
            journal
                .lock()
                .map_err(|_| anyhow::anyhow!("wallet journal unavailable"))?
                .accept_signed_publication(operation, &result)
        }
        pub async fn sign(&self, request: &Request) -> Result<Signed> {
            request.validate()?;
            tokio::time::timeout(Duration::from_secs(165), async {
                let mut stream = UnixStream::connect(&self.path).await?;
                ensure!(
                    stream.peer_cred()?.uid() == self.signer_uid,
                    "Cashu signer peer not authorized"
                );
                let bytes = serde_json::to_vec(request)?;
                stream.write_u32(u32::try_from(bytes.len())?).await?;
                stream.write_all(&bytes).await?;
                stream.flush().await?;
                let length = usize::try_from(stream.read_u32().await?)?;
                ensure!(
                    length > 0 && length <= MAX_REPLY,
                    "Cashu signer reply exceeds bound"
                );
                let mut bytes = vec![0; length];
                stream.read_exact(&mut bytes).await?;
                let reply: Reply = serde_json::from_slice(&bytes)
                    .map_err(|_| anyhow::anyhow!("invalid Cashu signer reply"))?;
                let Reply::Signed { value } = reply else {
                    anyhow::bail!("Cashu signing request not completed");
                };
                value.validate_for(request)?;
                Ok(value)
            })
            .await
            .map_err(|_| anyhow::anyhow!("Cashu signer deadline exceeded"))?
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::publication::tests::{announcement, envelope, owner, sign};
    use std::sync::{
        Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    };
    struct Fixture {
        encrypts: AtomicUsize,
        decrypts: AtomicUsize,
        signs: AtomicUsize,
        fail_decrypt: AtomicBool,
        fail_sign: AtomicBool,
        wrong_plaintext: AtomicBool,
        wrong_event: AtomicBool,
        plaintext: Mutex<String>,
    }
    impl Fixture {
        fn new() -> Self {
            Self {
                encrypts: 0.into(),
                decrypts: 0.into(),
                signs: 0.into(),
                fail_decrypt: false.into(),
                fail_sign: false.into(),
                wrong_plaintext: false.into(),
                wrong_event: false.into(),
                plaintext: Mutex::new(String::new()),
            }
        }
    }
    #[async_trait]
    impl Backend for Fixture {
        async fn encrypt_self(&self, target: &str, plaintext: &str) -> Result<String> {
            assert_eq!(target, owner());
            self.encrypts.fetch_add(1, Ordering::SeqCst);
            *self.plaintext.lock().unwrap() = plaintext.into();
            Ok(envelope(9))
        }
        async fn decrypt_self(&self, target: &str, ciphertext: &str) -> Result<String> {
            assert_eq!(target, owner());
            assert_eq!(ciphertext, envelope(9));
            self.decrypts.fetch_add(1, Ordering::SeqCst);
            ensure!(
                !self.fail_decrypt.swap(false, Ordering::SeqCst),
                "synthetic sensitive backend diagnostic"
            );
            if self.wrong_plaintext.load(Ordering::SeqCst) {
                return Ok("wrong".into());
            }
            Ok(self.plaintext.lock().unwrap().clone())
        }
        async fn sign(&self, mut unsigned: Unsigned) -> Result<String> {
            self.signs.fetch_add(1, Ordering::SeqCst);
            ensure!(
                !self.fail_sign.swap(false, Ordering::SeqCst),
                "synthetic sensitive signing diagnostic"
            );
            if self.wrong_event.load(Ordering::SeqCst) {
                unsigned.kind = 1;
            }
            sign(unsigned)
        }
    }
    fn policy() -> Policy {
        Policy {
            broker_instance: Uuid::from_u128(1),
            broker_epoch: Uuid::from_u128(2),
            wallet_instance: Uuid::from_u128(3),
            wallet_epoch: Uuid::from_u128(4),
            wallet_uid: rustix::process::geteuid().as_raw() + 1,
            owner: owner(),
            receiving_key: cdk::nuts::SecretKey::from_slice(&[1; 32])
                .unwrap()
                .public_key()
                .x_only_public_key()
                .to_string(),
            mints: vec!["https://mint.example".into()],
            relays: vec!["wss://relay.example".into()],
            announcements: true,
            redemptions: true,
            max_history_sats: 100,
            max_new_per_hour: 8,
            max_attempts: 4,
        }
    }
    fn request() -> Request {
        Request {
            version: 1,
            wallet_instance: policy().wallet_instance,
            wallet_epoch: policy().wallet_epoch,
            operation_id: Uuid::new_v4(),
            intent: Intent::Redemption {
                owner: owner(),
                created_at: 100,
                acquisition_id: Uuid::new_v4(),
                source_event: "ab".repeat(32),
                sender: owner(),
                net_sats: 7,
            },
        }
    }
    #[tokio::test]
    async fn restored_signer_fence_prevents_initialization_and_reopening() -> Result<()> {
        let dir = crate::test_directory()?;
        let marker = dir.path().join("CASHU_RECOVERY_REQUIRED");
        std::fs::write(&marker, b"fixture signing history needs reconciliation")?;
        let error = Broker::initialize(dir.path(), policy())
            .err()
            .ok_or_else(|| anyhow::anyhow!("fenced signer initialized"))?;
        assert!(
            error
                .to_string()
                .contains("recovery requires explicit reconciliation")
        );
        assert!(!dir.path().join("cashu-signing.sqlite").exists());
        assert!(!dir.path().join("wallet-writer.lock").exists());
        // Clear only this synthetic fixture marker to create retained local history.
        std::fs::remove_file(&marker)?;
        drop(Broker::initialize(dir.path(), policy())?);
        let database = dir.path().join("cashu-signing.sqlite");
        let before = std::fs::read(&database)?;
        std::os::unix::fs::symlink(dir.path().join("missing"), &marker)?;
        let error = Broker::open(dir.path(), policy())
            .err()
            .ok_or_else(|| anyhow::anyhow!("fenced signer reopened"))?;
        assert!(
            error
                .to_string()
                .contains("recovery requires explicit reconciliation")
        );
        assert_eq!(std::fs::read(database)?, before);
        Ok(())
    }

    #[tokio::test]
    async fn decrypt_and_sign_failures_reopen_with_original_ciphertext_and_exact_result()
    -> Result<()> {
        let dir = crate::test_directory()?;
        let mut broker = Broker::initialize(dir.path(), policy())?;
        let backend = Fixture::new();
        let request = request();
        backend.fail_decrypt.store(true, Ordering::SeqCst);
        let error = broker.sign(&request, &backend, 100).await.err().unwrap();
        assert!(!format!("{error:#}").contains("sensitive"));
        assert_eq!(backend.encrypts.load(Ordering::SeqCst), 1);
        assert_eq!(backend.signs.load(Ordering::SeqCst), 0);
        assert!(broker.sign(&request, &backend, 279).await.is_err());
        drop(broker);
        let mut broker = Broker::open(dir.path(), policy())?;
        backend.fail_sign.store(true, Ordering::SeqCst);
        assert!(broker.sign(&request, &backend, 280).await.is_err());
        assert_eq!(backend.encrypts.load(Ordering::SeqCst), 1);
        assert_eq!(backend.decrypts.load(Ordering::SeqCst), 2);
        drop(broker);
        let mut broker = Broker::open(dir.path(), policy())?;
        let signed = broker.sign(&request, &backend, 460).await?;
        assert_eq!(backend.encrypts.load(Ordering::SeqCst), 1);
        assert_eq!(backend.decrypts.load(Ordering::SeqCst), 2);
        assert_eq!(backend.signs.load(Ordering::SeqCst), 2);
        drop(broker);
        let mut disabled = policy();
        disabled.announcements = false;
        disabled.redemptions = false;
        let mut broker = Broker::open(dir.path(), disabled)?;
        let replay = broker.sign(&request, &backend, 0).await?;
        assert_eq!(replay.event_json, signed.event_json);
        assert_eq!(replay.ciphertext, signed.ciphertext);
        assert_eq!(backend.signs.load(Ordering::SeqCst), 2);
        Ok(())
    }
    #[tokio::test]
    async fn mismatches_exhaustion_and_duplicate_source_never_sign_new_events() -> Result<()> {
        let dir = crate::test_directory()?;
        let mut p = policy();
        p.max_attempts = 1;
        let mut broker = Broker::initialize(dir.path(), p)?;
        let backend = Fixture::new();
        let request = request();
        backend.wrong_plaintext.store(true, Ordering::SeqCst);
        assert!(broker.sign(&request, &backend, 100).await.is_err());
        assert_eq!(backend.signs.load(Ordering::SeqCst), 0);
        backend.wrong_plaintext.store(false, Ordering::SeqCst);
        assert!(broker.sign(&request, &backend, 280).await.is_err());
        let mut replacement = request.clone();
        replacement.operation_id = Uuid::new_v4();
        assert!(broker.sign(&replacement, &backend, 280).await.is_err());
        if let Intent::Redemption { acquisition_id, .. } = &mut replacement.intent {
            *acquisition_id = Uuid::new_v4();
        }
        assert!(broker.sign(&replacement, &backend, 280).await.is_err());
        let mut changed = request.clone();
        if let Intent::Redemption { net_sats, .. } = &mut changed.intent {
            *net_sats = 8;
        }
        assert!(broker.sign(&changed, &backend, 280).await.is_err());
        assert_eq!(backend.encrypts.load(Ordering::SeqCst), 1);
        assert_eq!(
            broker
                .connection
                .query_row("SELECT COUNT(*) FROM requests", [], |r| r.get::<_, i64>(0))?,
            1
        );
        Ok(())
    }
    #[tokio::test]
    async fn announcement_policy_budgets_clock_and_native_signature_are_enforced() -> Result<()> {
        let dir = crate::test_directory()?;
        let mut p = policy();
        p.max_new_per_hour = 1;
        let mut broker = Broker::initialize(dir.path(), p)?;
        let backend = Fixture::new();
        let mut request = request();
        request.intent = announcement();
        let mut invalid = request.clone();
        if let Intent::Announcement { mints, .. } = &mut invalid.intent {
            mints[0] = "https://unapproved.example".into();
        }
        assert!(broker.sign(&invalid, &backend, 100).await.is_err());
        backend.wrong_event.store(true, Ordering::SeqCst);
        assert!(broker.sign(&request, &backend, 100).await.is_err());
        assert!(broker.sign(&request, &backend, 99).await.is_err());
        backend.wrong_event.store(false, Ordering::SeqCst);
        let signed = broker.sign(&request, &backend, 280).await?;
        assert!(signed.ciphertext.is_none());
        assert_eq!(backend.encrypts.load(Ordering::SeqCst), 0);
        let mut next = request.clone();
        next.operation_id = Uuid::new_v4();
        assert!(broker.sign(&next, &backend, 280).await.is_err());
        broker.sign(&next, &backend, 3700).await?;
        drop(broker);
        let mut changed = policy();
        changed.broker_epoch = Uuid::new_v4();
        assert!(Broker::open(dir.path(), changed).is_err());
        assert!(Broker::initialize(dir.path(), policy()).is_err());
        Ok(())
    }
    #[tokio::test]
    async fn disabled_policy_and_future_timestamps_fail_before_backend_contact() -> Result<()> {
        let dir = crate::test_directory()?;
        let mut disabled = policy();
        disabled.announcements = false;
        disabled.redemptions = false;
        disabled.max_history_sats = 0;
        disabled.max_new_per_hour = 0;
        disabled.max_attempts = 0;
        let mut broker = Broker::initialize(dir.path(), disabled)?;
        let backend = Fixture::new();
        let mut request = request();
        request.intent = announcement();
        assert!(broker.sign(&request, &backend, 100).await.is_err());
        drop(broker);
        let mut broker = Broker::open(dir.path(), policy())?;
        if let Intent::Announcement { created_at, .. } = &mut request.intent {
            *created_at = 401;
        }
        assert!(broker.sign(&request, &backend, 100).await.is_err());
        assert_eq!(backend.signs.load(Ordering::SeqCst), 0);
        assert_eq!(
            broker
                .connection
                .query_row("SELECT COUNT(*) FROM requests", [], |r| r.get::<_, i64>(0))?,
            0
        );
        Ok(())
    }
    #[tokio::test]
    async fn private_server_rejects_peer_before_reading_and_client_before_writing() -> Result<()> {
        use tokio::{
            io::AsyncReadExt,
            net::{UnixListener, UnixStream},
        };
        let dir = crate::test_directory()?;
        let mut broker = Broker::initialize(dir.path(), policy())?;
        let (mut server, _peer) = UnixStream::pair()?;
        assert!(
            tokio::time::timeout(
                std::time::Duration::from_secs(1),
                broker.connection(&mut server, &Fixture::new())
            )
            .await?
            .is_err()
        );
        let path = dir.path().join("signer.sock");
        let listener = UnixListener::bind(&path)?;
        let client = ipc::Client::new(path, policy().wallet_uid)?;
        let request = request();
        let (sent, received) = tokio::join!(client.sign(&request), async {
            let (mut peer, _) = listener.accept().await?;
            let mut bytes = Vec::new();
            peer.read_to_end(&mut bytes).await?;
            Ok::<_, anyhow::Error>(bytes)
        });
        assert!(sent.is_err());
        assert!(received?.is_empty());
        let mut wire = serde_json::to_value(&request)?;
        wire["arbitrary_kind"] = serde_json::json!(1);
        assert!(Request::decode(&serde_json::to_vec(&wire)?).is_err());
        assert!(Request::decode(&vec![b' '; 65_537]).is_err());
        Ok(())
    }
}
