//! Synthetic end-to-end private-core / ordered-membership regressions.
use crate::test_invoices as invoices;
mod nostr {
    pub(super) use crate::nostr_event::test_event as signed;
}
use crate::{
    core_bridge::{
        AuthenticatedBatch, Batch, CoreEvent, EventBody, Response,
        unix::{Client, self_credentials},
    },
    cyberherd::{
        calendar::{DEFAULT_TIMEZONE, DayWindow},
        discovery::Discovery,
        ingress::CoreBinding,
        membership::{FeederAvailability, Settings},
        relay::{Filter, Query, ReadBatch},
        social,
        store::{Status, Store},
        targets::{AuthenticatedProfile, TargetPolicy, Targets},
        worker::{self, Environment, Step},
    },
    domain::payment::SettledPayment,
    ledger::{LedgerStore, StoredStrikeReceiveRequest},
    nostr_event::VerifiedEvent,
    zap::ZapRequest,
};
use serde_json::json;
use sqlx::SqlitePool;
use std::{
    collections::{BTreeMap, BTreeSet},
    time::Duration,
};
use tempfile::TempDir;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::UnixListener,
};
use uuid::Uuid;

fn public(key: u8) -> String {
    nostr::signed(key, 1, 100, "", vec![]).pubkey
}
fn tag(values: &[&str]) -> Vec<String> {
    values.iter().map(|v| (*v).into()).collect()
}
fn signed(key: u8, kind: u64, now: i64, content: &str, tags: Vec<Vec<String>>) -> VerifiedEvent {
    VerifiedEvent::verify(nostr::signed(key, kind, now as u64, content, tags)).unwrap()
}

// A synthetic peer exercises actual framing and kernel UID checks; no public
// constructor can turn a deserialized Batch into authenticated input.
async fn authenticate(batch: Batch) -> AuthenticatedBatch {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("core.sock");
    let listener = UnixListener::bind(&path).unwrap();
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let len = stream.read_u32().await.unwrap();
        let mut raw = vec![0; len as usize];
        stream.read_exact(&mut raw).await.unwrap();
        let raw = serde_json::to_vec(&Response::Events {
            batch: Box::new(batch),
        })
        .unwrap();
        stream.write_u32(raw.len() as u32).await.unwrap();
        stream.write_all(&raw).await.unwrap();
    });
    // Request identity is separately checked by the client against the response.
    let client = Client::new(path, self_credentials().unwrap().0).unwrap();
    let result = client.events(None, 0, 128).await;
    server.await.unwrap();
    result.unwrap()
}

struct Fixture {
    _root: TempDir,
    core: LedgerStore,
    store: Store,
    database: String,
    now: i64,
    owner: String,
    note: VerifiedEvent,
    targets: Targets,
    binding: CoreBinding,
}
impl Fixture {
    async fn new() -> Self {
        Self::with_clock(crate::cyberherd::calendar::now().unwrap(), 100).await
    }
    async fn with_clock(now: i64, history_seconds: i64) -> Self {
        let root = TempDir::new().unwrap();
        let core = LedgerStore::connect(&format!(
            "sqlite://{}",
            root.path().join("core.db").display()
        ))
        .await
        .unwrap();
        let database = format!("sqlite://{}", root.path().join("herd.db").display());
        let store = Store::connect(&database).await.unwrap();
        let owner = public(1);
        store
            .initialize(
                &owner,
                &Settings {
                    minimum_sats: 10,
                    max_members: 10,
                    member_percent: 10,
                },
                now - history_seconds,
            )
            .await
            .unwrap();
        let note = signed(
            1,
            1,
            now - history_seconds / 2,
            "today",
            vec![tag(&["t", "CyberHerd"])],
        );
        let targets = Targets::project(
            TargetPolicy {
                project_pubkey: owner.clone(),
                tracked_tags: vec!["CyberHerd".into()],
            },
            &DayWindow::at(DEFAULT_TIMEZONE, now).unwrap(),
            vec![note.clone()],
            vec![],
            BTreeSet::new(),
            now,
        )
        .unwrap();
        let epoch = core.core_event_batch(None, 0, 128).await.unwrap().epoch;
        let binding = CoreBinding {
            core_uid: self_credentials().unwrap().0,
            epoch,
            start_after: 0,
            credit_pool: "herd".into(),
            payees: BTreeMap::from([("herd".into(), owner.clone())]),
        };
        store
            .initialize_core_ingress(&owner, &binding)
            .await
            .unwrap();
        Self {
            _root: root,
            core,
            store,
            database,
            now,
            owner,
            note,
            targets,
            binding,
        }
    }
    fn profile(&self, key: u8) -> AuthenticatedProfile {
        AuthenticatedProfile::parse(
            signed(
                key,
                0,
                self.now - 10,
                &json!({"name":format!("User {key}"),"lud16":format!("user{key}@example.com")})
                    .to_string(),
                vec![],
            ),
            &public(key),
            self.now,
        )
        .unwrap()
    }
    async fn paid(&self, id: u8, key: u8, sats: u64, target: &VerifiedEvent) -> String {
        // Request timestamp deliberately predates the cycle; settlement time,
        // not payer-controlled request time, governs membership.
        let raw = serde_json::to_string(&nostr::signed(
            key,
            9734,
            100,
            "",
            vec![
                tag(&["p", &self.owner]),
                tag(&["e", &target.event().id]),
                tag(&["relays", "wss://relay.example.com/"]),
            ],
        ))
        .unwrap();
        self.paid_raw(id, sats, &raw).await;
        raw
    }
    async fn paid_raw(&self, id: u8, sats: u64, raw: &str) {
        let zap = ZapRequest::validate(raw, &self.owner, &public(4), sats * 1000).unwrap();
        let request = Uuid::new_v4();
        let invoice = invoices::invoice_with_payment_hash(
            sats * 1000,
            &zap.description_hash(),
            Duration::from_secs(self.now as u64),
            [id; 32],
        );
        self.core
            .record_strike_receive_request_with_zap(
                &StoredStrikeReceiveRequest {
                    receive_request_id: request,
                    address_user: "herd".into(),
                    credit_pool: "herd".into(),
                    amount_msat: sats * 1000,
                    description_hash: zap.description_hash(),
                    payment_hash: hex::encode([id; 32]),
                    invoice,
                    created_provider: None,
                },
                Some(&zap),
            )
            .await
            .unwrap();
        self.core
            .record_payment(&SettledPayment {
                source: "strike".into(),
                source_id: format!("paid-{id}"),
                payment_hash: Some(hex::encode([id; 32])),
                address_user: "herd".into(),
                credit_pool: "herd".into(),
                amount_msat: sats * 1000,
                settled_at: Some(self.now),
                context_json: Some(json!({"receive_request_id":request}).to_string()),
            })
            .await
            .unwrap();
    }
    async fn page(&self) -> Batch {
        self.core.core_event_batch(None, 0, 128).await.unwrap()
    }
    async fn ingest(&self) -> usize {
        self.store
            .ingest_core(&self.owner, &authenticate(self.page().await).await)
            .await
            .unwrap()
    }
    async fn run(
        &self,
        profile: Option<&AuthenticatedProfile>,
        feeder: FeederAvailability,
    ) -> Step {
        worker::step(
            &self.store,
            &self.owner,
            Environment {
                targets: &self.targets,
                discovery_available: true,
                feeder,
                profile,
                now: self.now,
            },
        )
        .await
        .unwrap()
    }
    async fn bridge(&self) -> (Client, tokio::task::JoinHandle<anyhow::Result<()>>) {
        let (uid, gid) = self_credentials().unwrap();
        std::fs::set_permissions(
            self._root.path(),
            std::os::unix::fs::PermissionsExt::from_mode(0o750),
        )
        .unwrap();
        let config = crate::core_bridge::BridgeConfig {
            socket: self._root.path().join("delivery.sock"),
            peer_uid: uid,
            group_gid: gid,
            reward_policy: None,
            payout_peer_uid: None,
            delegated_feeder_messages: false,
            payout_execution_enabled: false,
            payout_account_id: None,
        };
        let server = crate::core_bridge::unix::Server::bind(&config)
            .await
            .unwrap();
        let task = tokio::spawn(server.run(self.core.clone(), crate::config::RuntimeMode::Active));
        (
            Client::new(config.socket, uid)
                .unwrap()
                .with_request_spacing(Duration::from_millis(1100)),
            task,
        )
    }
}

