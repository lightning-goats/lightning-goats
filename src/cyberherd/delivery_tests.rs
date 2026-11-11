//! Temporary stores, real private IPC and synthetic signatures/relay ACKs only.
use super::*;
use crate::{
    config::RuntimeMode,
    core_bridge::{
        BridgeConfig,
        unix::{Server, self_credentials},
    },
    cyberherd::{
        calendar::DayWindow,
        conversation::MemberConversation,
        membership::{Action, FeederAvailability, Profile, Settings, Target},
        store::Interaction,
        targets::{ProviderCache, StreamAuthentication, TargetPolicy, Targets},
    },
    gateway::FeederSafety,
    ledger::LedgerStore,
    nostr_event::test_event,
};
use sqlx::SqlitePool;
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
};
use uuid::Uuid;

fn key(n: u8) -> String {
    test_event(n, 0, 100, "", vec![]).pubkey
}
#[derive(Clone, Default)]
struct Fake {
    signs: Arc<AtomicUsize>,
    events: Arc<Mutex<Vec<crate::nostr::SignedNostrEvent>>>,
    lose: Arc<AtomicBool>,
}
impl Transport for Fake {
    async fn sign(&self, u: &Unsigned) -> Result<VerifiedEvent> {
        self.signs.fetch_add(1, Ordering::SeqCst);
        VerifiedEvent::verify(test_event(
            1,
            u.kind,
            u.created_at,
            &u.content,
            u.tags.clone(),
        ))
    }
    async fn publish(&self, event: &VerifiedEvent, relay: &str) -> Result<RelayAck> {
        self.events.lock().unwrap().push(event.event().clone());
        if self.lose.swap(false, Ordering::SeqCst) {
            bail!("synthetic lost relay acknowledgement");
        }
        Ok(RelayAck {
            event_id: event.event().id.clone(),
            relay: relay.into(),
            at: calendar::now()?,
        })
    }
}
struct Fixture {
    root: tempfile::TempDir,
    store: Store,
    core: LedgerStore,
    url: String,
    core_url: String,
    targets: Targets,
    owner: String,
}
impl Fixture {
    async fn new() -> Self {
        let root = tempfile::TempDir::new().unwrap();
        let url = format!("sqlite://{}", root.path().join("members.db").display());
        let core_url = format!("sqlite://{}", root.path().join("core.db").display());
        let store = Store::connect(&url).await.unwrap();
        let core = LedgerStore::connect(&core_url).await.unwrap();
        let now = calendar::now().unwrap();
        let owner = key(1);
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
        store
            .seed_templates(
                &owner,
                &crate::cyberherd::templates::Catalog::legacy().unwrap(),
            )
            .await
            .unwrap();
        store
            .seed_templates(
                &owner,
                &crate::cyberherd::templates::Catalog::delivery_defaults().unwrap(),
            )
            .await
            .unwrap();
        store
            .initialize_core_ingress(
                &owner,
                &crate::cyberherd::ingress::CoreBinding {
                    core_uid: self_credentials().unwrap().0,
                    epoch: core.overlay_stream_id().await.unwrap(),
                    start_after: 0,
                    credit_pool: "herd".into(),
                    payees: BTreeMap::from([("herd".into(), owner.clone())]),
                },
            )
            .await
            .unwrap();
        store.initialize_delivery(&owner, 0).await.unwrap();
        let note = VerifiedEvent::verify(test_event(
            1,
            1,
            now as u64,
            "today",
            vec![vec!["t".into(), "CyberHerd".into()]],
        ))
        .unwrap();
        let stream = VerifiedEvent::verify(test_event(
            1,
            30311,
            now as u64,
            "",
            vec![
                vec!["d".into(), "original-stream".into()],
                vec!["status".into(), "live".into()],
            ],
        ))
        .unwrap();
        let StreamAuthentication::Accepted(stream) = ProviderCache::default()
            .authenticate(stream, &owner, now)
            .await
            .unwrap()
        else {
            panic!("project stream must authenticate")
        };
        let targets = Targets::project(
            TargetPolicy {
                project_pubkey: owner.clone(),
                tracked_tags: vec!["CyberHerd".into()],
            },
            &DayWindow::at("America/Denver", now).unwrap(),
            vec![note],
            vec![*stream],
            BTreeSet::new(),
            now,
        )
        .unwrap();
        Self {
            root,
            store,
            core,
            url,
            core_url,
            targets,
            owner,
        }
    }
    async fn apply(&self, source: &str, amount: u64) -> MemberConversation {
        let note = self
            .targets
            .current()
            .into_iter()
            .find(|t| t.kind == 1)
            .unwrap();
        let context = self.targets.interaction_context(note);
        self.store
            .process_with_context(
                &self.owner,
                &Interaction {
                    source_id: format!("settlement:{source}"),
                    source_hash: "44".repeat(32),
                    pubkey: key(2),
                    profile: Profile {
                        name: "Alice".into(),
                        picture: None,
                        lud16: Some("alice@example.com".into()),
                    },
                    action: Action::Zap {
                        amount_sats: amount,
                    },
                    occurred_at: calendar::now().unwrap(),
                },
                Target::Current,
                FeederAvailability::Ready,
                Some(&context),
            )
            .await
            .unwrap();
        let db = SqlitePool::connect(&self.url).await.unwrap();
        let raw: String = sqlx::query_scalar(
            "SELECT conversation_json FROM member_conversations WHERE owner=? AND member=?",
        )
        .bind(&self.owner)
        .bind(key(2))
        .fetch_one(&db)
        .await
        .unwrap();
        serde_json::from_str(&raw).unwrap()
    }
    async fn queue(&self) {
        while self
            .store
            .queue_next_delivery(
                &self.owner,
                &["wss://relay.damus.io/".into()],
                calendar::now().unwrap(),
            )
            .await
            .unwrap()
        {}
    }
    async fn ready(&self, paused: Option<bool>) {
        self.core
            .update_feeding_presentation(
                1000,
                RuntimeMode::Active,
                paused.map(|on| FeederSafety {
                    override_enabled: on,
                    remote_enabled: true,
                }),
            )
            .await
            .unwrap();
    }
    async fn bridge(&self, mode: RuntimeMode) -> (Client, tokio::task::JoinHandle<Result<()>>) {
        self.bridge_role(mode, false).await
    }
    async fn bridge_role(
        &self,
        mode: RuntimeMode,
        payout: bool,
    ) -> (Client, tokio::task::JoinHandle<Result<()>>) {
        let (uid, gid) = self_credentials().unwrap();
        std::fs::set_permissions(
            self.root.path(),
            std::os::unix::fs::PermissionsExt::from_mode(0o750),
        )
        .unwrap();
        let config = BridgeConfig {
            socket: self
                .root
                .path()
                .join(format!("{}.sock", Uuid::new_v4().simple())),
            peer_uid: if payout { uid + 1 } else { uid },
            group_gid: gid,
            reward_policy: Some(crate::reward_funding::RewardPolicy {
                owner: self.owner.clone(),
                member_percent_ceiling: 0,
                max_sats_per_feed: 0,
                max_sats_per_24h: 0,
            }),
            payout_peer_uid: Some(if payout { uid } else { uid + 1 }),
            delegated_feeder_messages: false,
            payout_execution_enabled: false,
            payout_account_id: None,
        };
        let server = Server::bind(&config).await.unwrap();
        let task = tokio::spawn(server.run(self.core.clone(), mode));
        (Client::new(config.socket, uid).unwrap(), task)
    }
}
async fn tick(f: &Fixture, worker: &Worker<Fake>) -> Result<Step> {
    tokio::time::sleep(std::time::Duration::from_millis(1050)).await;
    f.ready(Some(false)).await;
    worker.tick().await
}

