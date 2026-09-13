//! Durable private operational alerts. No public event/ledger integration.
mod worker;
pub use worker::run_private_alert_worker;
mod policy;
use crate::{
    nostr::{NakClient, PrivateGiftWrap},
    strike::StrikeClient,
};
use anyhow::{Context, Result, bail};
use sha2::{Digest, Sha256};
use sqlx::{Row, SqlitePool};
use std::future::Future;

const MAX_PENDING: i64 = 128;

/// Protected runtime policy; deliberately not Debug or Serialize.
pub struct AlertPolicy {
    threshold_sats: u64,
    recipient: String,
    relays: Vec<String>,
    binding: String,
}
impl AlertPolicy {
    /// account_binding identifies the operator-reviewed provider account/config
    /// generation. It is not proof of the credential's effective account/scopes.
    pub fn new(
        threshold_sats: u64,
        recipient: String,
        relays: Vec<String>,
        account_binding: &str,
    ) -> Result<Self> {
        if threshold_sats == 0
            || recipient.len() != 64
            || !recipient
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            || account_binding.is_empty()
            || account_binding.len() > 256
        {
            bail!("invalid private alert policy");
        }
        crate::nostr::validate_private_relays(&relays)?;
        let encoded = serde_json::to_vec(&(threshold_sats, &recipient, &relays, account_binding))?;
        let binding = hex::encode(Sha256::digest(encoded));
        Ok(Self {
            threshold_sats,
            recipient,
            relays,
            binding,
        })
    }
}

#[derive(Clone)]
pub struct AlertStore {
    pool: SqlitePool,
    binding: String,
}
#[derive(Debug, PartialEq, Eq)]
pub enum Observation {
    BelowThreshold,
    AlreadyAlerted,
    Queued,
}
#[derive(Debug, PartialEq, Eq)]
pub enum Delivery {
    Empty,
    RetryPending,
    Delivered,
}

impl AlertStore {
    /// Use a dedicated file, never the financial or gateway ledger. Existing
    /// state is validated, not bootstrapped again if its singleton is missing.
    pub async fn connect(url: &str, policy: &AlertPolicy) -> Result<Self> {
        Self::open(url, policy, false).await
    }

    /// Explicit preparation only. Runtime startup must use connect, which never
    /// recreates forgotten episode state after loss of the database/tables.
    pub async fn initialize(url: &str, policy: &AlertPolicy) -> Result<Self> {
        Self::open(url, policy, true).await
    }

    async fn open(url: &str, policy: &AlertPolicy, initialize: bool) -> Result<Self> {
        let pool = crate::sqlite::connect_durable(url, 2).await?;
        let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
        let names: Vec<String> = sqlx::query_scalar("SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%' ORDER BY name")
            .fetch_all(&mut *tx).await?;
        if names.is_empty() {
            if !initialize {
                bail!("private alert database requires explicit initialization");
            }
            sqlx::query("CREATE TABLE private_alert_state (singleton INTEGER PRIMARY KEY CHECK(singleton=1), policy_binding TEXT NOT NULL, armed INTEGER NOT NULL CHECK(armed IN (0,1)))")
                .execute(&mut *tx).await?;
            sqlx::query("CREATE TABLE private_alert_outbox (seq INTEGER PRIMARY KEY AUTOINCREMENT, event_id TEXT UNIQUE NOT NULL CHECK(length(event_id)=64), ciphertext TEXT NOT NULL CHECK(length(CAST(ciphertext AS BLOB))<=65536), attempts INTEGER NOT NULL DEFAULT 0 CHECK(attempts>=0))")
                .execute(&mut *tx).await?;
            sqlx::query("PRAGMA user_version=1")
                .execute(&mut *tx)
                .await?;
            sqlx::query("INSERT INTO private_alert_state VALUES (1,?,1)")
                .bind(&policy.binding)
                .execute(&mut *tx)
                .await?;
        } else if initialize || names != ["private_alert_outbox", "private_alert_state"] {
            bail!("private alert database contains unrelated tables");
        }
        let version: i64 = sqlx::query_scalar("PRAGMA user_version")
            .fetch_one(&mut *tx)
            .await?;
        if version != 1 {
            bail!("unsupported private alert database version");
        }
        let state: Vec<(i64, String, i64)> =
            sqlx::query_as("SELECT singleton,policy_binding,armed FROM private_alert_state")
                .fetch_all(&mut *tx)
                .await?;
        if state.len() != 1
            || state[0].0 != 1
            || state[0].1 != policy.binding
            || ![0, 1].contains(&state[0].2)
        {
            bail!("private alert state/policy requires reconciliation");
        }
        tx.commit().await?;
        Ok(Self {
            pool,
            binding: policy.binding.clone(),
        })
    }