#[tokio::test]
async fn operator_fence_drains_bounded_presentation_tail_but_cannot_pass_a_confirmed_feeding() {
    use crate::cyberherd::{http_auth, operator};
    use base64::Engine as _;
    use sha2::{Digest as _, Sha256};
    fn authorize(cmd: &operator::Command, now: i64) -> http_auth::OperatorAuthorization {
        let body = serde_json::to_vec(cmd).unwrap();
        let proof = nostr::signed(
            3,
            27235,
            now as u64,
            "",
            vec![
                tag(&["u", "https://example.com/api/v1/cyberherd/operator/command"]),
                tag(&["method", "POST"]),
                tag(&["payload", &hex::encode(Sha256::digest(&body))]),
                tag(&["nonce", &Uuid::new_v4().to_string()]),
            ],
        );
        http_auth::Config {
            public_origin: "https://example.com".into(),
            operators: vec![public(3)],
        }
        .verify_operator(
            &format!(
                "Nostr {}",
                base64::engine::general_purpose::STANDARD
                    .encode(serde_json::to_vec(&proof).unwrap())
            ),
            "/api/v1/cyberherd/operator/command",
            &body,
            now,
        )
        .unwrap()
    }
    let f = Fixture::new().await;
    for n in 0..130 {
        f.core
            .append_event("nostr_publication_attempt", &json!({"attempt":n}))
            .await
            .unwrap();
    }
    let (client, task) = f.bridge().await;
    assert!(
        operator::CoreFence::observe(&f.store, &f.owner, &client)
            .await
            .is_err()
    );
    assert_eq!(
        f.store
            .core_cursor(&f.owner)
            .await
            .unwrap()
            .received_through,
        128
    );
    let fence = operator::CoreFence::observe(&f.store, &f.owner, &client)
        .await
        .unwrap();
    fence
        .validate(
            &f.store.core_cursor(&f.owner).await.unwrap(),
            crate::cyberherd::calendar::now().unwrap(),
        )
        .unwrap();
    assert_eq!(
        f.store
            .core_cursor(&f.owner)
            .await
            .unwrap()
            .received_through,
        130
    );
    assert!(f.store.ingress_head(&f.owner).await.unwrap().is_none());
    assert_eq!(f.store.snapshot(&f.owner).await.unwrap().revision, 0);
    let mut cmd = operator::Command {
        id: Uuid::new_v4(),
        expected_revision: 0,
        action: operator::Action::Settings {
            minimum_sats: "20".into(),
            max_members: 10,
            member_percent: 10,
        },
    };
    let now = crate::cyberherd::calendar::now().unwrap();
    f.store
        .operator_command(
            &f.owner,
            &authorize(&cmd, now),
            &cmd,
            operator::Environment {
                feeder: FeederAvailability::Unknown,
                context: crate::cyberherd::conversation::Context::default(),
                ordering: Some(fence),
                now,
            },
        )
        .await
        .unwrap();
    assert_eq!(f.store.snapshot(&f.owner).await.unwrap().revision, 1);
    f.paid(11, 2, 1000, &f.note).await;
    let feed = f.core.begin_feed_attempt(1000).await.unwrap().unwrap();
    f.core.confirm_feed_attempt(feed).await.unwrap();
    let fence = operator::CoreFence::observe(&f.store, &f.owner, &client)
        .await
        .unwrap();
    cmd.id = Uuid::new_v4();
    cmd.expected_revision = 1;
    cmd.action = operator::Action::Settings {
        minimum_sats: "21".into(),
        max_members: 10,
        member_percent: 10,
    };
    let now = crate::cyberherd::calendar::now().unwrap();
    let error = f
        .store
        .operator_command(
            &f.owner,
            &authorize(&cmd, now),
            &cmd,
            operator::Environment {
                feeder: FeederAvailability::Ready,
                context: crate::cyberherd::conversation::Context::default(),
                ordering: Some(fence),
                now,
            },
        )
        .await
        .unwrap_err();
    assert_eq!(
        error.to_string(),
        "operator edit awaits confirmed feeding snapshot"
    );
    assert_eq!(f.store.snapshot(&f.owner).await.unwrap().revision, 1);
    assert_eq!(
        f.store
            .snapshot(&f.owner)
            .await
            .unwrap()
            .settings
            .minimum_sats,
        20
    );
    assert!(matches!(
        f.store.ingress_head(&f.owner).await.unwrap().unwrap().input,
        crate::cyberherd::ingress::Input::Payment(_)
    ));
    task.abort();
}

#[tokio::test]
async fn completed_empty_profile_lookup_allows_paid_join_and_following_feed() {
    let f = Fixture::new().await;
    f.paid(10, 2, 1000, &f.note).await;
    let feed = f.core.begin_feed_attempt(1000).await.unwrap().unwrap();
    f.core.confirm_feed_attempt(feed).await.unwrap();
    assert_eq!(f.ingest().await, 2);
    assert_eq!(f.run(None, FeederAvailability::Ready).await, Step::Waiting);
    f.store
        .record_reward_profile_read(&f.owner, &public(2), true, f.now)
        .await
        .unwrap();
    assert!(matches!(
        f.run(None, FeederAvailability::Ready).await,
        Step::Interaction(_)
    ));
    let roster = f.store.snapshot(&f.owner).await.unwrap().roster;
    assert!(roster.members[&public(2)].active);
    assert!(roster.members[&public(2)].profile.lud16.is_none());
    assert!(matches!(
        f.run(None, FeederAvailability::Ready).await,
        Step::Feeding(_)
    ));
}

#[tokio::test]
async fn actual_core_payments_precede_feed_and_later_backdated_social_cannot_enter_snapshot() {
    let f = Fixture::new().await;
    let raw = f.paid(10, 2, 1000, &f.note).await;
    // Two independently settled invoices may reuse precisely one signed request.
    f.paid_raw(11, 1000, &raw).await;
    let feed = f.core.begin_feed_attempt(1000).await.unwrap().unwrap();
    f.core.confirm_feed_attempt(feed).await.unwrap();
    assert_eq!(f.ingest().await, 3);
    assert_eq!(f.run(None, FeederAvailability::Ready).await, Step::Waiting);
    assert!(
        f.store
            .feeding_barrier(&f.owner, feed)
            .await
            .unwrap()
            .is_none()
    );
    let social = signed(3, 7, f.now - 30, "+", vec![tag(&["e", &f.note.event().id])]);
    social::stage(&f.store, &f.owner, social, f.now)
        .await
        .unwrap();
    let profile = f.profile(2);
    for _ in 0..2 {
        assert!(matches!(
            f.run(Some(&profile), FeederAvailability::Ready).await,
            Step::Interaction(_)
        ));
    }
    let Step::Feeding(barrier) = f.run(None, FeederAvailability::Ready).await else {
        panic!("feed barrier missing")
    };
    assert_eq!(
        barrier.snapshot.roster.members[&public(2)].amount_sats,
        2000
    );
    assert!(!barrier.snapshot.roster.members.contains_key(&public(3)));
    let user3 = f.profile(3);
    f.run(Some(&user3), FeederAvailability::Ready).await;
    assert!(f.store.snapshot(&f.owner).await.unwrap().roster.members[&public(3)].active);
    assert_eq!(
        f.store
            .feeding_barrier(&f.owner, feed)
            .await
            .unwrap()
            .unwrap(),
        *barrier
    );
    assert_eq!(f.ingest().await, 0);
    assert_eq!(f.run(None, FeederAvailability::Ready).await, Step::Idle);
    // Separate membership work never modifies paid core credit.
    assert_eq!(f.core.feed_credit_sats().await.unwrap(), 1000);
}

