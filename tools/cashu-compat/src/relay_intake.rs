//! Default-off, serialized NIP-61 discovery and durable proof admission.
use crate::{
    journal::{
        AdmissionConflict,
        relay_scans::{Lane, Outcome, Registration},
    },
    relay::{Batch, Filter},
    service::Runtime,
    service_config,
};
use anyhow::{Result, ensure};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    #[serde(default)]
    pub enabled: bool,
    pub relays: Vec<String>,
    pub historical_floor: i64,
    pub window_seconds: i64,
    pub overlap_seconds: i64,
    pub event_limit: u16,
}
impl Config {
    pub fn validate(&self, receive_nutzap: bool) -> Result<()> {
        ensure!(
            !self.enabled || receive_nutzap,
            "relay intake requires nutzap admission"
        );
        ensure!(
            !self.relays.is_empty()
                && self.relays.len() <= 8
                && self.historical_floor >= 0
                && self.historical_floor <= i64::MAX - 30
                && (1..=86_400).contains(&self.window_seconds)
                && self.overlap_seconds >= 0
                && self.overlap_seconds < self.window_seconds
                && (1..=256).contains(&self.event_limit),
            "invalid bounded relay intake configuration"
        );
        let mut unique = BTreeSet::new();
        for relay in &self.relays {
            let url = crate::relay::validate_relay(relay)?;
            ensure!(unique.insert(url.to_string()), "duplicate receive relay");
        }
        Ok(())
    }
}

/// Hash the entire admission inventory, not only advertised URLs. Sorting makes
/// harmless inventory reordering stable; changed keys or limits restart discovery.
fn scope(config: &service_config::Config, relay: &Config) -> Result<[u8; 32]> {
    let mut mints: Vec<_> = config
        .mints
        .iter()
        .map(|m| {
            (
                &m.id,
                &m.advertised_url,
                &m.policy_version,
                m.receive_cashu,
                m.receive_nutzap,
                m.bolt11_exit_qualified,
            )
        })
        .collect();
    mints.sort();
    let mut keys: Vec<_> = config
        .keysets
        .iter()
        .map(|k| (&k.mint_id, &k.id, &k.keys))
        .collect();
    keys.sort();
    let mut inventory = serde_json::json!({
        "version":1,"instance":config.wallet_instance,"epoch":config.wallet_epoch,
        "recipient":config.recipient,"receiving_key":config.receiving_key,
        "mints":mints,"keysets":keys,"body_bytes":config.body_bytes,
        "proof_count":config.proof_count,"receipt_sats":config.receipt_sats,
        "floor":relay.historical_floor,"span":relay.window_seconds,
        "overlap":relay.overlap_seconds,"limit":relay.event_limit
    });
    let mut historical = config
        .retained_receiving_keys
        .iter()
        .filter(|key| key.nutzap.is_some())
        .collect::<Vec<_>>();
    historical.sort_by_key(|key| &key.public_key);
    if !historical.is_empty() {
        inventory["retained_receiving_keys"] = serde_json::to_value(historical)?;
    }
    let inventory = serde_json::to_vec(&inventory)?;
    let mut hash = Sha256::new();
    hash.update(b"lightning-goats-nutzap-discovery-v1\0");
    hash.update(inventory);
    Ok(hash.finalize().into())
}