#[tokio::test]
async fn note_and_stream_outboxes_recover_lost_ack_without_resigning_and_keep_independent_joining_roots()
 {
    let f = Fixture::new().await;
    let conversation = f.apply("join", 34).await;
    f.queue().await;
    assert!(
        f.store
            .published_joining(&f.owner, &conversation, 1, &key(2))
            .await
            .unwrap()
            .is_none()
    );
    let (core, task) = f.bridge(RuntimeMode::Active).await;
    let fake = Fake::default();
    fake.lose.store(true, Ordering::SeqCst);
    let worker = Worker {
        store: f.store.clone(),
        core,
        transport: fake.clone(),
        owner: f.owner.clone(),
    };
    assert!(tick(&f, &worker).await.is_err());
    assert!(
        f.store
            .published_joining(&f.owner, &conversation, 1, &key(2))
            .await
            .unwrap()
            .is_none()
    );
    let reopened = Store::connect(&f.url).await.unwrap();
    let worker = Worker {
        store: reopened.clone(),
        core: worker.core,
        transport: fake.clone(),
        owner: f.owner.clone(),
    };
    assert_eq!(tick(&f, &worker).await.unwrap(), Step::Published);
    tokio::time::sleep(std::time::Duration::from_secs(5)).await;
    assert_eq!(tick(&f, &worker).await.unwrap(), Step::Published);
    assert_eq!(fake.signs.load(Ordering::SeqCst), 2);
    let first = fake.events.lock().unwrap().clone();
    assert_eq!(first.len(), 3);
    assert_eq!(
        first.iter().map(|e| &e.id).collect::<BTreeSet<_>>().len(),
        2
    );
    assert!(first.iter().any(|e| e.kind == 1));
    assert!(first.iter().any(|e| e.kind == 1311));
    let reference = reopened
        .published_joining(&f.owner, &conversation, 1, &key(2))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(reference.event.kind, 1);
    let message = reopened
        .pending_member_overlay(&f.owner)
        .await
        .unwrap()
        .unwrap();
    assert!(message.message.contains("Alice"));
    assert!(!message.message.contains("nostr:npub"));
    f.ready(Some(false)).await;
    let seq = worker.core.member_overlay(&message).await.unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(600)).await;
    assert_eq!(worker.core.member_overlay(&message).await.unwrap(), seq);
    reopened
        .acknowledge_member_overlay(&message, seq)
        .await
        .unwrap();
    assert!(
        reopened
            .pending_member_overlay(&f.owner)
            .await
            .unwrap()
            .is_none()
    );
    f.apply("increase", 34).await;
    f.queue().await;
    assert_eq!(tick(&f, &worker).await.unwrap(), Step::Published);
    assert_eq!(tick(&f, &worker).await.unwrap(), Step::Published);
    {
        let all = fake.events.lock().unwrap();
        for event in all.iter().skip(3) {
            let starter = first.iter().find(|e| e.kind == event.kind).unwrap();
            assert!(
                event
                    .tags
                    .iter()
                    .any(|t| t.first().is_some_and(|x| x == "e")
                        && t.get(1) == Some(&starter.id)
                        && t.get(3).is_some_and(|x| x == "reply"))
            );
            assert!(event.content.contains("34 sats"));
            assert!(event.content.contains("68"));
            assert!(!event.content.contains("Current CyberHerd shares"));
        }
    }
    assert_eq!(f.core.feed_credit_sats().await.unwrap(), 0);
    let db = SqlitePool::connect(&f.core_url).await.unwrap();
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM message_outbox")
            .fetch_one(&db)
            .await
            .unwrap(),
        0
    );
    task.abort();
}

#[tokio::test]
async fn paused_unknown_and_canary_never_sign_or_publish_member_threads() {
    let f = Fixture::new().await;
    f.apply("join", 34).await;
    f.queue().await;
    let fake = Fake::default();
    let (core, task) = f.bridge(RuntimeMode::Active).await;
    let worker = Worker {
        store: f.store.clone(),
        core,
        transport: fake.clone(),
        owner: f.owner.clone(),
    };
    for paused in [Some(true), None] {
        f.ready(paused).await;
        assert!(matches!(
            worker.tick().await.unwrap(),
            Step::Waiting | Step::Idle
        ));
        tokio::time::sleep(std::time::Duration::from_millis(600)).await;
    }
    task.abort();
    let (core, task) = f.bridge(RuntimeMode::Canary).await;
    let worker = Worker {
        store: f.store.clone(),
        core,
        transport: fake.clone(),
        owner: f.owner.clone(),
    };
    f.ready(Some(false)).await;
    assert!(matches!(
        worker.tick().await.unwrap(),
        Step::Waiting | Step::Idle
    ));
    assert_eq!(fake.signs.load(Ordering::SeqCst), 0);
    assert!(fake.events.lock().unwrap().is_empty());
    // Canary still supports overlay-only staged testing under the same owner.
    let message = f
        .store
        .pending_member_overlay(&f.owner)
        .await
        .unwrap()
        .unwrap();
    let seq = worker.core.member_overlay(&message).await.unwrap();
    assert!(seq > 0);
    task.abort();
}

#[tokio::test]
async fn low_payment_floor_template_opt_out_and_unchanged_allocation_do_not_emit_extra_messages() {
    let f = Fixture::new().await;
    f.apply("join", 34).await;
    f.queue().await;
    f.apply("small", 5).await;
    f.queue().await;
    assert_eq!(
        f.store.snapshot(&f.owner).await.unwrap().roster.members[&key(2)].amount_sats,
        39
    );
    let db = SqlitePool::connect(&f.url).await.unwrap();
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM delivery_messages")
            .fetch_one(&db)
            .await
            .unwrap(),
        1
    );
    f.store
        .edit_template(
            &f.owner,
            "opt-out",
            &crate::cyberherd::store::TemplateEdit::DisableCategory {
                category: "member_increase".into(),
            },
        )
        .await
        .unwrap();
    f.store
        .seed_templates(
            &f.owner,
            &crate::cyberherd::templates::Catalog::legacy().unwrap(),
        )
        .await
        .unwrap();
    f.apply("muted", 34).await;
    f.queue().await;
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM delivery_messages")
            .fetch_one(&db)
            .await
            .unwrap(),
        1
    );
    assert_eq!(
        crate::cyberherd::conversation::identity(&f.owner, &key(2), "msg:msg:settlement:join")
            .unwrap(),
        crate::cyberherd::conversation::identity(&f.owner, &key(2), "settlement:join").unwrap()
    );
}

#[tokio::test]
async fn idle_member_publisher_uses_neither_core_admission_nor_remote_signing() {
    let f = Fixture::new().await;
    let fake = Fake::default();
    let worker = Worker {
        store: f.store.clone(),
        core: Client::new(
            f.root.path().join("no-core-listener.sock"),
            self_credentials().unwrap().0,
        )
        .unwrap(),
        transport: fake.clone(),
        owner: f.owner.clone(),
    };
    assert_eq!(worker.tick().await.unwrap(), Step::Idle);
    assert_eq!(fake.signs.load(Ordering::SeqCst), 0);
    assert!(fake.events.lock().unwrap().is_empty());
}