#[tokio::test]
async fn checkpoint_failure_replays_frozen_domain_receipt_without_recounting_or_new_profile() {
    let f = Fixture::new().await;
    f.paid(10, 2, 1000, &f.note).await;
    f.ingest().await;
    let pool = SqlitePool::connect(&f.database).await.unwrap();
    sqlx::query("CREATE TRIGGER fail_finish BEFORE UPDATE OF state ON ingress WHEN NEW.state='done' BEGIN SELECT RAISE(ABORT,'synthetic checkpoint failure'); END").execute(&pool).await.unwrap();
    let profile = f.profile(2);
    assert!(
        worker::step(
            &f.store,
            &f.owner,
            Environment {
                targets: &f.targets,
                discovery_available: true,
                feeder: FeederAvailability::Ready,
                profile: Some(&profile),
                now: f.now
            }
        )
        .await
        .is_err()
    );
    let before = f.store.snapshot(&f.owner).await.unwrap();
    assert_eq!(before.roster.members[&public(2)].amount_sats, 1000);
    sqlx::query("DROP TRIGGER fail_finish")
        .execute(&pool)
        .await
        .unwrap();
    let reopened = Store::connect(&f.database).await.unwrap();
    assert!(matches!(
        worker::step(
            &reopened,
            &f.owner,
            Environment {
                targets: &f.targets,
                discovery_available: false,
                feeder: FeederAvailability::Unknown,
                profile: None,
                now: f.now
            }
        )
        .await
        .unwrap(),
        Step::Interaction(_)
    ));
    assert_eq!(reopened.snapshot(&f.owner).await.unwrap(), before);
    assert!(reopened.ingress_head(&f.owner).await.unwrap().is_none());
}

#[tokio::test]
async fn ingest_is_atomic_and_rejects_changed_peer_epoch_history_and_financial_proof() {
    let f = Fixture::new().await;
    f.paid(10, 2, 1000, &f.note).await;
    let pool = SqlitePool::connect(&f.database).await.unwrap();
    sqlx::query("CREATE TRIGGER fail_checkpoint BEFORE UPDATE ON core_streams BEGIN SELECT RAISE(ABORT,'synthetic ingest failure'); END").execute(&pool).await.unwrap();
    let page = f.page().await;
    assert!(
        f.store
            .ingest_core(&f.owner, &authenticate(page.clone()).await)
            .await
            .is_err()
    );
    assert_eq!(
        f.store
            .core_cursor(&f.owner)
            .await
            .unwrap()
            .received_through,
        0
    );
    assert!(f.store.ingress_head(&f.owner).await.unwrap().is_none());
    sqlx::query("DROP TRIGGER fail_checkpoint")
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(f.ingest().await, 1);
    for mutation in 0..3 {
        let mut bad = page.clone();
        match mutation {
            0 => bad.epoch = Uuid::new_v4(),
            1 => bad.events[0].observed_at += 1,
            _ => {
                let EventBody::Payment(p) = &mut bad.events[0].body else {
                    panic!()
                };
                p.address_user = "nova".into();
            }
        }
        assert!(
            f.store
                .ingest_core(&f.owner, &authenticate(bad).await)
                .await
                .is_err()
        );
    }
    let mut binding = f.binding.clone();
    binding.core_uid += 1;
    assert!(
        f.store
            .initialize_core_ingress(&f.owner, &binding)
            .await
            .is_err()
    );
    // A later envelope with a changed local row ID still has the same domain
    // source, but a genuinely different amount is a financial conflict.
    let mut repeated = page.clone();
    repeated.events.push(repeated.events[0].clone());
    repeated.events[1].seq = 2;
    repeated.through = 2;
    repeated.latest = 2;
    let EventBody::Payment(p) = &mut repeated.events[1].body else {
        panic!()
    };
    p.settlement_id += 100;
    assert_eq!(
        f.store
            .ingest_core(&f.owner, &authenticate(repeated.clone()).await)
            .await
            .unwrap(),
        0
    );
    repeated.events.push(repeated.events[1].clone());
    repeated.events[2].seq = 3;
    repeated.through = 3;
    repeated.latest = 3;
    let EventBody::Payment(p) = &mut repeated.events[2].body else {
        panic!()
    };
    p.zap = None;
    p.amount_msat += 1000;
    assert!(
        f.store
            .ingest_core(&f.owner, &authenticate(repeated).await)
            .await
            .is_err()
    );
    assert_eq!(
        f.store
            .core_cursor(&f.owner)
            .await
            .unwrap()
            .received_through,
        2
    );
}

#[tokio::test]
async fn paused_missing_profile_cannot_resume_as_broad_existing_member_project_zap() {
    let f = Fixture::new().await;
    f.paid(10, 2, 1000, &f.note).await;
    f.ingest().await;
    let profile = f.profile(2);
    f.run(Some(&profile), FeederAvailability::Ready).await;
    let older = signed(1, 1, f.now - 200, "other project content", vec![]);
    f.paid(11, 2, 34, &older).await;
    f.ingest().await;
    let targets = Targets::project(
        TargetPolicy {
            project_pubkey: f.owner.clone(),
            tracked_tags: vec!["CyberHerd".into()],
        },
        &DayWindow::at(DEFAULT_TIMEZONE, f.now).unwrap(),
        vec![f.note.clone(), older],
        vec![],
        BTreeSet::new(),
        f.now,
    )
    .unwrap();
    for (feeder, profile, expected) in [
        (FeederAvailability::Paused, None, Status::Deferred),
        (FeederAvailability::Ready, Some(&profile), Status::Discarded),
    ] {
        let step = worker::step(
            &f.store,
            &f.owner,
            Environment {
                targets: &targets,
                discovery_available: true,
                feeder,
                profile,
                now: f.now,
            },
        )
        .await
        .unwrap();
        if expected == Status::Deferred {
            assert_eq!(step, Step::Waiting);
        } else {
            let Step::Interaction(r) = step else { panic!() };
            assert_eq!(r.status, expected);
        }
    }
    assert_eq!(
        f.store.snapshot(&f.owner).await.unwrap().roster.members[&public(2)].amount_sats,
        1000
    );
}

#[tokio::test]
async fn feed_snapshot_and_queue_completion_are_atomic() {
    let f = Fixture::new().await;
    f.paid(10, 2, 1000, &f.note).await;
    let feed = f.core.begin_feed_attempt(1000).await.unwrap().unwrap();
    f.core.confirm_feed_attempt(feed).await.unwrap();
    f.ingest().await;
    f.run(Some(&f.profile(2)), FeederAvailability::Ready).await;
    let pool = SqlitePool::connect(&f.database).await.unwrap();
    sqlx::query("CREATE TRIGGER fail_barrier BEFORE INSERT ON feeding_barriers BEGIN SELECT RAISE(ABORT,'synthetic barrier failure'); END").execute(&pool).await.unwrap();
    assert!(
        worker::step(
            &f.store,
            &f.owner,
            Environment {
                targets: &f.targets,
                discovery_available: true,
                feeder: FeederAvailability::Ready,
                profile: None,
                now: f.now
            }
        )
        .await
        .is_err()
    );
    assert!(
        f.store
            .feeding_barrier(&f.owner, feed)
            .await
            .unwrap()
            .is_none()
    );
    assert!(f.store.ingress_head(&f.owner).await.unwrap().is_some());
    sqlx::query("DROP TRIGGER fail_barrier")
        .execute(&pool)
        .await
        .unwrap();
    assert!(matches!(
        f.run(None, FeederAvailability::Ready).await,
        Step::Feeding(_)
    ));
}

#[tokio::test]
async fn virtual_credit_and_unlinked_events_never_become_membership_or_reward_work() {
    let f = Fixture::new().await;
    let page = Batch {
        version: crate::core_bridge::VERSION,
        epoch: f.binding.epoch,
        after: 0,
        through: 2,
        latest: 2,
        events: vec![
            CoreEvent {
                seq: 1,
                observed_at: f.now,
                body: EventBody::OperatorCredit {
                    amount_sats: 1000,
                    address_user: "herd".into(),
                    credit_pool: "herd".into(),
                },
            },
            CoreEvent {
                seq: 2,
                observed_at: f.now,
                body: EventBody::Other,
            },
        ],
    };
    assert_eq!(
        f.store
            .ingest_core(&f.owner, &authenticate(page).await)
            .await
            .unwrap(),
        0
    );
    assert_eq!(f.run(None, FeederAvailability::Ready).await, Step::Idle);
    assert_eq!(
        f.store
            .core_cursor(&f.owner)
            .await
            .unwrap()
            .received_through,
        2
    );
}