    /// The balance future starts only after durable cross-process serialization.
    /// Provider/wrapper errors roll back without advancing the episode state.
    pub async fn poll(
        &self,
        policy: &AlertPolicy,
        strike: &StrikeClient,
        nak: &NakClient,
    ) -> Result<Observation> {
        self.observe_with(policy,
            || async { Ok(strike.btc_balance().await?.current_sats()) },
            || nak.wrap_private_message(&policy.recipient, "The operational balance has reached the approved sweep trigger. Please review the Strike balance and perform the approved manual sweep.")
        ).await
    }

    async fn observe_with<B, BF, W, WF>(
        &self,
        policy: &AlertPolicy,
        balance: B,
        wrap: W,
    ) -> Result<Observation>
    where
        B: FnOnce() -> BF,
        BF: Future<Output = Result<u64>>,
        W: FnOnce() -> WF,
        WF: Future<Output = Result<PrivateGiftWrap>>,
    {
        self.check_policy(policy)?;
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let armed: i64 = sqlx::query_scalar(
            "SELECT armed FROM private_alert_state WHERE singleton=1 AND policy_binding=?",
        )
        .bind(&self.binding)
        .fetch_one(&mut *tx)
        .await?;
        if ![0, 1].contains(&armed) {
            bail!("private alert state invalid");
        }
        let current = balance()
            .await
            .map_err(|_| anyhow::anyhow!("private alert balance read failed"))?;
        let result = if current < policy.threshold_sats {
            sqlx::query("UPDATE private_alert_state SET armed=1 WHERE singleton=1")
                .execute(&mut *tx)
                .await?;
            Observation::BelowThreshold
        } else if armed == 0 {
            Observation::AlreadyAlerted
        } else if armed == 1 {
            let pending: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM private_alert_outbox")
                .fetch_one(&mut *tx)
                .await?;
            if pending >= MAX_PENDING {
                bail!("private alert pending capacity exhausted");
            }
            let message = wrap()
                .await
                .map_err(|_| anyhow::anyhow!("private alert encryption failed"))?;
            sqlx::query("INSERT INTO private_alert_outbox(event_id,ciphertext) VALUES (?,?)")
                .bind(message.event_id())
                .bind(message.ciphertext_json())
                .execute(&mut *tx)
                .await?;
            sqlx::query("UPDATE private_alert_state SET armed=0 WHERE singleton=1")
                .execute(&mut *tx)
                .await?;
            Observation::Queued
        } else {
            bail!("private alert state invalid");
        };
        tx.commit().await?;
        Ok(result)
    }