#[tokio::test]
async fn frozen_unsigned_signature_and_ack_conflicts_cannot_create_a_joining_reference() {
    let f = Fixture::new().await;
    let conversation = f.apply("join", 34).await;
    f.queue().await;
    let job = f
        .store
        .reserve_delivery(&f.owner, calendar::now().unwrap())
        .await
        .unwrap()
        .unwrap();
    let wrong = VerifiedEvent::verify(test_event(
        1,
        job.unsigned.kind,
        job.unsigned.created_at,
        "altered",
        job.unsigned.tags.clone(),
    ))
    .unwrap();
    assert!(f.store.save_delivery_signature(&job, &wrong).await.is_err());
    let signed = Fake::default().sign(&job.unsigned).await.unwrap();
    f.store
        .save_delivery_signature(&job, &signed)
        .await
        .unwrap();
    let wrong = RelayAck {
        event_id: "00".repeat(32),
        relay: job.relay.clone(),
        at: calendar::now().unwrap(),
    };
    assert!(
        f.store
            .finish_delivery(&job, Some(&wrong), calendar::now().unwrap())
            .await
            .is_err()
    );
    assert!(
        f.store
            .published_joining(&f.owner, &conversation, 1, &key(2))
            .await
            .unwrap()
            .is_none()
    );
    let db = SqlitePool::connect(&f.url).await.unwrap();
    for sql in [
        "DELETE FROM delivery_signed",
        "UPDATE delivery_unsigned SET document_json='{}'",
        "DELETE FROM delivery_messages",
    ] {
        assert!(sqlx::query(sql).execute(&db).await.is_err());
    }
    // Helper rendering stays exact even beyond binary float's integer range.
    assert_eq!(
        crate::cyberherd::delivery::ratio(9_007_199_254_740_993, 10).unwrap(),
        "900719925474099.3"
    );
}

#[tokio::test]
async fn treat_messages_wait_for_real_joining_ack_and_reply_to_the_saved_stream_branch() {
    let f = Fixture::new().await;
    let conversation = f.apply("join", 34).await;
    f.queue().await;
    let first = f
        .store
        .reserve_delivery(&f.owner, calendar::now().unwrap())
        .await
        .unwrap()
        .unwrap();
    let second = f
        .store
        .reserve_delivery(&f.owner, calendar::now().unwrap())
        .await
        .unwrap()
        .unwrap();
    let fake = Fake::default();
    let first_event = fake.sign(&first.unsigned).await.unwrap();
    let second_event = fake.sign(&second.unsigned).await.unwrap();
    f.store
        .save_delivery_signature(&first, &first_event)
        .await
        .unwrap();
    f.store
        .save_delivery_signature(&second, &second_event)
        .await
        .unwrap();
    let db = SqlitePool::connect(&f.url).await.unwrap();
    let row = crate::cyberherd::rewards::Row {
        pubkey: key(2),
        profile_event_id: None,
        profile: Some(Profile {
            name: "Alice".into(),
            picture: None,
            lud16: Some("alice@example.com".into()),
        }),
        amount_sats: 34,
        awards: crate::cyberherd::shares::SocialAwards::default(),
        point_units: "340".into(),
        share_micro_percent: 10_000_000,
        gross_sats: 10,
        fee_limit_sats: 1,
        net_sats: 9,
        gross_micro_percent_of_feed: 1_000_000,
        recipient: None,
        skip: None,
        conversation: Some(conversation.clone()),
        score_basis: Some(10),
        zap_content: Some("Frozen zap text".into()),
        zap_wording: None,
    };
    let payload = serde_json::json!({"result":crate::payout::results::Row{member:key(2),gross_sats:10,verified_at:calendar::now().unwrap(),result:crate::payout::results::Status::Completed{net_sats:9,fee_sats:1}},"breakdown":row});
    sqlx::query("INSERT INTO domain_events(owner,cycle,source_id,event_type,snapshot_json) VALUES(?,1,'payout:synthetic-confirmed-result','payout_result',?)")
        .bind(&f.owner).bind(payload.to_string()).execute(&db).await.unwrap();
    f.queue().await;
    assert!(
        f.store
            .reserve_delivery(&f.owner, calendar::now().unwrap())
            .await
            .unwrap()
            .is_none()
    );
    // Saving a signature did not create either a thread anchor or a Nostr treat post.
    assert!(
        f.store
            .published_joining(&f.owner, &conversation, 1, &key(2))
            .await
            .unwrap()
            .is_none()
    );
    // The verified human-name result is independently deliverable in canary;
    // it creates no Nostr joining receipt or reply target.
    let (core, overlay_task) = f.bridge(RuntimeMode::Canary).await;
    f.ready(Some(false)).await;
    let initial = f
        .store
        .pending_member_overlay(&f.owner)
        .await
        .unwrap()
        .unwrap();
    let seq = core.member_overlay(&initial).await.unwrap();
    f.store
        .acknowledge_member_overlay(&initial, seq)
        .await
        .unwrap();
    let treat = f
        .store
        .pending_member_overlay(&f.owner)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(treat.category, "cyber_herd_treats");
    assert!(treat.message.contains("Alice"));
    assert!(treat.message.contains("9 sats"));
    let seq = core.member_overlay(&treat).await.unwrap();
    f.store
        .acknowledge_member_overlay(&treat, seq)
        .await
        .unwrap();
    assert!(
        f.store
            .published_joining(&f.owner, &conversation, 1, &key(2))
            .await
            .unwrap()
            .is_none()
    );
    overlay_task.abort();
    let ack = fake.publish(&first_event, &first.relay).await.unwrap();
    f.store
        .finish_delivery(&first, Some(&ack), calendar::now().unwrap())
        .await
        .unwrap();
    let ack = fake.publish(&second_event, &second.relay).await.unwrap();
    f.store
        .finish_delivery(&second, Some(&ack), calendar::now().unwrap())
        .await
        .unwrap();
    // Waiting rows were rotated with a two-second backoff, without inventing
    // newer target addresses or replacing already frozen joining bytes.
    tokio::time::sleep(std::time::Duration::from_secs(3)).await;
    let jobs = [
        f.store
            .reserve_delivery(&f.owner, calendar::now().unwrap())
            .await
            .unwrap()
            .unwrap(),
        f.store
            .reserve_delivery(&f.owner, calendar::now().unwrap())
            .await
            .unwrap()
            .unwrap(),
    ];
    for job in jobs {
        let joining = if job.unsigned.kind == first.unsigned.kind {
            &first_event
        } else {
            &second_event
        };
        assert!(
            job.unsigned
                .tags
                .iter()
                .any(|t| t.first().is_some_and(|v| v == "e")
                    && t.get(1) == Some(&joining.event().id)
                    && t.get(3).is_some_and(|v| v == "reply"))
        );
        if job.unsigned.kind == 1311 {
            assert!(job.unsigned.tags.iter().any(|t| {
                t.first().is_some_and(|v| v == "a")
                    && t.get(1)
                        == conversation
                            .context
                            .stream
                            .as_ref()
                            .unwrap()
                            .coordinate
                            .as_ref()
            }));
        }
        assert!(job.unsigned.content.contains("9 sats"));
        assert!(job.unsigned.content.contains("34 sats"));
    }
}