struct RelayFixture {
    events: Vec<VerifiedEvent>,
    complete: usize,
    saturated: bool,
    queries: std::sync::Mutex<Vec<Vec<Filter>>>,
}

struct ProfileOutboxes {
    base: RelayFixture,
    profiles: Vec<VerifiedEvent>,
    complete: bool,
    saturated: bool,
    hints: std::sync::Mutex<Vec<Vec<String>>>,
}
impl Query for ProfileOutboxes {
    async fn query(&self, filters: &[Filter]) -> anyhow::Result<ReadBatch> {
        self.base.query(filters).await
    }
    async fn query_hints(
        &self,
        relays: &[String],
        filters: &[Filter],
    ) -> anyhow::Result<ReadBatch> {
        assert!(!relays.is_empty() && relays.len() <= 8);
        assert!(relays.iter().all(|r| matches!(
            r.as_str(),
            "wss://outbox.example.com/" | "wss://second.example.com/"
        )));
        self.hints.lock().unwrap().push(relays.to_vec());
        Ok(ReadBatch {
            events: self
                .profiles
                .iter()
                .filter(|e| filters.iter().any(|f| f.matches(e)))
                .cloned()
                .collect(),
            completed_relays: usize::from(self.complete),
            failed_relays: usize::from(!self.complete),
            saturated: self.saturated,
        })
    }
}

fn profile_outboxes(
    f: &Fixture,
    quorum: usize,
    complete: bool,
    saturated: bool,
) -> ProfileOutboxes {
    ProfileOutboxes {
        base: RelayFixture {
            events: vec![
                f.profile(2).event().clone(),
                signed(
                    3,
                    10002,
                    f.now - 1,
                    "",
                    vec![
                        tag(&["r", "wss://outbox.example.com", "write"]),
                        tag(&["r", "wss://read-only.example.com", "read"]),
                        tag(&["r", "wss://127.0.0.1/private", "write"]),
                        tag(&["r", "wss://sendit.nosflare.com", "write"]),
                    ],
                ),
            ],
            complete: quorum,
            saturated: false,
            queries: Default::default(),
        },
        profiles: vec![f.profile(3).event().clone()],
        complete,
        saturated,
        hints: Default::default(),
    }
}

#[derive(Clone, Default)]
struct MemberPublication(std::sync::Arc<std::sync::Mutex<Vec<String>>>);
impl crate::cyberherd::delivery_worker::Transport for MemberPublication {
    async fn sign(
        &self,
        unsigned: &crate::cyberherd::store::delivery_state::Unsigned,
    ) -> anyhow::Result<VerifiedEvent> {
        VerifiedEvent::verify(nostr::signed(
            1,
            unsigned.kind,
            unsigned.created_at,
            &unsigned.content,
            unsigned.tags.clone(),
        ))
    }
    async fn publish(
        &self,
        event: &VerifiedEvent,
        relay: &str,
    ) -> anyhow::Result<crate::cyberherd::delivery_worker::RelayAck> {
        self.0.lock().unwrap().push(event.event().id.clone());
        Ok(crate::cyberherd::delivery_worker::RelayAck::synthetic(
            event, relay,
        ))
    }
}

#[tokio::test]
async fn signed_outbox_profile_resolves_second_member_like_and_repost_exactly_once() {
    let f = Fixture::new().await;
    let discovery = Discovery::new(profile_outboxes(&f, 1, true, false), 1).unwrap();
    let tags = vec![tag(&["e", &f.note.event().id]), tag(&["p", &f.owner])];
    let events = [
        signed(2, 6, f.now - 3, "", tags.clone()),
        signed(3, 7, f.now - 2, "+", tags.clone()),
        signed(3, 6, f.now - 1, "", tags),
    ];
    for event in &events {
        assert!(
            social::stage(&f.store, &f.owner, event.clone(), f.now)
                .await
                .unwrap()
        );
    }
    for key in [2, 3, 3] {
        let profile = discovery
            .refresh_profile(&f.store, &f.owner, &public(key), f.now)
            .await
            .unwrap()
            .unwrap();
        assert!(matches!(
            f.run(Some(&profile), FeederAvailability::Ready).await,
            Step::Interaction(_)
        ));
    }
    let snapshot = f.store.snapshot(&f.owner).await.unwrap();
    assert_eq!(snapshot.roster.active_count(), 2);
    assert_eq!(snapshot.roster.awards[&public(2)].points(), 3);
    assert_eq!(snapshot.roster.awards[&public(3)].points(), 4);
    assert_eq!(discovery.source().hints.lock().unwrap().len(), 2);
    let reopened = Store::connect(&f.database).await.unwrap();
    for event in events {
        assert!(
            !social::stage(&reopened, &f.owner, event, f.now)
                .await
                .unwrap()
        );
    }
    assert_eq!(reopened.snapshot(&f.owner).await.unwrap(), snapshot);
    reopened
        .seed_templates(
            &f.owner,
            &crate::cyberherd::templates::Catalog::legacy().unwrap(),
        )
        .await
        .unwrap();
    reopened
        .seed_templates(
            &f.owner,
            &crate::cyberherd::templates::Catalog::delivery_defaults().unwrap(),
        )
        .await
        .unwrap();
    reopened.initialize_delivery(&f.owner, 0).await.unwrap();
    while reopened
        .queue_next_delivery(&f.owner, &["wss://relay.example.com/".into()], f.now)
        .await
        .unwrap()
    {}
    let db = SqlitePool::connect(&f.database).await.unwrap();
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM delivery_messages")
        .fetch_one(&db)
        .await
        .unwrap();
    assert_eq!(
        count, 3,
        "both joins and the follow-up repost retain public intents"
    );
    // Complete membership -> outbox -> private safety -> signing -> matching
    // ACK on temporary stores, with no public relay connection.
    let (core, task) = f.bridge().await;
    let transport = MemberPublication::default();
    let delivery = crate::cyberherd::delivery_worker::Worker {
        store: reopened.clone(),
        core,
        transport: transport.clone(),
        owner: f.owner.clone(),
    };
    for _ in 0..3 {
        f.core
            .update_feeding_presentation(
                1000,
                crate::config::RuntimeMode::Active,
                Some(crate::gateway::FeederSafety {
                    override_enabled: false,
                    remote_enabled: true,
                }),
            )
            .await
            .unwrap();
        assert_eq!(
            delivery.tick().await.unwrap(),
            crate::cyberherd::delivery_worker::Step::Published
        );
    }
    assert_eq!(
        delivery.tick().await.unwrap(),
        crate::cyberherd::delivery_worker::Step::Idle
    );
    assert_eq!(
        transport
            .0
            .lock()
            .unwrap()
            .iter()
            .collect::<BTreeSet<_>>()
            .len(),
        3
    );
    let receipts: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM delivery_receipts")
        .fetch_one(&db)
        .await
        .unwrap();
    assert_eq!(receipts, 3);
    task.abort();
}

#[tokio::test]
async fn one_completed_outbox_cannot_confirm_two_disjoint_profile_reads() {
    let f = Fixture::new().await;
    let mut source = profile_outboxes(&f, 1, true, false);
    source.base.events.retain(|e| e.event().kind != 0);
    source.base.events.push(signed(
        2,
        10002,
        f.now - 1,
        "",
        vec![tag(&["r", "wss://second.example.com", "write"])],
    ));
    let discovery = Discovery::new(source, 1).unwrap();
    discovery
        .refresh_reward_profiles(&f.store, &f.owner, &[public(2), public(3)], f.now)
        .await
        .unwrap();
    for key in [2, 3] {
        assert!(
            f.store
                .reward_profile_read(&f.owner, &public(key), f.now, 3600)
                .await
                .unwrap()
                .is_none()
        );
    }
}