    /// At-least-once delivery of the same event bytes. A crash after relay ACK
    /// but before commit leaves this exact event available for harmless replay.
    pub async fn deliver_next(&self, policy: &AlertPolicy, nak: &NakClient) -> Result<Delivery> {
        self.check_policy(policy)?;
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let armed: i64 = sqlx::query_scalar(
            "SELECT armed FROM private_alert_state WHERE singleton=1 AND policy_binding=?",
        )
        .bind(&self.binding)
        .fetch_one(&mut *tx)
        .await?;
        if ![0, 1].contains(&armed) {
            bail!("private alert state invalid");
        }
        let row = sqlx::query(
            "SELECT seq,event_id,ciphertext FROM private_alert_outbox ORDER BY seq LIMIT 1",
        )
        .fetch_optional(&mut *tx)
        .await?;
        let Some(row) = row else {
            tx.commit().await?;
            return Ok(Delivery::Empty);
        };
        let seq: i64 = row.try_get("seq")?;
        let event_id: String = row.try_get("event_id")?;
        let ciphertext: String = row.try_get("ciphertext")?;
        let delivered: Result<()> = async {
            let wrap = nak
                .restore_private_wrap(&policy.recipient, &ciphertext)
                .await?;
            if wrap.event_id() != event_id {
                bail!("private alert event identity mismatch");
            }
            nak.publish_private_wrap(&policy.recipient, &policy.relays, &wrap)
                .await
        }
        .await;
        let result = if delivered.is_ok() {
            sqlx::query("DELETE FROM private_alert_outbox WHERE seq=?")
                .bind(seq)
                .execute(&mut *tx)
                .await?;
            Delivery::Delivered
        } else {
            sqlx::query(
                "UPDATE private_alert_outbox SET attempts=MIN(attempts+1,2147483647) WHERE seq=?",
            )
            .bind(seq)
            .execute(&mut *tx)
            .await?;
            Delivery::RetryPending
        };
        tx.commit()
            .await
            .context("private alert delivery commit failed")?;
        Ok(result)
    }
    fn check_policy(&self, policy: &AlertPolicy) -> Result<()> {
        if self.binding != policy.binding {
            bail!("private alert policy changed; reconciliation required");
        }
        Ok(())
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::config::NostrConfig;
    use std::{
        fs,
        os::unix::fs::PermissionsExt,
        sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        },
    };
    use tempfile::TempDir;
    struct Fixture {
        root: TempDir,
        policy: AlertPolicy,
        nak: NakClient,
        url: String,
    }
    impl Fixture {
        fn new() -> Self {
            let root = TempDir::new().unwrap();
            let executable = root.path().join("nak");
            fs::write(
                &executable,
                include_str!("../tests/fixtures/private_nak_stub.py"),
            )
            .unwrap();
            fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
            fs::write(root.path().join("mode"), "success").unwrap();
            let nak = NakClient::new(
                &NostrConfig {
                    nak_path: executable,
                    nak_config_path: root.path().into(),
                    bunker_pubkey: "ab".repeat(32),
                    relays: vec!["wss://announcement.invalid".into()],
                },
                "synthetic".into(),
            )
            .unwrap();
            let policy = AlertPolicy::new(
                100,
                "ab".repeat(32),
                vec!["wss://inbox.invalid".into()],
                "synthetic-account",
            )
            .unwrap();
            let url = format!("sqlite://{}", root.path().join("alerts.db").display());
            Self {
                root,
                policy,
                nak,
                url,
            }
        }
        async fn observe(&self, store: &AlertStore, amount: u64) -> Result<Observation> {
            store
                .observe_with(
                    &self.policy,
                    || async { Ok(amount) },
                    || {
                        self.nak.wrap_private_message(
                            &self.policy.recipient,
                            "SYNTHETIC-PLAINTEXT-NEVER-IN-DB",
                        )
                    },
                )
                .await
        }
        async fn rows(&self, store: &AlertStore) -> Vec<(String, String, i64)> {
            sqlx::query_as(
                "SELECT event_id,ciphertext,attempts FROM private_alert_outbox ORDER BY seq",
            )
            .fetch_all(&store.pool)
            .await
            .unwrap()
        }
        fn calls(&self) -> Vec<serde_json::Value> {
            fs::read_to_string(self.root.path().join("calls.jsonl"))
                .unwrap_or_default()
                .lines()
                .map(|s| serde_json::from_str(s).unwrap())
                .collect()
        }
    }
    #[tokio::test]
    async fn episode_rearms_only_below_threshold_and_retries_saved_bytes_after_reopen() {
        let f = Fixture::new();
        assert!(AlertStore::connect(&f.url, &f.policy).await.is_err());
        let store = AlertStore::initialize(&f.url, &f.policy).await.unwrap();
        assert!(AlertStore::initialize(&f.url, &f.policy).await.is_err());
        assert_eq!(f.observe(&store, 100).await.unwrap(), Observation::Queued);
        assert_eq!(
            f.observe(&store, 101).await.unwrap(),
            Observation::AlreadyAlerted
        );
        let saved = f.rows(&store).await;
        assert_eq!(saved.len(), 1);
        assert!(!saved[0].1.contains("SYNTHETIC-PLAINTEXT-NEVER-IN-DB"));
        fs::write(f.root.path().join("mode"), "publish_failure").unwrap();
        assert_eq!(
            store.deliver_next(&f.policy, &f.nak).await.unwrap(),
            Delivery::RetryPending
        );
        store.pool.close().await;
        let store = AlertStore::connect(&f.url, &f.policy).await.unwrap();
        let retried = f.rows(&store).await;
        assert_eq!((&retried[0].0, &retried[0].1), (&saved[0].0, &saved[0].1));
        assert_eq!(retried[0].2, 1);
        assert_eq!(
            f.observe(&store, 200).await.unwrap(),
            Observation::AlreadyAlerted
        );
        fs::write(f.root.path().join("mode"), "success").unwrap();
        assert_eq!(
            store.deliver_next(&f.policy, &f.nak).await.unwrap(),
            Delivery::Delivered
        );
        assert_eq!(
            f.observe(&store, 200).await.unwrap(),
            Observation::AlreadyAlerted
        );
        assert_eq!(
            f.observe(&store, 99).await.unwrap(),
            Observation::BelowThreshold
        );
        assert_eq!(f.observe(&store, 100).await.unwrap(), Observation::Queued);
        assert_ne!(f.rows(&store).await[0].0, saved[0].0);
        let calls = f.calls();
        assert_eq!(calls.iter().filter(|c| c["phase"] == "wrap").count(), 2);
        let published: Vec<_> = calls.iter().filter(|c| c["phase"] == "publish").collect();
        assert_eq!(published.len(), 2);
        assert!(published.iter().all(|c| c["raw"] == saved[0].1));
        let names:Vec<String>=sqlx::query_scalar("SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%' ORDER BY name")
            .fetch_all(&store.pool).await.unwrap();
        assert_eq!(names, ["private_alert_outbox", "private_alert_state"]);
    }
    #[tokio::test]
    async fn read_encrypt_and_database_failures_do_not_advance_state() {
        let f = Fixture::new();
        let store = AlertStore::initialize(&f.url, &f.policy).await.unwrap();
        assert!(
            store
                .observe_with(
                    &f.policy,
                    || async { bail!("provider error") },
                    || f.nak.wrap_private_message(&f.policy.recipient, "synthetic")
                )
                .await
                .is_err()
        );
        assert!(f.calls().is_empty());
        assert!(f.rows(&store).await.is_empty());
        fs::write(f.root.path().join("mode"), "no_encryption").unwrap();
        assert!(f.observe(&store, 100).await.is_err());
        assert!(f.rows(&store).await.is_empty());
        fs::write(f.root.path().join("mode"), "success").unwrap();
        sqlx::query("CREATE TRIGGER fail_insert BEFORE INSERT ON private_alert_outbox BEGIN SELECT RAISE(ABORT,'synthetic disk failure'); END")
            .execute(&store.pool).await.unwrap();
        assert!(f.observe(&store, 100).await.is_err());
        assert!(f.rows(&store).await.is_empty());
        sqlx::query("DROP TRIGGER fail_insert")
            .execute(&store.pool)
            .await
            .unwrap();
        assert_eq!(f.observe(&store, 100).await.unwrap(), Observation::Queued);
        assert!(
            store
                .observe_with(
                    &f.policy,
                    || async { bail!("read failed") },
                    || f.nak.wrap_private_message(&f.policy.recipient, "synthetic")
                )
                .await
                .is_err()
        );
        assert_eq!(
            f.observe(&store, 100).await.unwrap(),
            Observation::AlreadyAlerted
        );
        assert!(f.calls().iter().all(|c| c["phase"] != "publish"));
    }
    #[tokio::test]
    async fn independent_connections_lock_before_read_and_queue_one_event() {
        let f = Fixture::new();
        let a = AlertStore::initialize(&f.url, &f.policy).await.unwrap();
        let b = AlertStore::connect(&f.url, &f.policy).await.unwrap();
        let entered = Arc::new(AtomicUsize::new(0));
        let peak = Arc::new(AtomicUsize::new(0));
        let balance = || {
            let entered = entered.clone();
            let peak = peak.clone();
            async move {
                let count = entered.fetch_add(1, Ordering::SeqCst) + 1;
                peak.fetch_max(count, Ordering::SeqCst);
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                entered.fetch_sub(1, Ordering::SeqCst);
                Ok(100)
            }
        };
        let wrap = || f.nak.wrap_private_message(&f.policy.recipient, "synthetic");
        let (x, y) = tokio::join!(
            a.observe_with(&f.policy, balance, wrap),
            b.observe_with(&f.policy, balance, wrap)
        );
        let results = [x.unwrap(), y.unwrap()];
        assert!(results.contains(&Observation::Queued));
        assert!(results.contains(&Observation::AlreadyAlerted));
        assert_eq!(peak.load(Ordering::SeqCst), 1);
        assert_eq!(f.rows(&a).await.len(), 1);
        assert_eq!(f.calls().iter().filter(|c| c["phase"] == "wrap").count(), 1);
    }
    #[tokio::test]
    async fn relay_success_then_database_failure_replays_identical_event() {
        let f = Fixture::new();
        let store = AlertStore::initialize(&f.url, &f.policy).await.unwrap();
        f.observe(&store, 100).await.unwrap();
        let saved = f.rows(&store).await;
        sqlx::query("CREATE TRIGGER fail_delete BEFORE DELETE ON private_alert_outbox BEGIN SELECT RAISE(ABORT,'synthetic commit-window failure'); END")
            .execute(&store.pool).await.unwrap();
        assert!(store.deliver_next(&f.policy, &f.nak).await.is_err());
        assert_eq!(f.rows(&store).await, saved);
        store.pool.close().await;
        let store = AlertStore::connect(&f.url, &f.policy).await.unwrap();
        sqlx::query("DROP TRIGGER fail_delete")
            .execute(&store.pool)
            .await
            .unwrap();
        assert_eq!(
            store.deliver_next(&f.policy, &f.nak).await.unwrap(),
            Delivery::Delivered
        );
        let calls = f.calls();
        let published: Vec<_> = calls.iter().filter(|c| c["phase"] == "publish").collect();
        assert_eq!(published.len(), 2);
        assert!(published.iter().all(|c| c["raw"] == saved[0].1));
        assert_eq!(calls.iter().filter(|c| c["phase"] == "wrap").count(), 1);
    }