#[tokio::test]
async fn overlay_import_enforces_pause_owner_epoch_role_and_exact_receipts_without_financial_events()
 {
    let f = Fixture::new().await;
    f.apply("join", 34).await;
    f.queue().await;
    let message = f
        .store
        .pending_member_overlay(&f.owner)
        .await
        .unwrap()
        .unwrap();
    let (core, task) = f.bridge(RuntimeMode::Active).await;
    f.ready(Some(true)).await;
    assert!(core.member_overlay(&message).await.is_err());
    tokio::time::sleep(std::time::Duration::from_millis(600)).await;
    f.ready(None).await;
    assert!(core.member_overlay(&message).await.is_err());
    tokio::time::sleep(std::time::Duration::from_millis(600)).await;
    f.ready(Some(false)).await;
    let seq = core.member_overlay(&message).await.unwrap();
    let mut changed = message.clone();
    changed.owner = key(3);
    assert!(core.member_overlay(&changed).await.is_err());
    tokio::time::sleep(std::time::Duration::from_millis(600)).await;
    let mut changed = message.clone();
    changed.epoch = Uuid::new_v4();
    assert!(core.member_overlay(&changed).await.is_err());
    tokio::time::sleep(std::time::Duration::from_millis(600)).await;
    let mut changed = message.clone();
    changed.message = "replacement text".into();
    assert!(core.member_overlay(&changed).await.is_err());
    for category in [
        "payment_received",
        "feeder_confirmed",
        "feeder_trigger",
        "weather_status",
        "interface_info",
        "herd_reset_message",
    ] {
        let mut changed = message.clone();
        changed.category = category.into();
        assert!(changed.validate().is_err());
    }
    task.abort();
    let (uid, gid) = self_credentials().unwrap();
    let config = BridgeConfig {
        socket: f.root.path().join("payout-role.sock"),
        peer_uid: uid + 1,
        group_gid: gid,
        reward_policy: Some(crate::reward_funding::RewardPolicy {
            owner: f.owner.clone(),
            member_percent_ceiling: 0,
            max_sats_per_feed: 0,
            max_sats_per_24h: 0,
        }),
        payout_peer_uid: Some(uid),
        delegated_feeder_messages: false,
        payout_execution_enabled: false,
        payout_account_id: None,
    };
    let task = tokio::spawn(
        Server::bind(&config)
            .await
            .unwrap()
            .run(f.core.clone(), RuntimeMode::Active),
    );
    let payout = Client::new(config.socket, uid).unwrap();
    assert!(payout.member_overlay(&message).await.is_err());
    let events = f.core.events_after(0, 100).await.unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].seq, seq);
    assert_eq!(events[0].event_type, "cyberherd_message");
    let exported = f
        .core
        .core_event_batch(Some(message.epoch), 0, 128)
        .await
        .unwrap();
    assert!(
        exported
            .events
            .iter()
            .all(|e| matches!(e.body, crate::core_bridge::EventBody::Other))
    );
    assert_eq!(f.core.feed_credit_sats().await.unwrap(), 0);
    task.abort();
}

// Synthetic operator credits below create no real reward authority or physical
// request. confirm_feed_attempt simulates the gateway's correlated confirmed result.
async fn feed_credit(f: &Fixture, id: &str, sats: u64) {
    f.core
        .grant_operator_credit(
            &crate::ledger::OperatorCreditGrant {
                id: id.into(),
                amount_sats: sats,
                address_user: "herd".into(),
                reason: "isolated feeding presentation fixture".into(),
            },
            false,
        )
        .await
        .unwrap();
}
async fn confirm_food(f: &Fixture) -> Uuid {
    let id = f.core.begin_feed_attempt(1000).await.unwrap().unwrap();
    f.core.confirm_feed_attempt(id).await.unwrap();
    id
}
async fn freeze_food(f: &Fixture, client: &Client) {
    crate::cyberherd::worker::poll_core(&f.store, &f.owner, client)
        .await
        .unwrap();
    loop {
        let step = crate::cyberherd::worker::step(
            &f.store,
            &f.owner,
            crate::cyberherd::worker::Environment {
                targets: &f.targets,
                discovery_available: true,
                feeder: FeederAvailability::Ready,
                profile: None,
                now: calendar::now().unwrap(),
            },
        )
        .await
        .unwrap();
        if step == crate::cyberherd::worker::Step::Idle {
            break;
        }
        assert!(matches!(step, crate::cyberherd::worker::Step::Feeding(_)));
    }
}
async fn generate_food(f: &Fixture, targets: &Targets) {
    while f
        .store
        .queue_next_feeding_announcement(&f.owner, targets, calendar::now().unwrap())
        .await
        .unwrap()
    {}
    f.queue().await;
}

#[tokio::test]
async fn confirmed_feeding_uses_frozen_roster_trigger_amount_and_one_standalone_note_stream_pair() {
    let f = Fixture::new().await;
    f.apply("join-before-food", 34).await;
    f.core
        .configure_feeding_notices(Some(&f.owner))
        .await
        .unwrap();
    feed_credit(&f, "saved", 995).await;
    feed_credit(&f, "crossing", 10).await;
    let id = f.core.begin_feed_attempt(1000).await.unwrap().unwrap();
    feed_credit(&f, "arrived-during-dispatch", 200).await;
    f.core.confirm_feed_attempt(id).await.unwrap();
    assert_eq!(f.core.feed_credit_sats().await.unwrap(), 205);
    let source = f
        .core
        .events_after(0, 100)
        .await
        .unwrap()
        .into_iter()
        .find(|e| e.event_type == "feeder_confirmed")
        .unwrap();
    let rendered = crate::presentation::MessageRenderer::embedded()
        .unwrap()
        .render(&source, 1000)
        .unwrap();
    assert_eq!(rendered.overlay_type.as_deref(), Some("feeder_trigger"));
    assert!(rendered.overlay_message.is_none());
    assert!(rendered.nostr_content.is_none());
    f.core.advance_message_cursor(source.seq - 1).await.unwrap();
    assert_eq!(
        crate::messaging::process_next_message(
            &f.core,
            None,
            &crate::presentation::MessageRenderer::embedded().unwrap(),
            1000,
            RuntimeMode::Active
        )
        .await
        .unwrap(),
        crate::messaging::MessageProcessorStep::NonPublicSkipped { seq: source.seq }
    );
    let (client, task) = f.bridge(RuntimeMode::Active).await;
    freeze_food(&f, &client).await;
    let frozen = f
        .store
        .feeding_barrier(&f.owner, id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        frozen
            .feeding
            .presentation
            .as_ref()
            .unwrap()
            .triggering_payment_amount_sats,
        Some(10)
    );
    f.apply("later-increase", 34).await;
    generate_food(&f, &f.targets).await;
    let newer = VerifiedEvent::verify(test_event(
        1,
        30311,
        calendar::now().unwrap() as u64,
        "",
        vec![
            vec!["d".into(), "replacement-stream".into()],
            vec!["status".into(), "live".into()],
        ],
    ))
    .unwrap();
    let StreamAuthentication::Accepted(newer) = ProviderCache::default()
        .authenticate(newer, &f.owner, calendar::now().unwrap())
        .await
        .unwrap()
    else {
        panic!("new project stream must authenticate")
    };
    let changed = Targets::project(
        TargetPolicy {
            project_pubkey: f.owner.clone(),
            tracked_tags: vec!["CyberHerd".into()],
        },
        &DayWindow::at("America/Denver", calendar::now().unwrap()).unwrap(),
        vec![],
        vec![*newer],
        BTreeSet::new(),
        calendar::now().unwrap(),
    )
    .unwrap();
    assert!(
        !f.store
            .queue_next_feeding_announcement(&f.owner, &changed, calendar::now().unwrap())
            .await
            .unwrap()
    );
    let db = SqlitePool::connect(&f.url).await.unwrap();
    let raw:String=sqlx::query_scalar("SELECT document_json FROM delivery_messages WHERE json_type(document_json,'$.broadcast')='object'").fetch_one(&db).await.unwrap();
    let message: crate::cyberherd::delivery::Rendered = serde_json::from_str(&raw).unwrap();
    assert!(message.nostr.contains("10 sats"));
    assert!(message.overlay.contains("10 sats"));
    assert!(!message.nostr.contains("1000 sats"));
    assert!(message.nostr.contains("1 active members · 34 total points"));
    assert!(message.nostr.contains("Member allocation: 10%"));
    assert!(!message.overlay.contains("nostr:"));
    let broadcast = message.broadcast.as_ref().unwrap();
    assert_eq!(broadcast.request_id, id);
    assert_eq!(broadcast.proof.feed_credit_sats, 205);
    let destinations: Vec<String> = sqlx::query_scalar(
        "SELECT destination FROM delivery_destinations WHERE identity=? ORDER BY destination",
    )
    .bind(&message.identity)
    .fetch_all(&db)
    .await
    .unwrap();
    assert_eq!(destinations.len(), 2);
    assert!(destinations.contains(&"broadcast:note".to_owned()));
    assert!(destinations.iter().any(|s| s.ends_with(":original-stream")));
    let mut jobs = vec![];
    while let Some(job) = f
        .store
        .reserve_delivery(&f.owner, calendar::now().unwrap())
        .await
        .unwrap()
    {
        if job.feeding {
            jobs.push(job);
        }
        if jobs.len() == 2 {
            break;
        }
    }
    assert_eq!(jobs.len(), 2);
    let note = &jobs.iter().find(|j| j.unsigned.kind == 1).unwrap().unsigned;
    let chat = &jobs
        .iter()
        .find(|j| j.unsigned.kind == 1311)
        .unwrap()
        .unsigned;
    assert_eq!(note.kind, 1);
    assert!(
        !note
            .tags
            .iter()
            .any(|t| matches!(t[0].as_str(), "e" | "a" | "p"))
    );
    assert_eq!(chat.kind, 1311);
    assert_eq!(chat.content, note.content);
    assert!(
        chat.tags
            .iter()
            .any(|t| t[0] == "a" && t[1].ends_with(":original-stream") && t[3] == "root")
    );
    assert!(!chat.tags.iter().any(|t| matches!(t[0].as_str(), "p" | "e")));
    assert_eq!(f.core.feed_credit_sats().await.unwrap(), 205);
    task.abort();
}