#[tokio::test]
async fn outbox_profiles_preserve_quorum_backoff_and_signed_address_removal() {
    for (quorum, complete, saturated) in [(0, true, false), (1, false, false), (1, true, true)] {
        let f = Fixture::new().await;
        let discovery =
            Discovery::new(profile_outboxes(&f, quorum, complete, saturated), 1).unwrap();
        assert!(
            discovery
                .refresh_profile(&f.store, &f.owner, &public(3), f.now)
                .await
                .unwrap()
                .is_none()
        );
        assert!(
            f.store
                .reward_profile_read(&f.owner, &public(3), f.now, 3600)
                .await
                .unwrap()
                .is_none()
        );
        assert!(
            f.store
                .profile(&f.owner, &public(3), f.now)
                .await
                .unwrap()
                .is_some(),
            "signed evidence survives an incomplete read"
        );
    }
    let f = Fixture::new().await;
    f.store
        .remember_profile(&f.owner, &f.profile(3))
        .await
        .unwrap();
    let mut source = profile_outboxes(&f, 1, true, false);
    source.profiles.push(signed(3, 0, f.now - 1, "{}", vec![]));
    let discovery = Discovery::new(source, 1).unwrap();
    discovery
        .refresh_reward_profiles(&f.store, &f.owner, &[public(2), public(3)], f.now)
        .await
        .unwrap();
    let profile = f
        .store
        .profile(&f.owner, &public(3), f.now)
        .await
        .unwrap()
        .unwrap();
    assert!(
        profile.profile().lud16.is_none(),
        "new signed removal wins over a cached address"
    );
    assert!(
        f.store
            .reward_profile_read(&f.owner, &public(2), f.now, 3600)
            .await
            .unwrap()
            .is_some()
    );
    assert!(
        f.store
            .reward_profile_read(&f.owner, &public(3), f.now, 3600)
            .await
            .unwrap()
            .is_some()
    );
}
impl Query for RelayFixture {
    async fn query(&self, filters: &[Filter]) -> anyhow::Result<ReadBatch> {
        for filter in filters {
            filter.validate()?;
        }
        self.queries.lock().unwrap().push(filters.to_vec());
        Ok(ReadBatch {
            events: self
                .events
                .iter()
                .filter(|event| filters.iter().any(|f| f.matches(event)))
                .cloned()
                .collect(),
            completed_relays: self.complete,
            failed_relays: usize::from(self.complete == 0),
            saturated: self.saturated,
        })
    }
}

#[tokio::test]
async fn refresh_uses_host_filters_and_signed_negative_profile_while_missing_quorum_defers() {
    let f = Fixture::new().await;
    let stream = signed(
        1,
        30311,
        f.now - 20,
        "",
        vec![tag(&["d", "live"]), tag(&["status", "live"])],
    );
    let profile = f.profile(2).event().clone();
    let negative = signed(2, 0, f.now - 5, "{}", vec![]);
    let source = RelayFixture {
        events: vec![f.note.clone(), stream, profile, negative],
        complete: 1,
        saturated: false,
        queries: Default::default(),
    };
    let mut discovery = Discovery::new(source, 1).unwrap();
    let policy = TargetPolicy {
        project_pubkey: f.owner.clone(),
        tracked_tags: vec!["CyberHerd".into()],
    };
    let day = DayWindow::at(DEFAULT_TIMEZONE, f.now).unwrap();
    let refreshed = discovery
        .refresh(&f.store, &policy, &day, &[], f.now)
        .await
        .unwrap();
    assert!(refreshed.available);
    assert_eq!(refreshed.targets.current().len(), 2);
    let profile = discovery
        .refresh_profile(&f.store, &f.owner, &public(2), f.now)
        .await
        .unwrap()
        .unwrap();
    assert!(profile.profile().lud16.is_none());
    assert!(
        discovery.source().queries.lock().unwrap()[0]
            .iter()
            .any(|filter| filter.people == vec![f.owner.clone()])
    );
    // Cached current targets remain useful evidence but cannot cause apply or
    // stale discard when today's refresh has no configured completion quorum.
    discovery.source().queries.lock().unwrap().clear();
    discovery = Discovery::new(
        RelayFixture {
            events: vec![],
            complete: 0,
            saturated: false,
            queries: Default::default(),
        },
        1,
    )
    .unwrap();
    let unavailable = discovery
        .refresh(&f.store, &policy, &day, &[], f.now)
        .await
        .unwrap();
    assert!(!unavailable.available);
    assert_eq!(unavailable.targets.current().len(), 2);
}

#[tokio::test]
async fn full_intake_retains_core_and_social_cursors_until_existing_work_drains() {
    let f = Fixture::new().await;
    f.paid(10, 2, 1000, &f.note).await;
    let pool = SqlitePool::connect(&f.database).await.unwrap();
    sqlx::query("WITH RECURSIVE numbers(n) AS (SELECT 1 UNION ALL SELECT n+1 FROM numbers WHERE n<8192) INSERT INTO ingress(owner,source_key,kind,payload_json) SELECT ?, 'fixture:'||n,'social','\"fixture\"' FROM numbers")
        .bind(&f.owner).execute(&pool).await.unwrap();
    let error = f
        .store
        .ingest_core(&f.owner, &authenticate(f.page().await).await)
        .await
        .unwrap_err();
    assert!(error.is::<crate::cyberherd::ingress::IngressFull>());
    assert_eq!(
        f.store
            .core_cursor(&f.owner)
            .await
            .unwrap()
            .received_through,
        0
    );
    let event = signed(3, 7, f.now - 30, "+", vec![tag(&["e", &f.note.event().id])]);
    assert!(
        social::stage(&f.store, &f.owner, event.clone(), f.now)
            .await
            .unwrap_err()
            .is::<crate::cyberherd::ingress::IngressFull>()
    );
    assert!(
        f.store
            .observed_event(&f.owner, &event.event().id)
            .await
            .unwrap()
            .is_none()
    );
    sqlx::query("DELETE FROM ingress WHERE source_key LIKE 'fixture:%'")
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(f.ingest().await, 1);
}

#[tokio::test]
async fn social_backfill_advances_only_complete_unsaturated_windows_and_reuses_raw_receipts() {
    let f = Fixture::new().await;
    let event = signed(2, 7, f.now - 30, "+", vec![tag(&["e", &f.note.event().id])]);
    let source = RelayFixture {
        events: vec![event.clone()],
        complete: 1,
        saturated: true,
        queries: Default::default(),
    };
    let discovery = Discovery::new(source, 1).unwrap();
    assert!(
        !discovery
            .backfill(&f.store, &f.owner, &f.targets, f.now - 100, f.now)
            .await
            .unwrap()
    );
    let key =
        crate::cyberherd::ingress::hash(&("engagement-v2", f.now - 100, false, &f.note.event().id))
            .unwrap();
    assert_eq!(
        f.store
            .relay_window(&f.owner, &key, f.now - 100)
            .await
            .unwrap(),
        f.now - 100
    );
    let discovery = Discovery::new(
        RelayFixture {
            events: vec![event],
            complete: 1,
            saturated: false,
            queries: Default::default(),
        },
        1,
    )
    .unwrap();
    assert!(
        discovery
            .backfill(&f.store, &f.owner, &f.targets, f.now - 100, f.now)
            .await
            .unwrap()
    );
    assert_eq!(
        f.store
            .relay_window(&f.owner, &key, f.now - 100)
            .await
            .unwrap(),
        f.now
    );
    let profile = f.profile(2);
    f.run(Some(&profile), FeederAvailability::Ready).await;
    assert!(f.store.ingress_head(&f.owner).await.unwrap().is_none());
    assert_eq!(f.store.snapshot(&f.owner).await.unwrap().revision, 1);
}