    #[tokio::test]
    async fn pending_capacity_rejects_before_encryption() {
        let f = Fixture::new();
        let store = AlertStore::initialize(&f.url, &f.policy).await.unwrap();
        for i in 0..MAX_PENDING {
            sqlx::query("INSERT INTO private_alert_outbox(event_id,ciphertext) VALUES (?,?)")
                .bind(format!("{i:064x}"))
                .bind("synthetic capacity fixture")
                .execute(&store.pool)
                .await
                .unwrap();
        }
        assert!(f.observe(&store, 100).await.is_err());
        assert!(f.calls().is_empty());
        assert_eq!(f.rows(&store).await.len(), MAX_PENDING as usize);
    }

    #[tokio::test]
    async fn changed_policy_missing_state_and_financial_database_are_rejected() {
        let f = Fixture::new();
        let store = AlertStore::initialize(&f.url, &f.policy).await.unwrap();
        let changed = AlertPolicy::new(
            101,
            "ab".repeat(32),
            vec!["wss://inbox.invalid".into()],
            "synthetic-account",
        )
        .unwrap();
        assert!(AlertStore::connect(&f.url, &changed).await.is_err());
        sqlx::query("DELETE FROM private_alert_state")
            .execute(&store.pool)
            .await
            .unwrap();
        assert!(f.observe(&store, 100).await.is_err());
        assert!(store.deliver_next(&f.policy, &f.nak).await.is_err());
        assert!(AlertStore::connect(&f.url, &f.policy).await.is_err());
        assert!(f.calls().is_empty());
        let url = format!("sqlite://{}", f.root.path().join("financial.db").display());
        let financial = crate::ledger::LedgerStore::connect(&url).await.unwrap();
        assert!(AlertStore::connect(&url, &f.policy).await.is_err());
        assert!(financial.next_outbox_entry().await.unwrap().is_none());
    }
}