#[derive(Debug, PartialEq, Eq)]
pub enum Tick {
    Idle,
    Scanned {
        admitted: u16,
        rejected: u16,
        complete: bool,
    },
}
pub struct Worker<'a> {
    runtime: &'a Runtime,
    registrations: Vec<Registration>,
    mints: Vec<String>,
    next: usize,
    limit: u16,
}
impl<'a> Worker<'a> {
    /// Offline assembly; no state is mutated and no relay is contacted.
    pub fn new(runtime: &'a Runtime) -> Result<Self> {
        let mut registrations = Vec::new();
        let mut mints = Vec::new();
        let mut limit = 1;
        if let Some(config) = &runtime.config.relay_intake {
            config.validate(runtime.config.receive_nutzap)?;
            if config.enabled {
                let scope = scope(&runtime.config, config)?;
                for relay in &config.relays {
                    registrations.push(Registration {
                        scope,
                        relay: relay.clone(),
                        floor: config.historical_floor,
                        span: config.window_seconds,
                        overlap: config.overlap_seconds,
                    });
                }
                mints = runtime
                    .config
                    .mints
                    .iter()
                    .filter(|m| m.receive_nutzap && m.bolt11_exit_qualified)
                    .map(|m| m.advertised_url.clone())
                    .collect();
                limit = config.event_limit;
                Filter {
                    recipient: runtime.config.recipient.clone(),
                    mints: mints.clone(),
                    since: 0,
                    until: 0,
                    limit,
                }
                .validate()?;
            }
        }
        Ok(Self {
            runtime,
            registrations,
            mints,
            next: 0,
            limit,
        })
    }
    pub async fn tick(&mut self, now: i64) -> Result<Tick> {
        self.tick_with(now, |relay, filter| async move {
            crate::relay::query(&relay, &filter).await
        })
        .await
    }
    async fn tick_with<F, Fut>(&mut self, now: i64, query: F) -> Result<Tick>
    where
        F: FnOnce(String, Filter) -> Fut,
        Fut: std::future::Future<Output = Result<Batch>>,
    {
        if self.registrations.is_empty() {
            return Ok(Tick::Idle);
        }
        // Round robin across both lanes and all configured relays, including failures.
        let slot = self.next;
        self.next = (self.next + 1) % (self.registrations.len() * 2);
        let registration = &self.registrations[slot / 2];
        if now < registration.floor {
            return Ok(Tick::Idle);
        }
        let lane = if slot % 2 == 0 {
            Lane::Recent
        } else {
            Lane::History
        };
        let claim = {
            let mut journal = self
                .runtime
                .journal
                .lock()
                .map_err(|_| anyhow::anyhow!("wallet journal unavailable"))?;
            journal.require_epoch(self.runtime.config.wallet_epoch)?;
            journal.claim_relay_scan(registration, lane, now)?
        };
        let Some(claim) = claim else {
            return Ok(Tick::Idle);
        };
        let filter = Filter {
            recipient: self.runtime.config.recipient.clone(),
            mints: self.mints.clone(),
            since: claim.since as u64,
            until: claim.until as u64,
            limit: self.limit,
        };
        let batch = query(registration.relay.clone(), filter).await;
        let (mut admitted, mut rejected) = (0_u16, 0_u16);
        let mut outcome = Outcome::Incomplete;
        let mut complete = false;
        if let Ok(batch) = batch {
            ensure!(
                batch.events.len() <= usize::from(self.limit),
                "relay reader exceeded event budget"
            );
            // Bound processing as well as network work. Unprocessed input is replayed.
            let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
            let mut processed_all = true;
            for event in batch.events {
                if tokio::time::Instant::now() >= deadline {
                    processed_all = false;
                    break;
                }
                let body = serde_json::to_string(event.event())?;
                match crate::intake::nutzap_json_at(
                    &body,
                    &self.runtime.config.recipient,
                    &self.runtime.policy.registry,
                    &self.runtime.policy.keys,
                    &self.runtime.policy.limits,
                    crate::intake::NutzapReceivers {
                        current: self.runtime.policy.receiver,
                        retained: &self.runtime.config.retained_receiving_keys,
                        now: u64::try_from(clock()?)?,
                    },
                ) {
                    Err(_) => rejected += 1,
                    Ok(contribution) => {
                        let result = self
                            .runtime
                            .journal
                            .lock()
                            .map_err(|_| anyhow::anyhow!("wallet journal unavailable"))?
                            .admit(&contribution.intake());
                        match result {
                            Ok(_) => admitted += 1,
                            Err(error) if error.is::<AdmissionConflict>() => rejected += 1,
                            Err(error) => return Err(error),
                        }
                    }
                }
                tokio::task::yield_now().await;
            }
            complete = processed_all && batch.complete && !batch.saturated;
            outcome = if !processed_all {
                Outcome::Incomplete
            } else if batch.saturated {
                Outcome::Saturated
            } else if complete {
                Outcome::AdmittedComplete
            } else {
                Outcome::Incomplete
            };
        }
        self.runtime
            .journal
            .lock()
            .map_err(|_| anyhow::anyhow!("wallet journal unavailable"))?
            .finish_relay_scan_with_counts(&claim, outcome, clock()?, admitted, rejected)?;
        Ok(Tick::Scanned {
            admitted,
            rejected,
            complete,
        })
    }
    pub async fn run(&mut self) -> Result<()> {
        loop {
            self.tick(clock()?).await?;
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    }
}
fn clock() -> Result<i64> {
    Ok(i64::try_from(
        SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs(),
    )?)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use cdk::{
        nuts::{Proof, SecretKey},
        secp256k1::{Keypair, Message, Secp256k1},
    };
    use lightning_goats_nostr_event::{SignedNostrEvent, VerifiedEvent, canonical_bytes};
    use std::{os::unix::fs::PermissionsExt, path::Path};
    use uuid::Uuid;

    pub(crate) fn config(path: &Path) -> Result<service_config::Config> {
        let uid = rustix::process::geteuid().as_raw();
        service_config::Config::decode(&serde_json::to_vec(&serde_json::json!({
            "version":1,"runtime_uid":uid,"core_uid":uid+1,"ipc_gid":12347,
            "wallet_instance":Uuid::new_v4(),"wallet_epoch":Uuid::new_v4(),
            "state_directory":path,"intake_socket":"/run/cashu-fixture/intake.sock",
            "receiving_key":SecretKey::from_slice(&[1;32])?.public_key().to_hex(),
            "recipient":SecretKey::from_slice(&[4;32])?.public_key().x_only_public_key().to_string(),
            "descriptors":[],"receive_nutzap":true,
            "mints":[{"id":"fixture","advertised_url":"https://mint.example","policy_version":"v1","receive_cashu":false,"receive_nutzap":true,"bolt11_exit_qualified":true}],
            "keysets":[{"mint_id":"fixture","id":"00deadbeef123456","keys":{"8":SecretKey::from_slice(&[7;32])?.public_key().to_hex()}}],
            "body_bytes":65536,"proof_count":8,"receipt_sats":100,
            "deadline_seconds":5,"connections_per_second":8,
            "relay_intake":{"enabled":true,"relays":["wss://relay.example"],"historical_floor":0,"window_seconds":60,"overlap_seconds":5,"event_limit":16}
        }))?)
    }
    async fn runtime(path: &Path) -> Result<Runtime> {
        for (name, bytes) in [
            ("wallet-seed.bin", vec![7; 64]),
            ("receiving-key.bin", vec![1; 32]),
        ] {
            let file = path.join(name);
            std::fs::write(&file, bytes)?;
            std::fs::set_permissions(file, std::fs::Permissions::from_mode(0o600))?;
        }
        Runtime::initialize(config(path)?).await
    }
    fn event(
        config: &service_config::Config,
        proofs: &[Proof],
        content: &str,
    ) -> Result<VerifiedEvent> {
        let secp = Secp256k1::new();
        let key =
            Keypair::from_secret_key(&secp, &cdk::secp256k1::SecretKey::from_slice(&[2; 32])?);
        let mut tags = vec![
            vec!["p".into(), config.recipient.clone()],
            vec!["u".into(), "https://mint.example".into()],
        ];
        for proof in proofs {
            tags.push(vec!["proof".into(), serde_json::to_string(proof)?]);
        }
        let mut event = SignedNostrEvent {
            id: String::new(),
            pubkey: key.x_only_public_key().0.to_string(),
            created_at: clock()? as u64,
            kind: 9321,
            tags,
            content: content.into(),
            sig: String::new(),
        };
        let hash: [u8; 32] = Sha256::digest(canonical_bytes(&event)?).into();
        event.id = cdk::util::hex::encode(hash);
        event.sig = secp
            .sign_schnorr_no_aux_rand(&Message::from_digest(hash), &key)
            .to_string();
        VerifiedEvent::from_json(&serde_json::to_string(&event)?)
    }

    #[test]
    #[ignore = "requires the isolated native wallet WSS fixture"]
    fn isolated_wss_sender() -> Result<()> {
        let host_namespace = std::env::var_os("LG_CASHU_WSS_HOST_NETNS")
            .ok_or_else(|| anyhow::anyhow!("fixture host namespace is required"))?;
        ensure!(
            std::fs::read_link("/proc/self/ns/net")?.as_os_str() != host_namespace
                && std::fs::read_to_string("/proc/net/dev")?
                    .lines()
                    .skip(2)
                    .all(|line| line.trim_start().starts_with("lo:")),
            "synthetic sender requires a private network namespace"
        );
        let config = config(Path::new("/run/lg-nutzap-fixture"))?;
        let intent = crate::publication::Intent::announcement(&config, clock()?.try_into()?)?;
        let announcement = intent.verify_signed(
            None,
            &crate::publication::tests::sign(intent.unsigned(None)?)?,
        )?;
        let lock = announcement
            .singleton_tag("pubkey", true)?
            .ok_or_else(|| anyhow::anyhow!("fixture announcement omitted a lock"))?;
        let proof = crate::intake::tests::proof(
            serde_json::json!(["P2PK",{"nonce":"0123456789abcdef0123456789abcdef","data":lock,"tags":[]}])
                .to_string(),
        );
        let first = event(&config, std::slice::from_ref(&proof), "first delivery")?;
        let replay = event(&config, &[proof], "same proof in a new event")?;
        println!(
            "CASHU_WSS_FIXTURE {}",
            serde_json::json!({
                "recipient": config.recipient,
                "receiving_key": config.receiving_key,
                "mint": &config.mints[0].advertised_url,
                "first": first.event(),
                "replay": replay.event(),
                "mint_key": SecretKey::from_slice(&[7; 32])?.public_key().to_hex(),
            })
        );
        Ok(())
    }

    #[tokio::test]
    async fn rejects_bad_events_and_overlap_without_blocking_later_proofs_or_replay() -> Result<()>
    {
        let dir = crate::test_directory()?;
        let runtime = runtime(dir.path()).await?;
        let first = crate::intake::tests::locked();
        let second =
            crate::intake::tests::proof(first.secret.to_string().replace("fixture", "second"));
        let events = vec![
            event(&runtime.config, &[], "invalid")?,
            event(&runtime.config, std::slice::from_ref(&first), "first")?,
            event(&runtime.config, &[first.clone(), second.clone()], "overlap")?,
            event(&runtime.config, std::slice::from_ref(&second), "second")?,
        ];
        let mut worker = Worker::new(&runtime)?;
        let result = worker
            .tick_with(clock()?, |_, _| async {
                Ok(Batch {
                    events,
                    complete: true,
                    saturated: false,
                })
            })
            .await?;
        assert_eq!(
            result,
            Tick::Scanned {
                admitted: 2,
                rejected: 2,
                complete: true
            }
        );
        let replay = event(&runtime.config, &[first], "different wrapper")?;
        let result = worker
            .tick_with(clock()?, |_, _| async {
                Ok(Batch {
                    events: vec![replay],
                    complete: true,
                    saturated: false,
                })
            })
            .await?;
        assert_eq!(
            result,
            Tick::Scanned {
                admitted: 1,
                rejected: 0,
                complete: true
            }
        );
        let db = rusqlite::Connection::open(dir.path().join("acquisition.sqlite"))?;
        assert_eq!(
            db.query_row("SELECT COUNT(*) FROM acquisitions", [], |r| r
                .get::<_, i64>(0))?,
            2
        );
        assert_eq!(
            db.query_row(
                "SELECT COUNT(*) FROM acquisitions WHERE state='validated'",
                [],
                |r| r.get::<_, i64>(0)
            )?,
            2
        );
        assert_eq!(
            db.query_row(
                "SELECT rejected FROM relay_scan_results WHERE lane='recent'",
                [],
                |r| r.get::<_, i64>(0)
            )?,
            2
        );
        let status = crate::status::Status::read(&runtime.config)?;
        assert!(status.relay_intake_enabled);
        assert_eq!(status.relay_scans.len(), 2);
        assert_eq!(
            status
                .relay_scans
                .iter()
                .find(|s| s.lane == "recent")
                .unwrap()
                .last_rejected,
            2
        );
        let mut reopened_config = config(dir.path())?;
        reopened_config.wallet_instance = runtime.config.wallet_instance;
        reopened_config.wallet_epoch = runtime.config.wallet_epoch;
        reopened_config
            .relay_intake
            .as_mut()
            .unwrap()
            .relays
            .push("wss://second.example/?fixture-secret=redact".into());
        let replay = event(&runtime.config, &[second], "after restart on another relay")?;
        drop(worker);
        drop(db);
        drop(runtime);
        let reopened = Runtime::open(reopened_config).await?;
        let mut worker = Worker::new(&reopened)?;
        worker.next = 2;
        assert_eq!(
            worker
                .tick_with(clock()?, |_, _| async {
                    Ok(Batch {
                        events: vec![replay],
                        complete: true,
                        saturated: false,
                    })
                })
                .await?,
            Tick::Scanned {
                admitted: 1,
                rejected: 0,
                complete: true
            }
        );
        let status = crate::status::Status::read(&reopened.config)?;
        assert_eq!(status.state_counts.get("validated"), Some(&2));
        assert!(!serde_json::to_string(&status)?.contains("fixture-secret"));
        Ok(())
    }
    #[tokio::test]
    async fn disabled_is_offline_and_network_failure_does_not_starve_next_lane() -> Result<()> {
        let dir = crate::test_directory()?;
        let mut runtime = runtime(dir.path()).await?;
        runtime.config.relay_intake.as_mut().unwrap().enabled = false;
        let mut worker = Worker::new(&runtime)?;
        assert_eq!(
            worker
                .tick_with(clock()?, |_, _| async {
                    panic!("disabled relay contacted")
                })
                .await?,
            Tick::Idle
        );
        drop(worker);
        runtime.config.relay_intake.as_mut().unwrap().enabled = true;
        let mut worker = Worker::new(&runtime)?;
        assert_eq!(
            worker
                .tick_with(clock()?, |_, _| async {
                    anyhow::bail!("fixture network failure")
                })
                .await?,
            Tick::Scanned {
                admitted: 0,
                rejected: 0,
                complete: false
            }
        );
        assert_eq!(
            worker
                .tick_with(clock()?, |_, _| async {
                    Ok(Batch {
                        events: vec![],
                        complete: true,
                        saturated: false,
                    })
                })
                .await?,
            Tick::Scanned {
                admitted: 0,
                rejected: 0,
                complete: true
            }
        );
        Ok(())
    }
    #[tokio::test]
    async fn storage_failure_keeps_checkpoint_unresolved() -> Result<()> {
        let dir = crate::test_directory()?;
        let runtime = runtime(dir.path()).await?;
        let db = rusqlite::Connection::open(dir.path().join("acquisition.sqlite"))?;
        db.execute_batch("CREATE TRIGGER fail_admission BEFORE INSERT ON acquisitions BEGIN SELECT RAISE(ABORT,'fixture storage failure'); END;")?;
        let events = vec![event(
            &runtime.config,
            &[crate::intake::tests::locked()],
            "valid",
        )?];
        assert!(
            Worker::new(&runtime)?
                .tick_with(clock()?, |_, _| async {
                    Ok(Batch {
                        events,
                        complete: true,
                        saturated: false,
                    })
                })
                .await
                .is_err()
        );
        assert_eq!(
            db.query_row("SELECT outcome FROM relay_scans", [], |r| r
                .get::<_, String>(0))?,
            "pending"
        );
        assert_eq!(
            db.query_row("SELECT COUNT(*) FROM acquisitions", [], |r| r
                .get::<_, i64>(0))?,
            0
        );
        Ok(())
    }
    #[test]
    fn configuration_and_scope_cover_admission_policy_changes() -> Result<()> {
        let mut config = config(Path::new("/tmp/cashu-fixture"))?;
        let original = scope(&config, config.relay_intake.as_ref().unwrap())?;
        config.receipt_sats += 1;
        assert_ne!(
            original,
            scope(&config, config.relay_intake.as_ref().unwrap())?
        );
        config.receipt_sats -= 1;
        config.keysets[0]
            .keys
            .insert(8, SecretKey::from_slice(&[8; 32])?.public_key().to_hex());
        assert_ne!(
            original,
            scope(&config, config.relay_intake.as_ref().unwrap())?
        );
        let relay = config.relay_intake.as_mut().unwrap();
        assert!(relay.validate(false).is_err());
        relay.relays.push("wss://relay.example/".into());
        assert!(relay.validate(true).is_err());
        relay.relays.pop();
        relay.event_limit = 257;
        assert!(relay.validate(true).is_err());
        Ok(())
    }
}
