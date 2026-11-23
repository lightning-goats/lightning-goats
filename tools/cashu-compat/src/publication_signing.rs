//! Default-off durable signing worker. It never grants credit or publishes to a relay.
use crate::{
    journal::Journal,
    publication_broker::{Request, Signed, ipc::Client},
    service::Runtime,
};
use anyhow::{Result, ensure};
use async_trait::async_trait;
use serde::Deserialize;
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration,
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub socket: PathBuf,
    pub signer_uid: u32,
    #[serde(default)]
    pub announcements: bool,
    #[serde(default)]
    pub redemptions: bool,
    #[serde(default)]
    pub maximum_attempts: u32,
}
impl Config {
    pub fn enabled(&self) -> bool {
        self.announcements || self.redemptions
    }
    pub fn validate(&self, wallet_uid: u32, core_uid: u32) -> Result<()> {
        crate::private_ipc::socket_path(&self.socket)?;
        ensure!(
            self.signer_uid > 0 && self.signer_uid != wallet_uid && self.signer_uid != core_uid,
            "signing requires a separate non-root identity"
        );
        ensure!(
            self.maximum_attempts <= 10_000 && (!self.enabled() || self.maximum_attempts > 0),
            "signing requires explicit bounded retries"
        );
        Ok(())
    }
}
#[async_trait]
pub trait Signer: Send + Sync {
    async fn sign(&self, request: &Request) -> Result<Signed>;
}
#[async_trait]
impl Signer for Client {
    async fn sign(&self, request: &Request) -> Result<Signed> {
        Client::sign(self, request).await
    }
}
#[derive(Debug, PartialEq, Eq)]
pub enum Tick {
    Idle,
    Signed,
    RetryRetained,
}
pub struct Worker<'a> {
    journal: Arc<Mutex<Journal>>,
    config: Option<&'a Config>,
    owner: &'a str,
    client: Option<Client>,
    announcement: Option<crate::publication::Intent>,
}
impl<'a> Worker<'a> {
    pub fn new(runtime: &'a Runtime) -> Result<Self> {
        let config = runtime.config.publication_signing.as_ref();
        let client = match config {
            Some(config) if config.enabled() => {
                config.validate(runtime.config.runtime_uid, runtime.config.core_uid)?;
                Some(Client::new(config.socket.clone(), config.signer_uid)?)
            }
            _ => None,
        };
        Ok(Self {
            journal: runtime.journal.clone(),
            config,
            owner: &runtime.config.recipient,
            client,
            announcement: config
                .filter(|c| c.announcements)
                .map(|_| crate::publication::Intent::announcement(&runtime.config, 0))
                .transpose()?,
        })
    }
    pub async fn tick(&self, now: i64) -> Result<Tick> {
        match &self.client {
            Some(client) => self.tick_with(now, client).await,
            None => Ok(Tick::Idle),
        }
    }
    async fn tick_with(&self, now: i64, signer: &impl Signer) -> Result<Tick> {
        let Some(config) = self.config.filter(|config| config.enabled()) else {
            return Ok(Tick::Idle);
        };
        let (operation, request) = {
            let mut journal = self
                .journal
                .lock()
                .map_err(|_| anyhow::anyhow!("wallet journal unavailable"))?;
            if config.redemptions {
                journal.stage_next_redemption(self.owner, now)?;
            }
            let Some(operation) = journal.claim_publication_signing(
                now,
                config.maximum_attempts,
                config.announcements,
                config.redemptions,
            )?
            else {
                return Ok(Tick::Idle);
            };
            let request = journal.publication_signing_request(&operation)?;
            request
                .intent
                .require_current_announcement(self.announcement.as_ref())?;
            (operation, request)
        };
        let result = match signer.sign(&request).await {
            Ok(result) => result,
            Err(_) => return Ok(Tick::RetryRetained),
        };
        self.journal
            .lock()
            .map_err(|_| anyhow::anyhow!("wallet journal unavailable"))?
            .accept_signed_publication(&operation, &result)?;
        Ok(Tick::Signed)
    }
    pub async fn run(&self) -> Result<()> {
        loop {
            let now = i64::try_from(
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)?
                    .as_secs(),
            )?;
            // Optional presentation failures must not stop intake/acquisition/receipt delivery.
            // Claims already persist backoff; storage/validation failures are retried conservatively.
            let delay = if self.tick(now).await.is_err() { 30 } else { 1 };
            tokio::time::sleep(Duration::from_secs(delay)).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::publication::tests::{announcement, owner, sign};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use uuid::Uuid;
    struct Fixture {
        calls: AtomicUsize,
    }
    #[async_trait]
    impl Signer for Fixture {
        async fn sign(&self, request: &Request) -> Result<Signed> {
            if self.calls.fetch_add(1, Ordering::SeqCst) == 0 {
                anyhow::bail!("synthetic unavailable signer");
            }
            Ok(Signed {
                operation_id: request.operation_id,
                ciphertext: None,
                event_json: sign(request.intent.unsigned(None)?)?,
            })
        }
    }
    #[tokio::test]
    async fn missing_signer_does_not_block_later_work_and_retry_keeps_original_request()
    -> Result<()> {
        let dir = crate::test_directory()?;
        let epoch = Uuid::new_v4();
        let instance = Uuid::new_v4();
        let path = dir.path().join("journal.sqlite");
        let mut journal = Journal::open(&path, epoch, 8)?;
        journal.runtime_registration(instance, &[7; 32], true)?;
        let first = Uuid::new_v4();
        let second = Uuid::new_v4();
        for id in [first, second] {
            journal.prepare_announcement(id, &announcement())?;
        }
        let config = Config {
            socket: "/run/fixture/sign.sock".into(),
            signer_uid: 12345,
            announcements: true,
            redemptions: false,
            maximum_attempts: 2,
        };
        let owner = owner();
        let journal = Arc::new(Mutex::new(journal));
        let worker = Worker {
            journal: journal.clone(),
            config: Some(&config),
            owner: &owner,
            announcement: Some(announcement()),
            client: None,
        };
        let signer = Fixture { calls: 0.into() };
        assert_eq!(worker.tick_with(100, &signer).await?, Tick::RetryRetained);
        assert_eq!(worker.tick_with(101, &signer).await?, Tick::Signed);
        assert_eq!(worker.tick_with(102, &signer).await?, Tick::Idle);
        assert!(
            journal
                .lock()
                .unwrap()
                .signed_publication(&format!("announcement:{first}"))?
                .is_none()
        );
        drop(worker);
        drop(journal);
        let mut journal = Journal::open(&path, epoch, 8)?;
        journal.runtime_registration(instance, &[7; 32], false)?;
        let journal = Arc::new(Mutex::new(journal));
        let worker = Worker {
            journal: journal.clone(),
            config: Some(&config),
            owner: &owner,
            announcement: Some(announcement()),
            client: None,
        };
        assert_eq!(worker.tick_with(279, &signer).await?, Tick::Idle);
        assert_eq!(worker.tick_with(280, &signer).await?, Tick::Signed);
        assert!(
            journal
                .lock()
                .unwrap()
                .signed_publication(&format!("announcement:{first}"))?
                .is_some()
        );
        assert_eq!(signer.calls.load(Ordering::SeqCst), 3);
        let disabled = Config {
            announcements: false,
            ..config
        };
        let worker = Worker {
            journal,
            config: Some(&disabled),
            owner: &owner,
            announcement: Some(announcement()),
            client: None,
        };
        assert_eq!(worker.tick_with(500, &signer).await?, Tick::Idle);
        assert_eq!(signer.calls.load(Ordering::SeqCst), 3);
        Ok(())
    }
    #[tokio::test]
    async fn stale_announcement_never_contacts_signer_or_blocks_later_policy() -> Result<()> {
        let dir = crate::test_directory()?;
        let mut journal = Journal::open(&dir.path().join("journal.sqlite"), Uuid::new_v4(), 8)?;
        journal.runtime_registration(Uuid::new_v4(), &[7; 32], true)?;
        journal.prepare_announcement(Uuid::new_v4(), &announcement())?;
        let mut current = announcement();
        if let crate::publication::Intent::Announcement { relays, .. } = &mut current {
            *relays = vec!["wss://new-receiver.example/".into()];
        }
        journal.prepare_announcement(Uuid::new_v4(), &current)?;
        let config = Config {
            socket: "/run/signer/sign.sock".into(),
            signer_uid: 12345,
            announcements: true,
            redemptions: false,
            maximum_attempts: 2,
        };
        let owner = owner();
        let worker = Worker {
            journal: Arc::new(Mutex::new(journal)),
            config: Some(&config),
            owner: &owner,
            client: None,
            announcement: Some(current),
        };
        let signer = Fixture { calls: 0.into() };
        assert!(worker.tick_with(100, &signer).await.is_err());
        assert_eq!(signer.calls.load(Ordering::SeqCst), 0);
        assert_eq!(worker.tick_with(101, &signer).await?, Tick::RetryRetained);
        assert_eq!(signer.calls.load(Ordering::SeqCst), 1);
        Ok(())
    }
    #[test]
    fn configuration_is_off_without_explicit_purpose_and_attempt_budget() -> Result<()> {
        let mut config: Config = serde_json::from_value(
            serde_json::json!({"socket":"/run/signer/sign.sock","signer_uid":12345}),
        )?;
        config.validate(12346, 12347)?;
        assert!(!config.enabled());
        config.redemptions = true;
        assert!(config.validate(12346, 12347).is_err());
        config.maximum_attempts = 2;
        config.validate(12346, 12347)?;
        assert!(config.validate(12345, 12347).is_err());
        assert!(config.validate(12346, 12345).is_err());
        Ok(())
    }
}