#[tokio::test]
async fn feeding_batch_deduplicates_presentation_but_retains_every_confirmed_reward_barrier_after_restart()
 {
    let f = Fixture::new().await;
    f.core
        .configure_feeding_notices(Some(&f.owner))
        .await
        .unwrap();
    feed_credit(&f, "one-payment", 2345).await;
    let first = confirm_food(&f).await;
    let second = confirm_food(&f).await;
    let (client, task) = f.bridge(RuntimeMode::Canary).await;
    freeze_food(&f, &client).await;
    let a = f
        .store
        .feeding_barrier(&f.owner, first)
        .await
        .unwrap()
        .unwrap();
    let b = f
        .store
        .feeding_barrier(&f.owner, second)
        .await
        .unwrap()
        .unwrap();
    assert!(a.feeding.presentation.is_some());
    assert!(b.feeding.presentation.is_none());
    assert_eq!(
        a.feeding
            .presentation
            .as_ref()
            .unwrap()
            .triggering_payment_amount_sats,
        Some(2345)
    );
    generate_food(&f, &f.targets).await;
    let reopened = Store::connect(&f.url).await.unwrap();
    assert!(
        !reopened
            .queue_next_feeding_announcement(&f.owner, &f.targets, calendar::now().unwrap())
            .await
            .unwrap()
    );
    let db = SqlitePool::connect(&f.url).await.unwrap();
    let receipts: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM feeding_announcements")
        .fetch_one(&db)
        .await
        .unwrap();
    let messages: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM delivery_messages")
        .fetch_one(&db)
        .await
        .unwrap();
    let barriers: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM feeding_barriers")
        .fetch_one(&db)
        .await
        .unwrap();
    // Both physical-confirmation receipts remain; only one public message exists.
    assert_eq!((receipts, messages, barriers), (2, 1, 2));
    assert_eq!(f.core.feed_credit_sats().await.unwrap(), 345);
    assert!(
        sqlx::query("DELETE FROM feeding_announcements")
            .execute(&db)
            .await
            .is_err()
    );
    // Two equally sized but distinct later payments are distinct announcements.
    feed_credit(&f, "next-payment", 1000).await;
    confirm_food(&f).await;
    tokio::time::sleep(std::time::Duration::from_millis(600)).await;
    freeze_food(&f, &client).await;
    generate_food(&f, &f.targets).await;
    let messages: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM delivery_messages")
        .fetch_one(&db)
        .await
        .unwrap();
    assert_eq!(messages, 2);
    task.abort();
}

#[tokio::test]
async fn feeding_overlay_requires_actual_owner_epoch_uuid_and_preserves_exact_receipt_without_extra_food()
 {
    let f = Fixture::new().await;
    f.core
        .configure_feeding_notices(Some(&f.owner))
        .await
        .unwrap();
    feed_credit(&f, "one-payment", 1005).await;
    confirm_food(&f).await;
    let (client, task) = f.bridge(RuntimeMode::Canary).await;
    freeze_food(&f, &client).await;
    generate_food(&f, &f.targets).await;
    let message = f
        .store
        .pending_feeding_overlay(&f.owner)
        .await
        .unwrap()
        .unwrap();
    assert!(
        f.store
            .pending_member_overlay(&f.owner)
            .await
            .unwrap()
            .is_none()
    );
    // An ordinary credit row, even with a plausible message and new identity,
    // cannot certify food or create an overlay notice.
    let mut forged = message.clone();
    forged.broadcast.proof.source_seq = 1;
    forged.broadcast.proof.identity = "66".repeat(32);
    forged.broadcast.request_id = Uuid::new_v4();
    tokio::time::sleep(std::time::Duration::from_millis(600)).await;
    assert!(client.feeding_overlay(&forged).await.is_err());
    f.ready(Some(true)).await; // Confirmed food stays announceable after a pause.
    tokio::time::sleep(std::time::Duration::from_millis(600)).await;
    let seq = client.feeding_overlay(&message).await.unwrap();
    assert_eq!(client.feeding_overlay(&message).await.unwrap(), seq); // lost ACK
    f.store
        .acknowledge_feeding_overlay(&message, seq)
        .await
        .unwrap();
    assert!(
        f.store
            .pending_feeding_overlay(&f.owner)
            .await
            .unwrap()
            .is_none()
    );
    let mut invalid = message.clone();
    invalid.broadcast.request_id = Uuid::new_v4();
    tokio::time::sleep(std::time::Duration::from_millis(600)).await;
    assert!(client.feeding_overlay(&invalid).await.is_err());
    let mut invalid = message.clone();
    invalid.broadcast.proof.owner = key(3);
    tokio::time::sleep(std::time::Duration::from_millis(600)).await;
    assert!(client.feeding_overlay(&invalid).await.is_err());
    let mut invalid = message.clone();
    invalid.epoch = Uuid::new_v4();
    tokio::time::sleep(std::time::Duration::from_millis(600)).await;
    assert!(client.feeding_overlay(&invalid).await.is_err());
    let mut invalid = message.clone();
    invalid.message = "replacement".into();
    tokio::time::sleep(std::time::Duration::from_millis(600)).await;
    assert!(client.feeding_overlay(&invalid).await.is_err());
    assert_eq!(f.core.feed_credit_sats().await.unwrap(), 5);
    let events = f.core.events_after(0, 100).await.unwrap();
    assert_eq!(
        events
            .iter()
            .filter(|e| e.event_type == "feeder_confirmed")
            .count(),
        1
    );
    assert_eq!(
        events
            .iter()
            .filter(|e| e.event_type == "cyberherd_message")
            .count(),
        1
    );
    assert!(f.core.next_outbox_entry().await.unwrap().is_none());
    task.abort();
    let (uid, gid) = self_credentials().unwrap();
    let role = BridgeConfig {
        socket: f.root.path().join("feeding-payout-role.sock"),
        peer_uid: uid + 1,
        group_gid: gid,
        reward_policy: Some(crate::reward_funding::RewardPolicy {
            owner: f.owner.clone(),
            member_percent_ceiling: 0,
            max_sats_per_feed: 0,
            max_sats_per_24h: 0,
        }),
        payout_peer_uid: Some(uid),
        payout_execution_enabled: false,
        delegated_feeder_messages: false,
        payout_account_id: None,
    };
    let task = tokio::spawn(
        Server::bind(&role)
            .await
            .unwrap()
            .run(f.core.clone(), RuntimeMode::Active),
    );
    let payout = Client::new(role.socket, uid).unwrap();
    assert!(payout.feeding_overlay(&message).await.is_err());
    task.abort();
}

