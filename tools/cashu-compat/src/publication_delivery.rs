//! Default-off routing and delivery of retained signed events; never financial authority.
use crate::{
    journal::Journal,
    publication::Intent,
    publication_relay::{self, Discovery},
    service::Runtime,
};
use anyhow::{Result, ensure};
use async_trait::async_trait;
use serde::Deserialize;
use std::{
    collections::BTreeSet,
    sync::{Arc, Mutex},
    time::Duration,
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    #[serde(default)]
    pub announcements: bool,
    #[serde(default)]
    pub redemptions: bool,
    #[serde(default)]
    pub base_relays: Vec<String>,
    #[serde(default)]
    pub discovery_relays: Vec<String>,
    #[serde(default)]
    pub maximum_attempts: u32,
}
impl Config {
    pub fn enabled(&self) -> bool {
        self.announcements || self.redemptions
    }
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.base_relays.len() <= 16
                && self.discovery_relays.len() <= 8
                && self.maximum_attempts <= 10_000,
            "publication delivery exceeds bounds"
        );
        ensure!(
            !self.enabled() || (!self.base_relays.is_empty() && self.maximum_attempts > 0),
            "publication delivery requires explicit destinations and attempts"
        );
        ensure!(
            !self.redemptions || !self.discovery_relays.is_empty(),
            "history delivery requires sender relay discovery"
        );
        for (values, discovery) in [(&self.base_relays, false), (&self.discovery_relays, true)] {
            let mut unique = BTreeSet::new();
            for raw in values {
                let url = crate::relay::validate_relay(raw)?;
                ensure!(
                    url.query().is_none()
                        && unique.insert(url.to_string())
                        && (!discovery || url.host_str() != Some("sendit.nosflare.com")),
                    "invalid or duplicate publication relay"
                );
            }
        }
        Ok(())
    }
}
#[async_trait]
pub trait Transport: Send + Sync {
    async fn discover(&self, relays: &[String], sender: &str, now: i64) -> Result<Discovery>;
    async fn publish(&self, relay: &str, raw: &str) -> Result<()>;
}
pub struct Relays;
#[async_trait]
impl Transport for Relays {
    async fn discover(&self, relays: &[String], sender: &str, now: i64) -> Result<Discovery> {
        publication_relay::discover(relays, sender, now).await
    }
    async fn publish(&self, relay: &str, raw: &str) -> Result<()> {
        publication_relay::publish(relay, raw).await
    }
}
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Tick {
    pub planned: bool,
    pub delivered: bool,
    pub retry_retained: bool,
}
pub struct Worker<'a> {
    journal: Arc<Mutex<Journal>>,
    config: Option<&'a Config>,
    owner: &'a str,
    announcement: Option<Intent>,
}
impl<'a> Worker<'a> {
    pub fn new(runtime: &'a Runtime) -> Result<Self> {
        if let Some(config) = &runtime.config.publication_delivery {
            config.validate()?;
        }
        Ok(Self {
            journal: runtime.journal.clone(),
            config: runtime.config.publication_delivery.as_ref(),
            owner: &runtime.config.recipient,
            announcement: runtime
                .config
                .publication_delivery
                .as_ref()
                .filter(|c| c.announcements)
                .map(|_| Intent::announcement(&runtime.config, 0))
                .transpose()?,
        })
    }
    pub async fn tick(&self, now: i64) -> Result<Tick> {
        self.tick_with(now, &Relays).await
    }
    async fn tick_with(&self, now: i64, transport: &impl Transport) -> Result<Tick> {
        let Some(config) = self.config.filter(|c| c.enabled()) else {
            return Ok(Tick::default());
        };
        let start = tokio::time::Instant::now();
        let mut outcome = Tick::default();
        match self.route_one(config, transport, now).await {
            Ok(planned) => outcome.planned = planned,
            Err(_) => outcome.retry_retained = true,
        }
        // A slow metadata query must not shorten the next relay attempt's backoff.
        let delivery_now = now
            .checked_add(i64::try_from(start.elapsed().as_secs())?)
            .ok_or_else(|| anyhow::anyhow!("publication clock overflow"))?;
        match self.deliver_one(config, transport, delivery_now).await {
            Ok(delivered) => outcome.delivered = delivered,
            Err(_) => outcome.retry_retained = true,
        }
        Ok(outcome)
    }
    async fn route_one(
        &self,
        config: &Config,
        transport: &impl Transport,
        now: i64,
    ) -> Result<bool> {
        let (id, intent) = {
            let mut journal = self
                .journal
                .lock()
                .map_err(|_| anyhow::anyhow!("wallet journal unavailable"))?;
            let Some(id) = journal.claim_publication_routing(
                now,
                config.maximum_attempts,
                config.announcements,
                config.redemptions,
            )?
            else {
                return Ok(false);
            };
            ensure!(
                journal.signed_publication(&id)?.is_some(),
                "signed publication missing"
            );
            let intent = journal.publication_intent(&id)?;
            intent.require_current_announcement(self.announcement.as_ref())?;
            ensure!(
                intent.owner() == self.owner,
                "publication owner differs from runtime"
            );
            (id, intent)
        };
        let discovery = match intent {
            Intent::Announcement { .. } => Discovery {
                read_relays: vec![],
                signed_list: None,
            },
            Intent::Redemption { sender, .. } => {
                tokio::time::timeout(
                    Duration::from_secs(25),
                    transport.discover(&config.discovery_relays, &sender, now),
                )
                .await??
            }
        };
        self.journal
            .lock()
            .map_err(|_| anyhow::anyhow!("wallet journal unavailable"))?
            .freeze_publication_routes(&id, &config.base_relays, &discovery, now)?;
        Ok(true)
    }
    async fn deliver_one(
        &self,
        config: &Config,
        transport: &impl Transport,
        now: i64,
    ) -> Result<bool> {
        let delivery = self
            .journal
            .lock()
            .map_err(|_| anyhow::anyhow!("wallet journal unavailable"))?
            .claim_publication_delivery_for(
                now,
                config.maximum_attempts,
                config.announcements,
                config.redemptions,
            )?;
        let Some(delivery) = delivery else {
            return Ok(false);
        };
        ensure!(
            lightning_goats_nostr_event::VerifiedEvent::from_json(&delivery.event)?
                .event()
                .pubkey
                == self.owner,
            "publication owner differs from runtime"
        );
        self.journal
            .lock()
            .map_err(|_| anyhow::anyhow!("wallet journal unavailable"))?
            .publication_intent(&delivery.publication)?
            .require_current_announcement(self.announcement.as_ref())?;
        tokio::time::timeout(
            Duration::from_secs(25),
            transport.publish(&delivery.relay, &delivery.event),
        )
        .await??;
        self.journal
            .lock()
            .map_err(|_| anyhow::anyhow!("wallet journal unavailable"))?
            .acknowledge_publication_relay(
                &delivery.publication,
                &delivery.relay,
                &delivery.event_id,
            )?;
        Ok(true)
    }
    pub async fn run(&self) -> Result<()> {
        loop {
            let now = i64::try_from(
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)?
                    .as_secs(),
            )?;
            let failed = self
                .tick(now)
                .await
                .map_or(true, |outcome| outcome.retry_retained);
            tokio::time::sleep(Duration::from_secs(if failed { 5 } else { 1 })).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        journal::{Admission, Transport as IntakeTransport, ValidatedIntake},
        publication::{
            Unsigned,
            tests::{announcement, envelope, owner, sign},
        },
    };
    use std::sync::atomic::{AtomicUsize, Ordering};
    use uuid::Uuid;
    struct Fixture {
        discoveries: AtomicUsize,
        sent: Mutex<Vec<(String, String)>>,
        list: String,
    }
    #[async_trait]
    impl Transport for Fixture {
        async fn discover(&self, _: &[String], sender: &str, _: i64) -> Result<Discovery> {
            assert_eq!(sender, owner());
            if self.discoveries.fetch_add(1, Ordering::SeqCst) == 0 {
                anyhow::bail!("synthetic unavailable metadata");
            }
            Ok(Discovery {
                read_relays: vec!["wss://read.example/".into()],
                signed_list: Some(self.list.clone()),
            })
        }
        async fn publish(&self, relay: &str, raw: &str) -> Result<()> {
            let mut sent = self.sent.lock().unwrap();
            sent.push((relay.into(), raw.into()));
            if sent.len() == 1 {
                anyhow::bail!("synthetic lost acknowledgment");
            }
            Ok(())
        }
    }
    fn config() -> Config {
        Config {
            announcements: true,
            redemptions: true,
            base_relays: vec!["wss://a.example/".into(), "wss://b.example/".into()],
            discovery_relays: vec!["wss://discovery.example/".into()],
            maximum_attempts: 2,
        }
    }
    fn seed(journal: &mut Journal) -> Result<Uuid> {
        let proofs = [crate::intake::tests::proof(
            crate::intake::tests::locked().secret.to_string(),
        )];
        let source = "ab".repeat(32);
        let author = owner();
        let Admission::New(id) = journal.admit(&ValidatedIntake {
            mint_id: "fixture",
            policy_version: "v1",
            transport: IntakeTransport::Nutzap,
            source_ref: &source,
            source_author: Some(&author),
            proofs: &proofs,
        })?
        else {
            anyhow::bail!("fixture must be fresh")
        };
        journal.prepare(id, b"synthetic-swap")?;
        journal.start_acquisition(id)?;
        journal.record_acquired(id, 7, 1, b"synthetic-owned-outputs")?;
        journal.prepare_redemption(id, &author, 100)?;
        let op = format!("redemption:{id}");
        journal.freeze_history_ciphertext(&op, &envelope(9))?;
        journal.freeze_signed_publication(&op, &sign(journal.publication_unsigned(&op)?)?)?;
        let ann = Uuid::new_v4();
        journal.prepare_announcement(ann, &announcement())?;
        let op = format!("announcement:{ann}");
        journal.freeze_signed_publication(&op, &sign(journal.publication_unsigned(&op)?)?)?;
        Ok(id)
    }
    #[tokio::test]
    async fn routing_failure_and_partial_ack_do_not_block_later_work_or_repeat_completed_relays()
    -> Result<()> {
        let dir = crate::test_directory()?;
        let epoch = Uuid::new_v4();
        let path = dir.path().join("journal.sqlite");
        let mut journal = Journal::open(&path, epoch, 8)?;
        let acquisition = seed(&mut journal)?;
        let config = config();
        config.validate()?;
        let owner = owner();
        let fixture = Fixture {
            discoveries: 0.into(),
            sent: Mutex::new(vec![]),
            list: sign(Unsigned {
                pubkey: owner.clone(),
                created_at: 100,
                kind: 10002,
                content: String::new(),
                tags: vec![vec![
                    "r".into(),
                    "wss://read.example/".into(),
                    "read".into(),
                ]],
            })?,
        };
        let journal = Arc::new(Mutex::new(journal));
        let worker = Worker {
            journal: journal.clone(),
            config: Some(&config),
            owner: &owner,
            announcement: Some(announcement()),
        };
        assert_eq!(
            worker.tick_with(100, &fixture).await?,
            Tick {
                retry_retained: true,
                ..Tick::default()
            }
        );
        assert_eq!(
            worker.tick_with(101, &fixture).await?,
            Tick {
                planned: true,
                retry_retained: true,
                delivered: false
            }
        );
        assert_eq!(
            worker.tick_with(102, &fixture).await?,
            Tick {
                delivered: true,
                ..Tick::default()
            }
        );
        drop(worker);
        drop(journal);
        let journal = Arc::new(Mutex::new(Journal::open(&path, epoch, 8)?));
        let worker = Worker {
            journal: journal.clone(),
            config: Some(&config),
            owner: &owner,
            announcement: Some(announcement()),
        };
        assert_eq!(worker.tick_with(129, &fixture).await?, Tick::default());
        assert_eq!(
            worker.tick_with(130, &fixture).await?,
            Tick {
                planned: true,
                delivered: true,
                retry_retained: false
            }
        );
        for now in [131, 132, 133] {
            assert!(worker.tick_with(now, &fixture).await?.delivered);
        }
        assert_eq!(worker.tick_with(200, &fixture).await?, Tick::default());
        let sent = fixture.sent.lock().unwrap();
        assert_eq!(sent.len(), 6);
        assert_eq!(sent[0], sent[5]);
        assert_eq!(
            sent.iter()
                .filter(|(relay, _)| relay == "wss://read.example/")
                .count(),
            1
        );
        assert_eq!(fixture.discoveries.load(Ordering::SeqCst), 2);
        assert_eq!(journal.lock().unwrap().state(acquisition)?, "acquired");
        assert_eq!(
            journal
                .lock()
                .unwrap()
                .export_receipt(acquisition)?
                .net_sats,
            7
        );
        Ok(())
    }
    #[tokio::test]
    async fn disabled_delivery_and_disabled_purpose_leave_pending_work_untouched() -> Result<()> {
        let dir = crate::test_directory()?;
        let mut journal = Journal::open(&dir.path().join("journal.sqlite"), Uuid::new_v4(), 8)?;
        seed(&mut journal)?;
        let fixture = Fixture {
            discoveries: 0.into(),
            sent: Mutex::new(vec![]),
            list: String::new(),
        };
        let mut config = config();
        config.announcements = false;
        config.redemptions = false;
        let journal = Arc::new(Mutex::new(journal));
        let owner = owner();
        let worker = Worker {
            journal: journal.clone(),
            config: Some(&config),
            owner: &owner,
            announcement: Some(announcement()),
        };
        assert_eq!(worker.tick_with(100, &fixture).await?, Tick::default());
        assert!(fixture.sent.lock().unwrap().is_empty());
        assert_eq!(fixture.discoveries.load(Ordering::SeqCst), 0);
        drop(worker);
        config.announcements = true;
        let worker = Worker {
            journal: journal.clone(),
            config: Some(&config),
            owner: &owner,
            announcement: Some(announcement()),
        };
        let tick = worker.tick_with(100, &fixture).await?;
        assert!(tick.planned && tick.retry_retained);
        assert_eq!(fixture.discoveries.load(Ordering::SeqCst), 0);
        drop(worker);
        config.announcements = false;
        config.redemptions = true;
        let worker = Worker {
            journal,
            config: Some(&config),
            owner: &owner,
            announcement: Some(announcement()),
        };
        assert_eq!(
            worker.tick_with(200, &fixture).await?,
            Tick {
                retry_retained: true,
                ..Tick::default()
            }
        );
        assert_eq!(fixture.sent.lock().unwrap().len(), 1); // Already-routed announcement remains paused.
        Ok(())
    }
    #[tokio::test]
    async fn already_routed_announcement_cannot_publish_stale_receiving_policy() -> Result<()> {
        let dir = crate::test_directory()?;
        let mut journal = Journal::open(&dir.path().join("journal.sqlite"), Uuid::new_v4(), 8)?;
        let id = Uuid::new_v4();
        let op = format!("announcement:{id}");
        journal.prepare_announcement(id, &announcement())?;
        let raw = sign(journal.publication_unsigned(&op)?)?;
        journal.freeze_signed_publication(&op, &raw)?;
        journal.freeze_publication_routes(
            &op,
            &["wss://a.example/".into()],
            &Discovery {
                read_relays: vec![],
                signed_list: None,
            },
            100,
        )?;
        let mut current = announcement();
        if let Intent::Announcement { relays, .. } = &mut current {
            *relays = vec!["wss://new-receiver.example/".into()];
        }
        let mut config = config();
        config.redemptions = false;
        let owner = owner();
        let journal = Arc::new(Mutex::new(journal));
        let worker = Worker {
            journal: journal.clone(),
            config: Some(&config),
            owner: &owner,
            announcement: Some(current),
        };
        let fixture = Fixture {
            discoveries: 0.into(),
            sent: Mutex::new(vec![]),
            list: String::new(),
        };
        assert_eq!(
            worker.tick_with(100, &fixture).await?,
            Tick {
                retry_retained: true,
                ..Tick::default()
            }
        );
        assert!(fixture.sent.lock().unwrap().is_empty());
        assert_eq!(fixture.discoveries.load(Ordering::SeqCst), 0);
        assert_eq!(journal.lock().unwrap().signed_publication(&op)?, Some(raw));
        assert!(
            journal
                .lock()
                .unwrap()
                .claim_publication_delivery(101, 2)?
                .is_none()
        );
        Ok(())
    }
    #[test]
    fn publication_requires_explicit_purpose_budget_and_safe_unique_routes() -> Result<()> {
        let mut empty: Config = serde_json::from_str("{}")?;
        empty.validate()?;
        assert!(!empty.enabled());
        empty.announcements = true;
        assert!(empty.validate().is_err());
        let valid = config();
        valid.validate()?;
        for raw in [
            "wss://127.0.0.1",
            "ws://relay.example",
            "wss://relay.example/?token=x",
        ] {
            let mut changed = config();
            changed.base_relays = vec![raw.into()];
            assert!(changed.validate().is_err());
        }
        let mut changed = config();
        changed.base_relays = vec!["wss://relay.example".into(), "wss://relay.example/".into()];
        assert!(changed.validate().is_err());
        let mut changed = config();
        changed.discovery_relays = vec!["wss://sendit.nosflare.com/".into()];
        assert!(changed.validate().is_err());
        let mut changed = config();
        changed.discovery_relays.clear();
        assert!(changed.validate().is_err());
        Ok(())
    }
}
