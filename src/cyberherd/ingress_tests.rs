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
        let root = TempDir::new().unwrap();
        let core = LedgerStore::connect(&format!(
            "sqlite://{}",
            root.path().join("core.db").display()
        ))
        .await
        .unwrap();
        let database = format!("sqlite://{}", root.path().join("herd.db").display());
        let store = Store::connect(&database).await.unwrap();
        let now = crate::cyberherd::calendar::now().unwrap();
        let owner = public(1);
        store
            .initialize(
                &owner,
                &Settings {
                    minimum_sats: 10,
                    max_members: 10,
                    member_percent: 10,
                },
                now - 100,
            )
            .await
            .unwrap();
        let note = signed(1, 1, now - 50, "today", vec![tag(&["t", "CyberHerd"])]);
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
        version: 1,
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
        crate::cyberherd::ingress::hash(&(f.now - 100, f.targets.current(), Vec::<String>::new()))
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