#[tokio::test]
async fn feeding_notes_hold_in_nonpublic_mode_but_retry_same_bytes_during_pause_and_keep_stream_root()
 {
    let f = Fixture::new().await;
    f.core
        .configure_feeding_notices(Some(&f.owner))
        .await
        .unwrap();
    feed_credit(&f, "one-payment", 1000).await;
    confirm_food(&f).await;
    let (client, task) = f.bridge(RuntimeMode::Canary).await;
    freeze_food(&f, &client).await;
    generate_food(&f, &f.targets).await;
    let fake = Fake::default();
    let worker = Worker {
        store: f.store.clone(),
        core: client,
        transport: fake.clone(),
        owner: f.owner.clone(),
    };
    assert_eq!(tick(&f, &worker).await.unwrap(), Step::Waiting);
    assert_eq!(fake.signs.load(Ordering::SeqCst), 0);
    task.abort();
    // Reopen the same durable store with a public-mode core, not a fresh message.
    let reopened = Store::connect(&f.url).await.unwrap();
    let db = SqlitePool::connect(&f.url).await.unwrap();
    sqlx::query("UPDATE delivery_work SET lease_until=0")
        .execute(&db)
        .await
        .unwrap();
    let (client, task) = f.bridge(RuntimeMode::Active).await;
    let worker = Worker {
        store: reopened,
        core: client,
        transport: fake.clone(),
        owner: f.owner.clone(),
    };
    f.ready(Some(true)).await;
    fake.lose.store(true, Ordering::SeqCst);
    tokio::time::sleep(std::time::Duration::from_millis(1050)).await;
    assert!(worker.tick().await.is_err());
    let original = fake.events.lock().unwrap()[0].clone();
    sqlx::query("UPDATE delivery_work SET lease_until=0")
        .execute(&db)
        .await
        .unwrap();
    // Do not let another destination hide the retry assertion.
    sqlx::query("UPDATE delivery_work SET attempted_at=0 WHERE destination=?")
        .bind(if original.kind == 1 {
            "broadcast:note".into()
        } else {
            format!(
                "stream:{}",
                f.targets
                    .current()
                    .into_iter()
                    .find(|t| t.kind == 30311)
                    .unwrap()
                    .coordinate
                    .unwrap()
            )
        })
        .execute(&db)
        .await
        .unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(1050)).await;
    assert_eq!(worker.tick().await.unwrap(), Step::Published);
    let events = fake.events.lock().unwrap().clone();
    assert_eq!(events[0], events[1]);
    assert_eq!(fake.signs.load(Ordering::SeqCst), 1);
    task.abort();
}

#[tokio::test]
async fn feeding_delegation_is_opt_in_and_legacy_core_announcement_remains_unchanged() {
    let f = Fixture::new().await;
    feed_credit(&f, "ordinary", 1000).await;
    let id = confirm_food(&f).await;
    let (client, task) = f.bridge(RuntimeMode::Active).await;
    freeze_food(&f, &client).await;
    assert!(
        f.store
            .feeding_barrier(&f.owner, id)
            .await
            .unwrap()
            .unwrap()
            .feeding
            .presentation
            .is_none()
    );
    generate_food(&f, &f.targets).await;
    assert!(
        f.store
            .pending_feeding_overlay(&f.owner)
            .await
            .unwrap()
            .is_none()
    );
    let source = f
        .core
        .events_after(0, 100)
        .await
        .unwrap()
        .into_iter()
        .find(|e| e.event_type == "feeder_confirmed")
        .unwrap();
    let message = crate::presentation::MessageRenderer::embedded()
        .unwrap()
        .render(&source, 1000)
        .unwrap();
    assert!(message.nostr_content.is_some());
    assert!(message.overlay_message.is_some());
    let db = SqlitePool::connect(&f.url).await.unwrap();
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM delivery_messages")
        .fetch_one(&db)
        .await
        .unwrap();
    assert_eq!(count, 0);
    task.abort();
}

#[tokio::test]
async fn feeding_template_opt_out_is_durable_for_a_payment_and_missing_stream_does_not_invent_a_chat()
 {
    let f = Fixture::new().await;
    f.core
        .configure_feeding_notices(Some(&f.owner))
        .await
        .unwrap();
    f.store
        .edit_template(
            &f.owner,
            "disable-feeding",
            &crate::cyberherd::store::TemplateEdit::DisableCategory {
                category: "feeder_trigger".into(),
            },
        )
        .await
        .unwrap();
    feed_credit(&f, "batch", 2345).await;
    confirm_food(&f).await;
    let (client, task) = f.bridge(RuntimeMode::Canary).await;
    freeze_food(&f, &client).await;
    generate_food(&f, &f.targets).await;
    f.store
        .edit_template(
            &f.owner,
            "enable-feeding",
            &crate::cyberherd::store::TemplateEdit::EnableCategory {
                category: "feeder_trigger".into(),
            },
        )
        .await
        .unwrap();
    confirm_food(&f).await;
    freeze_food(&f, &client).await;
    generate_food(&f, &f.targets).await;
    assert!(
        f.store
            .pending_feeding_overlay(&f.owner)
            .await
            .unwrap()
            .is_none()
    );
    feed_credit(&f, "different-payment", 1000).await;
    confirm_food(&f).await;
    tokio::time::sleep(std::time::Duration::from_millis(600)).await;
    freeze_food(&f, &client).await;
    let no_stream = Targets::project(
        TargetPolicy {
            project_pubkey: f.owner.clone(),
            tracked_tags: vec!["CyberHerd".into()],
        },
        &DayWindow::at("America/Denver", calendar::now().unwrap()).unwrap(),
        vec![],
        vec![],
        BTreeSet::new(),
        calendar::now().unwrap(),
    )
    .unwrap();
    generate_food(&f, &no_stream).await;
    let db = SqlitePool::connect(&f.url).await.unwrap();
    let destinations: Vec<String> =
        sqlx::query_scalar("SELECT destination FROM delivery_destinations")
            .fetch_all(&db)
            .await
            .unwrap();
    assert_eq!(destinations, vec!["broadcast:note"]);
    assert!(
        f.store
            .pending_feeding_overlay(&f.owner)
            .await
            .unwrap()
            .is_some()
    );
    task.abort();
}