#[tokio::test]
async fn cold_social_start_recovers_earlier_users_and_recent_reactions_without_skipping_history() {
    let today =
        DayWindow::at(DEFAULT_TIMEZONE, crate::cyberherd::calendar::now().unwrap()).unwrap();
    let f = Fixture::with_clock(today.start + 43200, 36000).await;
    let old = signed(
        2,
        7,
        f.now - 3600,
        "+",
        vec![tag(&["e", &f.note.event().id])],
    );
    let recent = signed(3, 7, f.now - 1, "+", vec![tag(&["e", &f.note.event().id])]);
    let discovery = Discovery::new(
        RelayFixture {
            events: vec![old.clone(), recent.clone()],
            complete: 1,
            saturated: false,
            queries: Default::default(),
        },
        1,
    )
    .unwrap();
    let start = f.now - 36000;
    assert!(
        discovery
            .backfill(&f.store, &f.owner, &f.targets, start, f.now)
            .await
            .unwrap()
    );
    for event in [&old, &recent] {
        assert!(
            f.store
                .observed_event(&f.owner, &event.event().id)
                .await
                .unwrap()
                .is_some()
        );
    }
    {
        let queries = discovery.source().queries.lock().unwrap();
        assert_eq!(queries.len(), 2);
        assert_eq!(queries[0][0].since, Some((f.now - 300) as u64));
        assert_eq!(queries[1][0].since, Some(start as u64));
    }
    let key = crate::cyberherd::ingress::hash(&("engagement-v2", start, false, &f.note.event().id))
        .unwrap();
    assert_eq!(
        f.store.relay_window(&f.owner, &key, start).await.unwrap(),
        f.now
    );
    // Retrying/restarting keeps exact source identities; it cannot re-award.
    discovery
        .backfill(&f.store, &f.owner, &f.targets, start, f.now + 1)
        .await
        .unwrap();
    let pool = SqlitePool::connect(&f.database).await.unwrap();
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM ingress WHERE owner=?")
        .bind(&f.owner)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 2);
    f.run(Some(&f.profile(3)), FeederAvailability::Ready).await;
    f.run(Some(&f.profile(2)), FeederAvailability::Ready).await;
    let snapshot = f.store.snapshot(&f.owner).await.unwrap();
    assert_eq!(snapshot.roster.active_count(), 2);
    for key in [2, 3] {
        assert!(snapshot.roster.members[&public(key)].active);
        assert!(snapshot.roster.awards[&public(key)].like);
    }
}

#[tokio::test]
async fn stream_updates_keep_prior_scope_cursors_and_recover_new_aliases_without_quorum_skips() {
    let f = Fixture::new().await;
    let policy = TargetPolicy {
        project_pubkey: f.owner.clone(),
        tracked_tags: vec!["CyberHerd".into()],
    };
    let start = f.now - 36000;
    let note_key =
        crate::cyberherd::ingress::hash(&("engagement-v2", start, false, &f.note.event().id))
            .unwrap();
    for offset in [20, 10] {
        let stream = signed(
            1,
            30311,
            f.now - offset,
            "",
            vec![tag(&["d", "live"]), tag(&["status", "live"])],
        );
        let stream_key =
            crate::cyberherd::ingress::hash(&("engagement-v2", start, false, &stream.event().id))
                .unwrap();
        let mut discovery = Discovery::new(
            RelayFixture {
                events: vec![f.note.clone(), stream],
                complete: 1,
                saturated: false,
                queries: Default::default(),
            },
            1,
        )
        .unwrap();
        let refreshed = discovery
            .refresh(
                &f.store,
                &policy,
                &DayWindow::at(DEFAULT_TIMEZONE, f.now).unwrap(),
                &[],
                f.now,
            )
            .await
            .unwrap();
        assert!(refreshed.available);
        discovery
            .backfill(&f.store, &f.owner, &refreshed.targets, start, f.now)
            .await
            .unwrap();
        assert_eq!(
            f.store
                .relay_window(&f.owner, &note_key, start)
                .await
                .unwrap(),
            f.now
        );
        assert_eq!(
            f.store
                .relay_window(&f.owner, &stream_key, start)
                .await
                .unwrap(),
            f.now
        );
    }
    let incomplete = Discovery::new(
        RelayFixture {
            events: vec![],
            complete: 0,
            saturated: false,
            queries: Default::default(),
        },
        1,
    )
    .unwrap();
    assert!(
        !incomplete
            .backfill(&f.store, &f.owner, &f.targets, start, f.now + 100)
            .await
            .unwrap()
    );
    assert_eq!(
        f.store
            .relay_window(&f.owner, &note_key, start)
            .await
            .unwrap(),
        f.now
    );
}

#[tokio::test]
async fn observed_pause_restricts_all_queued_payments_not_only_the_current_head() {
    let f = Fixture::new().await;
    f.paid(10, 2, 1000, &f.note).await;
    f.ingest().await;
    f.run(Some(&f.profile(2)), FeederAvailability::Ready).await;
    let other = signed(1, 1, f.now - 200, "another project note", vec![]);
    for id in [11, 12] {
        f.paid(id, 2, 34, &other).await;
    }
    f.ingest().await;
    f.store.defer_pending_payments(&f.owner).await.unwrap();
    let targets = Targets::project(
        TargetPolicy {
            project_pubkey: f.owner.clone(),
            tracked_tags: vec!["CyberHerd".into()],
        },
        &DayWindow::at(DEFAULT_TIMEZONE, f.now).unwrap(),
        vec![f.note.clone(), other],
        vec![],
        BTreeSet::new(),
        f.now,
    )
    .unwrap();
    for _ in 0..2 {
        let Step::Interaction(receipt) = worker::step(
            &f.store,
            &f.owner,
            Environment {
                targets: &targets,
                discovery_available: true,
                feeder: FeederAvailability::Ready,
                profile: None,
                now: f.now,
            },
        )
        .await
        .unwrap() else {
            panic!("paused broad target not discarded");
        };
        assert_eq!(receipt.status, Status::Discarded);
    }
    assert_eq!(
        f.store.snapshot(&f.owner).await.unwrap().roster.members[&public(2)].amount_sats,
        1000
    );
}

#[tokio::test]
async fn closed_cycle_work_discards_without_profile_or_discovery_and_never_blocks_a_feed() {
    let f = Fixture::new().await;
    f.paid(10, 2, 1000, &f.note).await;
    let feed = f.core.begin_feed_attempt(1000).await.unwrap().unwrap();
    f.core.confirm_feed_attempt(feed).await.unwrap();
    // Stage social before the core page so both expired inputs precede feeding.
    social::stage(
        &f.store,
        &f.owner,
        signed(3, 7, f.now - 20, "+", vec![tag(&["e", &f.note.event().id])]),
        f.now,
    )
    .await
    .unwrap();
    f.ingest().await;
    f.store
        .reset(&f.owner, "synthetic-reset", f.now + 1)
        .await
        .unwrap();
    for _ in 0..2 {
        let Step::Interaction(receipt) = worker::step(
            &f.store,
            &f.owner,
            Environment {
                targets: &f.targets,
                discovery_available: false,
                feeder: FeederAvailability::Unknown,
                profile: None,
                now: f.now + 2,
            },
        )
        .await
        .unwrap() else {
            panic!("old cycle is still blocking")
        };
        assert_eq!(receipt.status, Status::Discarded);
    }
    let Step::Feeding(barrier) = f.run(None, FeederAvailability::Unknown).await else {
        panic!()
    };
    assert!(barrier.snapshot.roster.members.is_empty());
    assert_eq!(barrier.feeding.request_id, feed);
    assert_eq!(f.core.feed_credit_sats().await.unwrap(), 0);
}

#[tokio::test]
async fn profileless_like_cannot_block_a_full_herd_repost_but_cannot_claim_an_open_seat() {
    use crate::cyberherd::membership::Decision;
    let mut f = Fixture::new().await;
    for key in 2..12 {
        let event = signed(
            key,
            7,
            f.now - 20,
            "+",
            vec![tag(&["e", &f.note.event().id])],
        );
        social::stage(&f.store, &f.owner, event, f.now)
            .await
            .unwrap();
        assert!(matches!(
            f.run(Some(&f.profile(key)), FeederAvailability::Ready)
                .await,
            Step::Interaction(_)
        ));
    }
    let event = signed(12, 7, f.now - 2, "+", vec![tag(&["e", &f.note.event().id])]);
    social::stage(&f.store, &f.owner, event, f.now)
        .await
        .unwrap();
    assert_eq!(f.run(None, FeederAvailability::Paused).await, Step::Ignored);
    f.now += 6;
    assert!(
        f.store
            .queue_optional_social_retry(&f.owner, f.now)
            .await
            .unwrap()
    );
    let Step::Interaction(receipt) = f.run(None, FeederAvailability::Ready).await else {
        panic!("capacity-only rejection should drain the head")
    };
    assert!(matches!(
        receipt.transition.unwrap().decision,
        Decision::Full { .. }
    ));
    let repost = signed(
        13,
        6,
        f.now - 1,
        "",
        vec![tag(&["e", &f.note.event().id]), tag(&["p", &f.owner])],
    );
    social::stage(&f.store, &f.owner, repost, f.now)
        .await
        .unwrap();
    let Step::Interaction(receipt) = f.run(Some(&f.profile(13)), FeederAvailability::Ready).await
    else {
        panic!("verified repost should process")
    };
    assert!(matches!(
        receipt.transition.unwrap().decision,
        Decision::Displaced { .. }
    ));
    let roster = f.store.snapshot(&f.owner).await.unwrap();
    assert_eq!(roster.roster.active_count(), 10);
    assert!(roster.roster.members[&public(13)].active);
    assert_eq!(f.core.feed_credit_sats().await.unwrap(), 0);
    let event = signed(14, 6, f.now, "", vec![tag(&["e", &f.note.event().id])]);
    social::stage(&f.store, &f.owner, event, f.now)
        .await
        .unwrap();
    assert_eq!(f.run(None, FeederAvailability::Ready).await, Step::Ignored);

    let empty = Fixture::new().await;
    let event = signed(
        12,
        7,
        empty.now - 1,
        "+",
        vec![tag(&["e", &empty.note.event().id])],
    );
    social::stage(&empty.store, &empty.owner, event, empty.now)
        .await
        .unwrap();
    assert_eq!(
        empty.run(None, FeederAvailability::Ready).await,
        Step::Ignored
    );
    assert_eq!(
        empty
            .store
            .snapshot(&empty.owner)
            .await
            .unwrap()
            .roster
            .active_count(),
        0
    );
}

