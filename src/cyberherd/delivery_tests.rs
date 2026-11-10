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
            peer_uid: uid,
            group_gid: gid,
            reward_policy: Some(crate::reward_funding::RewardPolicy {
                owner: self.owner.clone(),
                member_percent_ceiling: 0,
                max_sats_per_feed: 0,
                max_sats_per_24h: 0,
            }),
            payout_peer_uid: Some(uid + 1),
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
        assert_eq!(worker.tick().await.unwrap(), Step::Waiting);
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
    assert_eq!(worker.tick().await.unwrap(), Step::Waiting);
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
    // Saving a signature did not create either a thread anchor or a treat post.
    assert!(
        f.store
            .published_joining(&f.owner, &conversation, 1, &key(2))
            .await
            .unwrap()
            .is_none()
    );
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