#[tokio::test]
async fn funded_or_uncertain_food_does_not_create_feeding_notice_or_debit() {
    let f = Fixture::new().await;
    f.core
        .configure_feeding_notices(Some(&f.owner))
        .await
        .unwrap();
    feed_credit(&f, "funded", 2345).await;
    let id = f.core.begin_feed_attempt(1000).await.unwrap().unwrap();
    f.core
        .mark_feed_unknown(id, "synthetic uncertain owner result")
        .await
        .unwrap();
    let (client, task) = f.bridge(RuntimeMode::Canary).await;
    freeze_food(&f, &client).await;
    generate_food(&f, &f.targets).await;
    assert!(
        f.store
            .pending_feeding_overlay(&f.owner)
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(f.core.feed_credit_sats().await.unwrap(), 2345);
    assert_eq!(
        f.core.unresolved_feed_attempt().await.unwrap().unwrap().id,
        id
    );
    assert!(f.core.begin_feed_attempt(1000).await.is_err());
    task.abort();
}

#[tokio::test]
async fn feeding_missing_bounded_attribution_uses_editable_neutral_wording_without_threshold_claim()
{
    let f = Fixture::new().await;
    f.core
        .configure_feeding_notices(Some(&f.owner))
        .await
        .unwrap();
    feed_credit(&f, "saved", 1000).await;
    let core = SqlitePool::connect(&f.core_url).await.unwrap();
    sqlx::query("WITH RECURSIVE n(x) AS (SELECT 1 UNION ALL SELECT x+1 FROM n WHERE x<4097) INSERT INTO ledger_entries(entry_type,source_key,delta_sats) SELECT 'HERD_RECEIPT','synthetic-later:'||x,1 FROM n").execute(&core).await.unwrap();
    let id = confirm_food(&f).await;
    assert_eq!(f.core.feed_credit_sats().await.unwrap(), 4097);
    let (client, task) = f.bridge(RuntimeMode::Canary).await;
    freeze_food(&f, &client).await;
    let barrier = f
        .store
        .feeding_barrier(&f.owner, id)
        .await
        .unwrap()
        .unwrap();
    assert!(
        barrier
            .feeding
            .presentation
            .as_ref()
            .unwrap()
            .triggering_payment_amount_sats
            .is_none()
    );
    f.store
        .edit_template(
            &f.owner,
            "custom-neutral",
            &crate::cyberherd::store::TemplateEdit::Set {
                category: "feeder_confirmed_unattributed".into(),
                variant: "0".into(),
                text: "Confirmed food, with attribution unavailable. {difference_message}".into(),
            },
        )
        .await
        .unwrap();
    generate_food(&f, &f.targets).await;
    let message = f
        .store
        .pending_feeding_overlay(&f.owner)
        .await
        .unwrap()
        .unwrap();
    assert!(
        message
            .message
            .starts_with("Confirmed food, with attribution unavailable.")
    );
    assert!(!message.message.contains("1000 sats"));
    assert!(!message.message.contains("0 sats"));
    task.abort();
}

async fn hint_catalog(f: &Fixture) {
    let catalog = f.store.templates(&f.owner).await.unwrap();
    let keys = catalog
        .category("repost_hint")
        .unwrap()
        .keys()
        .cloned()
        .collect::<Vec<_>>();
    for (i, key) in keys.into_iter().enumerate() {
        f.store
            .edit_template(
                &f.owner,
                &format!("delete-hint-{i}"),
                &crate::cyberherd::store::TemplateEdit::Delete {
                    category: "repost_hint".into(),
                    variant: key,
                },
            )
            .await
            .unwrap();
    }
    f.store
        .edit_template(
            &f.owner,
            "custom-hint",
            &crate::cyberherd::store::TemplateEdit::Set {
                category: "repost_hint".into(),
                variant: "custom".into(),
                text: "HINT_ONLY {name}: a first repost can help next time.".into(),
            },
        )
        .await
        .unwrap();
}
async fn no_treat(f: &Fixture, start: u64, frozen_repost: bool) -> i64 {
    let cycle = f.store.snapshot(&f.owner).await.unwrap().cycle;
    let source = (start..start + 1000)
        .map(|i| format!("payout:hint:{i}"))
        .find(|source| {
            let id = crate::cyberherd::conversation::identity(&f.owner, &key(2), source).unwrap();
            u8::from_str_radix(&id[..2], 16).unwrap() % 3 == 0
        })
        .unwrap();
    let row = crate::cyberherd::rewards::Row {
        pubkey: key(2),
        profile_event_id: None,
        profile: Some(Profile {
            name: "Alice".into(),
            picture: None,
            lud16: Some("alice@example.com".into()),
        }),
        amount_sats: 34,
        awards: crate::cyberherd::shares::SocialAwards {
            repost: frozen_repost,
            like: false,
        },
        point_units: "340".into(),
        share_micro_percent: 10_000_000,
        gross_sats: 0,
        fee_limit_sats: 1,
        net_sats: 0,
        gross_micro_percent_of_feed: 0,
        recipient: None,
        skip: Some(crate::cyberherd::rewards::Skip::RecipientMinimum),
        conversation: None,
        score_basis: Some(10),
        zap_content: None,
        zap_wording: None,
    };
    let db = SqlitePool::connect(&f.url).await.unwrap();
    sqlx::query("INSERT INTO domain_events(owner,cycle,source_id,event_type,snapshot_json) VALUES(?,?,?,'payout_skipped',?)")
        .bind(&f.owner).bind(cycle).bind(source).bind(serde_json::json!({"result":null,"breakdown":row}).to_string()).execute(&db).await.unwrap().last_insert_rowid()
}
#[tokio::test]
async fn no_treat_hint_is_once_per_cycle_exact_after_reopen_and_uses_human_overlay_names() {
    let f = Fixture::new().await;
    f.apply("join-hints", 34).await;
    hint_catalog(&f).await;
    let first = no_treat(&f, 0, false).await;
    f.queue().await;
    let db = SqlitePool::connect(&f.url).await.unwrap();
    sqlx::query("INSERT INTO domain_events(owner,cycle,source_id,event_type,snapshot_json) SELECT owner,cycle,'msg:msg:'||source_id,event_type,snapshot_json FROM domain_events WHERE seq=?")
        .bind(first).execute(&db).await.unwrap();
    let reopened = Store::connect(&f.url).await.unwrap();
    no_treat(&f, 1000, false).await;
    while reopened
        .queue_next_delivery(
            &f.owner,
            &["wss://relay.damus.io/".into()],
            calendar::now().unwrap(),
        )
        .await
        .unwrap()
    {}
    let db = SqlitePool::connect(&f.url).await.unwrap();
    let messages:Vec<String>=sqlx::query_scalar("SELECT document_json FROM delivery_messages WHERE json_extract(document_json,'$.category')='cyber_herd_no_treats'").fetch_all(&db).await.unwrap();
    assert_eq!(messages.len(), 2);
    assert_eq!(
        messages.iter().filter(|m| m.contains("HINT_ONLY")).count(),
        1
    );
    let hinted: crate::cyberherd::delivery::Rendered =
        serde_json::from_str(messages.iter().find(|m| m.contains("HINT_ONLY")).unwrap()).unwrap();
    assert!(hinted.nostr.contains("HINT_ONLY nostr:npub"));
    assert!(hinted.overlay.contains("HINT_ONLY Alice"));
    assert!(
        sqlx::query("DELETE FROM delivery_repost_hints")
            .execute(&db)
            .await
            .is_err()
    );
    f.store
        .reset(&f.owner, "next-hint-cycle", calendar::now().unwrap())
        .await
        .unwrap();
    f.apply("join-next-cycle", 34).await;
    no_treat(&f, 2000, false).await;
    f.queue().await;
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM delivery_repost_hints")
        .fetch_one(&db)
        .await
        .unwrap();
    assert_eq!(count, 2);
}
#[tokio::test]
async fn no_treat_hint_checks_later_repost_and_preserves_template_opt_out_without_claiming_slot() {
    let f = Fixture::new().await;
    f.apply("join", 34).await;
    hint_catalog(&f).await;
    f.store
        .edit_template(
            &f.owner,
            "no-hint",
            &crate::cyberherd::store::TemplateEdit::DisableCategory {
                category: "repost_hint".into(),
            },
        )
        .await
        .unwrap();
    no_treat(&f, 0, false).await;
    f.queue().await;
    let db = SqlitePool::connect(&f.url).await.unwrap();
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM delivery_repost_hints")
            .fetch_one(&db)
            .await
            .unwrap(),
        0
    );
    f.store
        .edit_template(
            &f.owner,
            "allow-hint",
            &crate::cyberherd::store::TemplateEdit::EnableCategory {
                category: "repost_hint".into(),
            },
        )
        .await
        .unwrap();
    no_treat(&f, 1000, false).await;
    // A repost earned after the payout snapshot still suppresses the hint.
    f.store
        .process(
            &f.owner,
            &Interaction {
                source_id: format!("nostr:{}", "55".repeat(32)),
                source_hash: "66".repeat(32),
                pubkey: key(2),
                profile: Profile {
                    name: "Alice".into(),
                    picture: None,
                    lud16: Some("alice@example.com".into()),
                },
                action: Action::Repost,
                occurred_at: calendar::now().unwrap(),
            },
            Target::Current,
            FeederAvailability::Ready,
        )
        .await
        .unwrap();
    f.queue().await;
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM delivery_repost_hints")
            .fetch_one(&db)
            .await
            .unwrap(),
        0
    );
}
#[tokio::test]
async fn no_treat_hint_claim_and_cursor_roll_back_when_message_insert_fails() {
    let f = Fixture::new().await;
    f.apply("join", 34).await;
    hint_catalog(&f).await;
    f.queue().await;
    let event = no_treat(&f, 0, false).await;
    let db = SqlitePool::connect(&f.url).await.unwrap();
    sqlx::query("CREATE TRIGGER synthetic_delivery_failure BEFORE INSERT ON delivery_messages BEGIN SELECT RAISE(ABORT,'fixture failure'); END").execute(&db).await.unwrap();
    assert!(
        f.store
            .queue_next_delivery(
                &f.owner,
                &["wss://relay.damus.io/".into()],
                calendar::now().unwrap()
            )
            .await
            .is_err()
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM delivery_repost_hints")
            .fetch_one(&db)
            .await
            .unwrap(),
        0
    );
    assert!(
        sqlx::query_scalar::<_, i64>("SELECT through FROM delivery_cursors")
            .fetch_one(&db)
            .await
            .unwrap()
            < event
    );
    sqlx::query("DROP TRIGGER synthetic_delivery_failure")
        .execute(&db)
        .await
        .unwrap();
    f.queue().await;
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM delivery_repost_hints")
            .fetch_one(&db)
            .await
            .unwrap(),
        1
    );
}