#[tokio::test]
async fn missing_repost_profile_cannot_hold_paid_zap_or_confirmed_feed_and_late_retry_cannot_backpay()
 {
    let mut f = Fixture::new().await;
    f.core.initialize_reward_funding().await.unwrap();
    let repost = signed(
        2,
        6,
        f.now - 1,
        "",
        vec![tag(&["e", &f.note.event().id]), tag(&["p", &f.owner])],
    );
    let event_id = repost.event().id.clone();
    social::stage(&f.store, &f.owner, repost.clone(), f.now)
        .await
        .unwrap();
    f.paid(77, 3, 1000, &f.note).await;
    let feed = f.core.begin_feed_attempt(1000).await.unwrap().unwrap();
    f.core.confirm_feed_attempt(feed).await.unwrap();
    assert_eq!(f.ingest().await, 2);
    assert_eq!(f.run(None, FeederAvailability::Ready).await, Step::Ignored);
    assert_eq!(
        f.store
            .snapshot(&f.owner)
            .await
            .unwrap()
            .roster
            .active_count(),
        0
    );
    let pool = SqlitePool::connect(&f.database).await.unwrap();
    let disposition: String =
        sqlx::query_scalar("SELECT result_json FROM ingress WHERE source_key=?")
            .bind(format!("nostr:{event_id}"))
            .fetch_one(&pool)
            .await
            .unwrap();
    let disposition: serde_json::Value = serde_json::from_str(&disposition).unwrap();
    assert_eq!(disposition["status"], "parked");
    assert_eq!(disposition["reason"], "profile_unavailable");
    assert!(
        !f.store
            .queue_optional_social_retry(&f.owner, f.now + 600)
            .await
            .unwrap()
    );
    // Restart retains the pending financial order and the original signed event.
    f.store = Store::connect(&f.database).await.unwrap();
    assert!(matches!(
        f.run(Some(&f.profile(3)), FeederAvailability::Ready).await,
        Step::Interaction(_)
    ));
    let Step::Feeding(barrier) = f.run(None, FeederAvailability::Ready).await else {
        panic!("confirmed feeding must drain after the paid zap");
    };
    assert!(barrier.snapshot.roster.members[&public(3)].active);
    assert!(!barrier.snapshot.roster.members.contains_key(&public(2)));
    assert_eq!(f.core.feed_credit_sats().await.unwrap(), 0);
    let frozen = *barrier;
    f.now += 6;
    assert!(
        f.store
            .queue_optional_social_retry(&f.owner, f.now)
            .await
            .unwrap()
    );
    assert!(matches!(
        f.run(Some(&f.profile(2)), FeederAvailability::Ready).await,
        Step::Interaction(_)
    ));
    assert!(f.store.snapshot(&f.owner).await.unwrap().roster.members[&public(2)].active);
    assert_eq!(
        f.store
            .feeding_barrier(&f.owner, feed)
            .await
            .unwrap()
            .unwrap(),
        frozen
    );
    assert!(
        !social::stage(&f.store, &f.owner, repost, f.now)
            .await
            .unwrap()
    );
    assert!(
        !f.store
            .queue_optional_social_retry(&f.owner, f.now + 600)
            .await
            .unwrap()
    );
    assert_eq!(f.ingest().await, 0);
    assert_eq!(f.core.feed_credit_sats().await.unwrap(), 0);
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM observed_events WHERE event_id=?")
            .bind(event_id)
            .fetch_one(&pool)
            .await
            .unwrap(),
        1
    );
}

#[tokio::test]
async fn unavailable_social_eligibility_retries_with_backoff_without_holding_financial_work() {
    let mut f = Fixture::new().await;
    let repost = signed(2, 6, f.now - 1, "", vec![tag(&["e", &f.note.event().id])]);
    let id = repost.event().id.clone();
    social::stage(&f.store, &f.owner, repost, f.now)
        .await
        .unwrap();
    assert_eq!(
        f.run(Some(&f.profile(2)), FeederAvailability::Unknown)
            .await,
        Step::Ignored
    );
    let pool = SqlitePool::connect(&f.database).await.unwrap();
    let raw: String = sqlx::query_scalar("SELECT result_json FROM ingress WHERE source_key=?")
        .bind(format!("nostr:{id}"))
        .fetch_one(&pool)
        .await
        .unwrap();
    let disposition: serde_json::Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(disposition["reason"], "eligibility_unavailable");
    f.paid(80, 3, 1000, &f.note).await;
    let feed = f.core.begin_feed_attempt(1000).await.unwrap().unwrap();
    f.core.confirm_feed_attempt(feed).await.unwrap();
    f.ingest().await;
    assert!(matches!(
        f.run(Some(&f.profile(3)), FeederAvailability::Ready).await,
        Step::Interaction(_)
    ));
    let Step::Feeding(barrier) = f.run(None, FeederAvailability::Ready).await else {
        panic!("unavailable social eligibility blocked the financial queue");
    };
    assert!(!barrier.snapshot.roster.members.contains_key(&public(2)));
    f.now += 6;
    assert!(
        f.store
            .queue_optional_social_retry(&f.owner, f.now)
            .await
            .unwrap()
    );
    let retry = f.store.ingress_head(&f.owner).await.unwrap().unwrap();
    assert!(
        !f.store
            .queue_optional_social_retry(&f.owner, f.now)
            .await
            .unwrap()
    );
    assert_eq!(
        f.run(None, FeederAvailability::Unknown).await,
        Step::Ignored
    );
    // The second failed attempt waits ten seconds, across a real store reopen.
    f.store = Store::connect(&f.database).await.unwrap();
    f.now += 9;
    assert!(
        !f.store
            .queue_optional_social_retry(&f.owner, f.now)
            .await
            .unwrap()
    );
    f.now += 1;
    assert!(
        f.store
            .queue_optional_social_retry(&f.owner, f.now)
            .await
            .unwrap()
    );
    assert!(f.store.ingress_head(&f.owner).await.unwrap().unwrap().seq > retry.seq);
    // A frozen verified profile survives retries even when a new lookup is absent.
    assert!(matches!(
        f.run(None, FeederAvailability::Ready).await,
        Step::Interaction(_)
    ));
    assert!(f.store.snapshot(&f.owner).await.unwrap().roster.members[&public(2)].active);
    assert_eq!(
        f.store
            .feeding_barrier(&f.owner, feed)
            .await
            .unwrap()
            .unwrap(),
        *barrier
    );
    assert_eq!(f.core.feed_credit_sats().await.unwrap(), 0);
}

#[tokio::test]
async fn legacy_malformed_and_unsupported_social_heads_are_audited_before_runtime_lookup() {
    for (kind, body, expected) in [
        (6, "not a signed embedded target", "malformed"),
        (7, "-", "unsupported"),
    ] {
        let f = Fixture::new().await;
        let event = signed(
            2,
            kind,
            f.now - 1,
            body,
            vec![tag(&["e", &f.note.event().id])],
        );
        // Fixture-only legacy admission: the outer event remains cryptographically
        // valid. Current ingress rejects malformed bodies before enqueueing.
        let e = event.event();
        let pool = SqlitePool::connect(&f.database).await.unwrap();
        let mut tx = pool.begin().await.unwrap();
        sqlx::query("INSERT INTO observed_events(owner,event_id,kind,author,created_at,event_json) VALUES(?,?,?,?,?,?)")
            .bind(&f.owner).bind(&e.id).bind(e.kind as i64).bind(&e.pubkey).bind(e.created_at as i64)
            .bind(serde_json::to_string(e).unwrap()).execute(&mut *tx).await.unwrap();
        sqlx::query("INSERT INTO social_inbox(owner,event_id) VALUES(?,?)")
            .bind(&f.owner)
            .bind(&e.id)
            .execute(&mut *tx)
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO ingress(owner,source_key,kind,payload_json) VALUES(?,?,'social',?)",
        )
        .bind(&f.owner)
        .bind(format!("nostr:{}", e.id))
        .bind(serde_json::to_string(&e.id).unwrap())
        .execute(&mut *tx)
        .await
        .unwrap();
        tx.commit().await.unwrap();
        f.paid(78, 3, 1000, &f.note).await;
        let feed = f.core.begin_feed_attempt(1000).await.unwrap().unwrap();
        f.core.confirm_feed_attempt(feed).await.unwrap();
        f.ingest().await;
        assert!(
            worker::screen_social_head(&f.store, &f.owner, f.now)
                .await
                .unwrap()
        );
        let raw: String = sqlx::query_scalar("SELECT result_json FROM ingress WHERE source_key=?")
            .bind(format!("nostr:{}", e.id))
            .fetch_one(&pool)
            .await
            .unwrap();
        let result: serde_json::Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(result["status"], "ignored");
        assert_eq!(result["reason"], expected);
        assert_eq!(
            sqlx::query_scalar::<_, String>("SELECT state FROM social_inbox WHERE event_id=?")
                .bind(&e.id)
                .fetch_one(&pool)
                .await
                .unwrap(),
            "done"
        );
        assert!(matches!(
            f.run(Some(&f.profile(3)), FeederAvailability::Ready).await,
            Step::Interaction(_)
        ));
        assert!(matches!(
            f.run(None, FeederAvailability::Ready).await,
            Step::Feeding(_)
        ));
        assert_eq!(f.core.feed_credit_sats().await.unwrap(), 0);
        assert!(
            !f.store
                .queue_optional_social_retry(&f.owner, f.now + 600)
                .await
                .unwrap()
        );
    }
}

#[tokio::test]
async fn optional_social_disposition_cannot_skip_money_and_late_failure_rolls_back_both_checkpoints()
 {
    use crate::cyberherd::ingress::SocialHold;
    let f = Fixture::new().await;
    let repost = signed(2, 6, f.now - 1, "", vec![tag(&["e", &f.note.event().id])]);
    let id = repost.event().id.clone();
    social::stage(&f.store, &f.owner, repost, f.now)
        .await
        .unwrap();
    let head = f.store.ingress_head(&f.owner).await.unwrap().unwrap();
    let pool = SqlitePool::connect(&f.database).await.unwrap();
    sqlx::query("CREATE TRIGGER fail_optional_disposition BEFORE UPDATE OF state ON ingress WHEN NEW.state='done' BEGIN SELECT RAISE(ABORT,'synthetic disposition failure'); END").execute(&pool).await.unwrap();
    assert!(
        f.store
            .settle_optional_social(&f.owner, head.seq, &id, SocialHold::Malformed, f.now)
            .await
            .is_err()
    );
    assert_eq!(
        sqlx::query_scalar::<_, String>("SELECT state FROM social_inbox WHERE event_id=?")
            .bind(&id)
            .fetch_one(&pool)
            .await
            .unwrap(),
        "pending"
    );
    assert_eq!(
        f.store.ingress_head(&f.owner).await.unwrap().unwrap().seq,
        head.seq
    );
    sqlx::query("DROP TRIGGER fail_optional_disposition")
        .execute(&pool)
        .await
        .unwrap();
    f.store
        .settle_optional_social(&f.owner, head.seq, &id, SocialHold::Malformed, f.now)
        .await
        .unwrap();
    f.paid(79, 3, 1000, &f.note).await;
    f.ingest().await;
    let payment = f.store.ingress_head(&f.owner).await.unwrap().unwrap();
    assert!(
        f.store
            .settle_optional_social(&f.owner, payment.seq, &id, SocialHold::Malformed, f.now)
            .await
            .is_err()
    );
    assert_eq!(
        f.store.ingress_head(&f.owner).await.unwrap().unwrap().seq,
        payment.seq
    );
    assert_eq!(f.core.feed_credit_sats().await.unwrap(), 1000);
}

#[tokio::test]
async fn feeding_zap_notice_uses_consumed_credit_not_a_later_same_amount_donor() {
    let f = Fixture::new().await;
    f.core
        .configure_feeding_notices(Some(&f.owner))
        .await
        .unwrap();
    f.paid(101, 2, 995, &f.note).await;
    f.paid(102, 3, 10, &f.note).await;
    let feed = f.core.begin_feed_attempt(1000).await.unwrap().unwrap();
    // This later zap has exactly the same amount as the triggering zap. Its
    // sender must not replace the consumed credit's immutable attribution.
    f.paid(103, 4, 10, &f.note).await;
    f.core.confirm_feed_attempt(feed).await.unwrap();
    assert_eq!(f.core.feed_credit_sats().await.unwrap(), 15);
    let event = f
        .core
        .events_after(0, 100)
        .await
        .unwrap()
        .into_iter()
        .find(|e| e.event_type == "feeder_confirmed")
        .unwrap();
    let zap = f
        .core
        .verified_zap_attribution(&event)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(zap.sender(), Some(public(3).as_str()));
    assert_eq!(zap.amount_sats(), 10);
    assert_eq!(f.ingest().await, 4);
    for key in [2, 3, 4] {
        assert!(matches!(
            f.run(Some(&f.profile(key)), FeederAvailability::Ready)
                .await,
            Step::Interaction(_)
        ));
    }
    assert!(matches!(
        f.run(None, FeederAvailability::Ready).await,
        Step::Feeding(_)
    ));
    f.store.initialize_delivery(&f.owner, 0).await.unwrap();
    for catalog in [
        super::templates::Catalog::legacy().unwrap(),
        super::templates::Catalog::delivery_defaults().unwrap(),
    ] {
        f.store.seed_templates(&f.owner, &catalog).await.unwrap();
    }
    assert!(
        f.store
            .queue_next_feeding_announcement(&f.owner, &f.targets, f.now)
            .await
            .unwrap()
    );
    assert!(
        !f.store
            .queue_next_feeding_announcement(&f.owner, &f.targets, f.now)
            .await
            .unwrap()
    );
    while f
        .store
        .queue_next_delivery(&f.owner, &["wss://relay.damus.io/".into()], f.now)
        .await
        .unwrap()
    {}
    let overlay = f
        .store
        .pending_feeding_overlay(&f.owner)
        .await
        .unwrap()
        .unwrap();
    assert!(overlay.message.contains("User 3"));
    assert!(!overlay.message.contains("User 4"));
    assert!(overlay.message.contains("10"));
    for goat in &overlay.broadcast.goats {
        assert!(overlay.message.contains(&goat.name));
    }
    let mut feeding = None;
    for _ in 0..32 {
        let Some(job) = f.store.reserve_delivery(&f.owner, f.now).await.unwrap() else {
            break;
        };
        if job.feeding {
            feeding = Some(job);
            break;
        }
    }
    let job = feeding.expect("feeding note must be queued");
    assert!(job.unsigned.content.contains(&zap.nostr_name().unwrap()));
    assert!(job.unsigned.tags.contains(&vec!["p".into(), public(3)]));
    assert!(!job.unsigned.tags.contains(&vec!["p".into(), public(4)]));
    // Presentation never debits again, including a reopen and another queue pass.
    let reopened = Store::connect(&f.database).await.unwrap();
    assert!(
        !reopened
            .queue_next_feeding_announcement(&f.owner, &f.targets, f.now)
            .await
            .unwrap()
    );
    assert_eq!(f.core.feed_credit_sats().await.unwrap(), 15);
}