#[tokio::test]
async fn public_roster_uses_exact_scores_large_sats_and_read_only_projection() {
    let f = Fixture::new().await;
    f.apply("large", 9_007_199_254_740_993).await;
    let before = f.store.snapshot(&f.owner).await.unwrap();
    let roster = f.store.public_roster(&f.owner).await.unwrap();
    assert_eq!(roster.members[0].amount_sats, "9007199254740993");
    assert_eq!(roster.members[0].points, "9007199254740993");
    assert_eq!(roster.members[0].share_percent, "10");
    assert_eq!(f.store.snapshot(&f.owner).await.unwrap(), before);
    let mut malformed = roster.cards();
    malformed.members[0].points = "9007199254740992".into();
    assert!(malformed.validate(true).is_err());
    let mut malformed = roster.cards();
    malformed.members[0].picture = Some("https://10.8.0.6/private".into());
    assert!(malformed.validate(true).is_err());
}
#[tokio::test]
async fn roster_overlay_is_monotone_exact_restart_safe_and_hydrates_snapshot_without_food_or_nostr()
{
    let f = Fixture::new().await;
    f.apply("join-roster", 34).await;
    let epoch = f.core.overlay_stream_id().await.unwrap();
    let first = crate::cyberherd::view::Overlay {
        epoch,
        roster: f.store.public_roster(&f.owner).await.unwrap().cards(),
    };
    let (client, task) = f.bridge(RuntimeMode::Canary).await;
    f.ready(Some(true)).await;
    let seq = client.roster_overlay(&first).await.unwrap();
    assert_eq!(client.roster_overlay(&first).await.unwrap(), seq);
    f.store.acknowledge_roster(&first, seq).await.unwrap();
    assert!(
        !Store::connect(&f.url)
            .await
            .unwrap()
            .roster_needs_delivery(&first)
            .await
            .unwrap()
    );
    f.apply("increase-roster", 34).await;
    let second = crate::cyberherd::view::Overlay {
        epoch,
        roster: f.store.public_roster(&f.owner).await.unwrap().cards(),
    };
    tokio::time::sleep(std::time::Duration::from_millis(600)).await;
    let second_seq = client.roster_overlay(&second).await.unwrap();
    assert!(second_seq > seq);
    let mut changed = first.clone();
    changed.roster.members[0].display_name = "stale replacement".into();
    tokio::time::sleep(std::time::Duration::from_millis(600)).await;
    assert!(client.roster_overlay(&changed).await.is_err());
    let mut other = second.clone();
    other.epoch = Uuid::new_v4();
    tokio::time::sleep(std::time::Duration::from_millis(600)).await;
    assert!(client.roster_overlay(&other).await.is_err());
    let mut other = second.clone();
    other.roster.owner = key(3);
    tokio::time::sleep(std::time::Duration::from_millis(600)).await;
    assert!(client.roster_overlay(&other).await.is_err());
    // Exact old ACKs are harmless; they cannot replace the current snapshot.
    tokio::time::sleep(std::time::Duration::from_millis(600)).await;
    assert_eq!(client.roster_overlay(&first).await.unwrap(), seq);
    let (_, raw) = f.core.overlay_snapshot_message(1000).await.unwrap();
    let state: serde_json::Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(state["cyberherd"]["members"][0]["amount_sats"], "68");
    f.store
        .reset(&f.owner, "empty-roster", calendar::now().unwrap())
        .await
        .unwrap();
    let empty = crate::cyberherd::view::Overlay {
        epoch,
        roster: f.store.public_roster(&f.owner).await.unwrap().cards(),
    };
    tokio::time::sleep(std::time::Duration::from_millis(600)).await;
    client.roster_overlay(&empty).await.unwrap();
    let reopened = LedgerStore::connect(&f.core_url).await.unwrap();
    let (_, raw) = reopened.overlay_snapshot_message(1000).await.unwrap();
    let state: serde_json::Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(state["cyberherd"]["members"], serde_json::json!([]));
    assert_eq!(f.core.feed_credit_sats().await.unwrap(), 0);
    assert!(f.core.next_outbox_entry().await.unwrap().is_none());
    assert!(
        f.core
            .core_event_batch(Some(epoch), 0, 128)
            .await
            .unwrap()
            .events
            .iter()
            .all(|e| matches!(e.body, crate::core_bridge::EventBody::Other))
    );
    task.abort();
    let (payout, payout_task) = f.bridge_role(RuntimeMode::Canary, true).await;
    let before = reopened.overlay_snapshot_message(1000).await.unwrap();
    assert!(payout.roster_overlay(&empty).await.is_err());
    assert_eq!(
        reopened.overlay_snapshot_message(1000).await.unwrap(),
        before
    );
    payout_task.abort();
}

#[tokio::test]
async fn public_targets_distinguish_unknown_from_fresh_empty_and_do_not_reuse_failed_discovery() {
    let f = Fixture::new().await;
    let policy = TargetPolicy {
        project_pubkey: f.owner.clone(),
        tracked_tags: vec!["CyberHerd".into()],
    };
    let now = calendar::now().unwrap();
    assert!(
        f.store
            .public_targets(&policy, "America/Denver", now)
            .await
            .unwrap()
            .is_none()
    );
    f.store
        .observe_view_discovery(&f.owner, true, now)
        .await
        .unwrap();
    assert_eq!(
        f.store
            .public_targets(&policy, "America/Denver", now)
            .await
            .unwrap(),
        Some(vec![])
    );
    assert!(
        f.store
            .public_targets(&policy, "America/Denver", now + 61)
            .await
            .unwrap()
            .is_none()
    );
    f.store
        .observe_view_discovery(&f.owner, false, now + 1)
        .await
        .unwrap();
    f.store
        .observe_view_discovery(&f.owner, true, now)
        .await
        .unwrap();
    assert!(
        f.store
            .public_targets(&policy, "America/Denver", now + 1)
            .await
            .unwrap()
            .is_none()
    );
}
