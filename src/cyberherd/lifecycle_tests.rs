use super::*;
use crate::{
    core_bridge::LifecycleContext,
    cyberherd::{
        calendar,
        delivery_worker::RelayAck,
        membership::Settings,
        store::{Store, TemplateEdit},
        targets::{ProviderCache, StreamAuthentication},
        templates::Catalog,
    },
    nostr_event::{VerifiedEvent, test_event},
};
use sqlx::{Row as _, SqlitePool};

struct Fixture {
    _root: tempfile::TempDir,
    url: String,
    db: SqlitePool,
    store: Store,
    owner: String,
    relays: Vec<String>,
    now: i64,
}
impl Fixture {
    async fn new() -> Self {
        let root = tempfile::TempDir::new().unwrap();
        let url = format!("sqlite://{}", root.path().join("members.db").display());
        let store = Store::connect(&url).await.unwrap();
        let owner = test_event(1, 0, 100, "", vec![]).pubkey;
        store
            .initialize(
                &owner,
                &Settings {
                    minimum_sats: 10,
                    max_members: 3,
                    member_percent: 10,
                },
                100,
            )
            .await
            .unwrap();
        for catalog in [
            Catalog::legacy().unwrap(),
            Catalog::lifecycle_defaults().unwrap(),
        ] {
            store.seed_templates(&owner, &catalog).await.unwrap();
        }
        Self {
            _root: root,
            db: SqlitePool::connect(&url).await.unwrap(),
            url,
            store,
            owner,
            relays: vec!["wss://relay.damus.io/".into(), "wss://nos.lol/".into()],
            now: calendar::now().unwrap(),
        }
    }
    async fn stream(&self, status: &str, slug: &str, at: i64) {
        let event = VerifiedEvent::verify(test_event(
            1,
            30311,
            at as u64,
            "",
            vec![
                vec!["d".into(), slug.into()],
                vec!["status".into(), status.into()],
            ],
        ))
        .unwrap();
        let StreamAuthentication::Accepted(stream) = ProviderCache::default()
            .authenticate(event, &self.owner, at)
            .await
            .unwrap()
        else {
            panic!("project stream must authenticate")
        };
        self.store
            .remember_stream(&self.owner, &stream)
            .await
            .unwrap();
    }
    fn state(paused: Option<bool>, at: i64) -> LifecycleContext {
        LifecycleContext {
            override_enabled: paused,
            observed_at: Some(at),
            nostr_enabled: true,
            solar: None,
        }
    }
    async fn observe(&self, paused: Option<bool>, at: i64) {
        self.store
            .observe_lifecycle(
                &self.owner,
                Observation {
                    state: &Self::state(paused, at),
                    discovery_available: true,
                    timezone: "America/Denver",
                    relays: &self.relays,
                    now: at,
                },
            )
            .await
            .unwrap();
    }
    async fn count(&self) -> i64 {
        sqlx::query_scalar("SELECT COUNT(*) FROM notice_events")
            .fetch_one(&self.db)
            .await
            .unwrap()
    }
}

#[tokio::test]
async fn lifecycle_initial_off_is_quiet_unknown_never_resumes_and_restart_keeps_order() {
    let f = Fixture::new().await;
    f.observe(Some(false), f.now - 3).await;
    assert_eq!(f.count().await, 0);
    f.observe(Some(true), f.now - 2).await;
    f.observe(None, f.now - 1).await;
    assert_eq!(f.count().await, 1);
    let before: i64 = sqlx::query_scalar("SELECT observed_at FROM lifecycle_observer")
        .fetch_one(&f.db)
        .await
        .unwrap();
    assert_eq!(before, f.now - 2);
    f.observe(Some(false), f.now).await;
    let reopened = Store::connect(&f.url).await.unwrap();
    reopened
        .observe_lifecycle(
            &f.owner,
            Observation {
                state: &Fixture::state(Some(false), f.now),
                discovery_available: true,
                timezone: "America/Denver",
                relays: &f.relays,
                now: f.now,
            },
        )
        .await
        .unwrap();
    assert_eq!(f.count().await, 2);
    let first = reopened
        .reserve_notice(&f.owner, f.now)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(first.purpose, "pause");
    assert_eq!(first.unsigned.kind, 1);
    assert!(
        !first
            .unsigned
            .tags
            .iter()
            .any(|t| t[0] == "e" || t[0] == "p")
    );
    let signed = VerifiedEvent::verify(test_event(
        1,
        1,
        first.unsigned.created_at,
        &first.unsigned.content,
        first.unsigned.tags.clone(),
    ))
    .unwrap();
    let raw = reopened
        .save_notice_signature(&first, &signed)
        .await
        .unwrap();
    // A lost ACK retains exact bytes and gates the later transition on this relay.
    reopened.finish_notice(&first, None, f.now).await.unwrap();
    assert!(
        reopened
            .reserve_notice(&f.owner, f.now)
            .await
            .unwrap()
            .is_none()
    );
    let retry = reopened
        .reserve_notice(&f.owner, f.now + 6)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(retry.identity, first.identity);
    assert_eq!(retry.signed.as_deref(), Some(raw.as_str()));
    let ack = RelayAck::synthetic(&signed, &retry.relay);
    reopened
        .finish_notice(&retry, Some(&ack), f.now + 6)
        .await
        .unwrap();
    // The other relay is independently ordered and sees pause before resume.
    let next = reopened
        .reserve_notice(&f.owner, f.now + 7)
        .await
        .unwrap()
        .unwrap();
    if next.purpose == "pause" {
        assert_ne!(next.relay, retry.relay);
        reopened
            .finish_notice(&next, None, f.now + 7)
            .await
            .unwrap();
    }
    let resume = if next.purpose == "resume" {
        next
    } else {
        reopened
            .reserve_notice(&f.owner, f.now + 8)
            .await
            .unwrap()
            .unwrap()
    };
    assert_eq!(resume.purpose, "resume");
    assert_eq!(resume.relay, retry.relay);
    assert!(
        sqlx::query("UPDATE notice_events SET purpose='invitation'")
            .execute(&f.db)
            .await
            .is_err()
    );
    assert!(
        sqlx::query(
            "UPDATE notice_destinations SET signed_json='changed' WHERE signed_json IS NOT NULL"
        )
        .execute(&f.db)
        .await
        .is_err()
    );
}

#[tokio::test]
async fn lifecycle_initial_on_announces_once_even_when_daylight_changes_and_category_deletion_survives()
 {
    let f = Fixture::new().await;
    f.observe(Some(true), f.now).await;
    assert_eq!(f.count().await, 1);
    for i in 1..5 {
        f.observe(Some(true), f.now + i).await;
    }
    assert_eq!(f.count().await, 1);
    for (n, category) in [
        "feeder_requests_paused",
        "feeder_requests_paused_night",
        "feeder_requests_enabled",
    ]
    .into_iter()
    .enumerate()
    {
        f.store
            .edit_template(
                &f.owner,
                &format!("disable-{n}"),
                &TemplateEdit::DisableCategory {
                    category: category.into(),
                },
            )
            .await
            .unwrap();
    }
    f.observe(Some(false), f.now + 5).await;
    f.observe(Some(true), f.now + 6).await;
    f.store
        .seed_templates(&f.owner, &Catalog::lifecycle_defaults().unwrap())
        .await
        .unwrap();
    assert_eq!(f.count().await, 1);
    assert!(
        f.store
            .templates(&f.owner)
            .await
            .unwrap()
            .category("feeder_requests_enabled")
            .is_none()
    );
}

#[tokio::test]
async fn lifecycle_invitations_are_stream_only_capacity_frozen_cooldown_and_pause_cancels_pending()
{
    let f = Fixture::new().await;
    f.stream("live", "one", f.now - 3).await;
    f.observe(Some(false), f.now - 2).await;
    assert_eq!(f.count().await, 1);
    let job = f
        .store
        .reserve_notice(&f.owner, f.now - 2)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(job.purpose, "invitation");
    assert_eq!(job.unsigned.kind, 1311);
    assert!(job.unsigned.content.contains("0/3"));
    assert!(
        job.unsigned
            .tags
            .iter()
            .any(|t| t[0] == "a" && t[1].ends_with(":one") && t[3] == "root")
    );
    assert!(
        f.store
            .notice_invitation_live(&job, f.now - 1)
            .await
            .unwrap()
    );
    f.observe(Some(false), f.now - 1).await;
    assert_eq!(f.count().await, 1);
    f.observe(Some(true), f.now).await;
    let canceled: bool =
        sqlx::query_scalar("SELECT canceled FROM notice_destinations WHERE identity=?")
            .bind(&job.identity)
            .fetch_one(&f.db)
            .await
            .unwrap();
    assert!(canceled);
    assert!(!f.store.notice_invitation_live(&job, f.now).await.unwrap());
    let pause = f
        .store
        .reserve_notice(&f.owner, f.now)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(pause.purpose, "pause");
    // Frozen stream is preserved after a new broadcast appears.
    f.stream("live", "two", f.now + 1).await;
    let rows = sqlx::query("SELECT unsigned_json FROM notice_destinations WHERE identity=?")
        .bind(&pause.identity)
        .fetch_all(&f.db)
        .await
        .unwrap();
    assert_eq!(rows.len(), 2);
    assert!(
        rows.iter()
            .any(|r| r.get::<String, _>("unsigned_json").contains(":one"))
    );
}

#[tokio::test]
async fn lifecycle_stream_invitation_requires_current_authenticated_head_and_template_opt_in() {
    let f = Fixture::new().await;
    f.stream("live", "one", f.now - 3).await;
    f.observe(Some(false), f.now - 2).await;
    let job = f
        .store
        .reserve_notice(&f.owner, f.now - 2)
        .await
        .unwrap()
        .unwrap();
    f.stream("ended", "one", f.now - 1).await;
    f.stream("live", "one", f.now - 4).await; // Cannot resurrect an ended head.
    assert!(!f.store.notice_invitation_live(&job, f.now).await.unwrap());
    assert!(
        f.store
            .reserve_notice(&f.owner, f.now + 7200)
            .await
            .unwrap()
            .is_none()
    );
    f.store
        .edit_template(
            &f.owner,
            "no-invites",
            &TemplateEdit::DisableCategory {
                category: "cyber_herd_invitation".into(),
            },
        )
        .await
        .unwrap();
    f.stream("live", "two", f.now + 7201).await;
    f.observe(Some(false), f.now + 7201).await;
    assert_eq!(f.count().await, 1);
}

#[tokio::test]
async fn lifecycle_evening_can_freeze_recent_ended_stream_without_restoring_it_to_live() {
    let f = Fixture::new().await;
    let zone: chrono_tz::Tz = "America/Denver".parse().unwrap();
    use chrono::TimeZone;
    let at = zone
        .with_ymd_and_hms(2026, 11, 10, 19, 0, 0)
        .unwrap()
        .timestamp();
    f.stream("ended", "evening", at - 300).await;
    f.observe(Some(true), at).await;
    assert_eq!(f.count().await, 1);
    let destinations: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM notice_destinations")
        .fetch_one(&f.db)
        .await
        .unwrap();
    assert_eq!(destinations, 2);
    let context: String = sqlx::query_scalar("SELECT context_json FROM notice_events")
        .fetch_one(&f.db)
        .await
        .unwrap();
    assert!(context.contains(":evening"));
    let next = Fixture::new().await;
    next.stream("ended", "old", at - 901).await;
    next.observe(Some(true), at).await;
    let destinations: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM notice_destinations")
        .fetch_one(&next.db)
        .await
        .unwrap();
    assert_eq!(destinations, 1);
}

#[test]
fn lifecycle_evening_solar_validation_and_mountain_fallback() {
    use chrono::TimeZone;
    let zone: chrono_tz::Tz = "America/Denver".parse().unwrap();
    let at = |hour| {
        zone.with_ymd_and_hms(2026, 11, 10, hour, 0, 0)
            .unwrap()
            .timestamp()
    };
    assert!(!evening("America/Denver", None, at(17)).unwrap());
    assert!(evening("America/Denver", None, at(18)).unwrap());
    let solar = |hour, phase, sunset, r| SolarContext {
        observed_at: at(hour),
        phase,
        sunset_at: sunset,
        direct_milli_w_m2: r,
    };
    assert!(
        !evening(
            "America/Denver",
            Some(&solar(19, Some(SunPhase::Daylight), None, None)),
            at(19)
        )
        .unwrap()
    );
    assert!(
        !evening(
            "America/Denver",
            Some(&solar(19, None, None, Some(1))),
            at(19)
        )
        .unwrap()
    );
    assert!(
        evening(
            "America/Denver",
            Some(&solar(17, None, Some(at(18)), Some(500))),
            at(17)
        )
        .unwrap()
    );
    assert!(
        !evening(
            "America/Denver",
            Some(&solar(16, None, Some(at(18)), Some(0))),
            at(16)
        )
        .unwrap()
    );
    assert!(
        evening(
            "America/Denver",
            Some(&solar(17, Some(SunPhase::Dusk), None, None)),
            at(17)
        )
        .unwrap()
    );
    assert!(
        !evening(
            "America/Denver",
            Some(&solar(9, Some(SunPhase::Night), None, Some(0))),
            at(9)
        )
        .unwrap()
    );
    assert!(
        evening(
            "America/Denver",
            Some(&solar(17, Some(SunPhase::Daylight), None, None)),
            at(19)
        )
        .unwrap()
    ); // Stale solar cannot override fallback.
    let parsed = SolarContext::parse(Some("NULL"), Some("UNDEF"), Some("NaN W/m²"), at(17));
    assert_eq!(parsed.phase, None);
    assert_eq!(parsed.sunset_at, None);
    assert_eq!(parsed.direct_milli_w_m2, None);
    assert_eq!(
        SolarContext::parse(Some("DAYLIGHT"), None, Some("0.5 W/m²"), at(17)).direct_milli_w_m2,
        Some(500)
    );
}

#[derive(Clone, Default)]
struct FakeTransport {
    signs: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    lose: std::sync::Arc<std::sync::atomic::AtomicBool>,
    ids: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
}
impl crate::cyberherd::delivery_worker::Transport for FakeTransport {
    async fn sign(
        &self,
        unsigned: &crate::cyberherd::store::delivery_state::Unsigned,
    ) -> anyhow::Result<VerifiedEvent> {
        self.signs.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        VerifiedEvent::verify(test_event(
            1,
            unsigned.kind,
            unsigned.created_at,
            &unsigned.content,
            unsigned.tags.clone(),
        ))
    }
    async fn publish(&self, event: &VerifiedEvent, relay: &str) -> anyhow::Result<RelayAck> {
        self.ids.lock().unwrap().push(event.event().id.clone());
        if self.lose.swap(false, std::sync::atomic::Ordering::SeqCst) {
            anyhow::bail!("synthetic lost ACK");
        }
        Ok(RelayAck::synthetic(event, relay))
    }
}
#[tokio::test]
async fn lifecycle_publisher_waits_on_unknown_but_sends_pause_while_override_on_and_reuses_signature()
 {
    use crate::{
        config::RuntimeMode,
        core_bridge::{
            BridgeConfig,
            unix::{Client, Server, self_credentials},
        },
        gateway::FeederSafety,
        ledger::LedgerStore,
    };
    use std::sync::atomic::Ordering;
    let f = Fixture::new().await;
    f.observe(Some(true), f.now - 3).await;
    let core = LedgerStore::connect(&format!(
        "sqlite://{}",
        f._root.path().join("core.db").display()
    ))
    .await
    .unwrap();
    let (uid, gid) = self_credentials().unwrap();
    std::fs::set_permissions(
        f._root.path(),
        std::os::unix::fs::PermissionsExt::from_mode(0o750),
    )
    .unwrap();
    let config = BridgeConfig {
        socket: f._root.path().join("c.sock"),
        peer_uid: uid,
        group_gid: gid,
        reward_policy: None,
        payout_peer_uid: None,
        delegated_feeder_messages: false,
        payout_execution_enabled: false,
        payout_account_id: None,
    };
    let server = Server::bind(&config).await.unwrap();
    let task = tokio::spawn(server.run(core.clone(), RuntimeMode::Active));
    let transport = FakeTransport::default();
    let worker = crate::cyberherd::notice_worker::Worker {
        store: f.store.clone(),
        core: Client::new(config.socket, uid).unwrap(),
        transport: transport.clone(),
        owner: f.owner.clone(),
    };
    assert_eq!(
        worker.tick().await.unwrap(),
        crate::cyberherd::delivery_worker::Step::Waiting
    );
    assert_eq!(transport.signs.load(Ordering::SeqCst), 0);
    core.update_feeding_presentation(
        1000,
        RuntimeMode::Active,
        Some(FeederSafety {
            override_enabled: true,
            remote_enabled: true,
        }),
    )
    .await
    .unwrap();
    sqlx::query("UPDATE notice_destinations SET lease_until=0")
        .execute(&f.db)
        .await
        .unwrap();
    transport.lose.store(true, Ordering::SeqCst);
    tokio::time::sleep(std::time::Duration::from_millis(1050)).await;
    // The first relay loses its ACK; the second must receive the same event.
    assert_eq!(
        worker.tick().await.unwrap(),
        crate::cyberherd::delivery_worker::Step::Published
    );
    assert_eq!(transport.signs.load(Ordering::SeqCst), 1);
    sqlx::query("UPDATE notice_destinations SET lease_until=0")
        .execute(&f.db)
        .await
        .unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(1050)).await;
    assert_eq!(
        worker.tick().await.unwrap(),
        crate::cyberherd::delivery_worker::Step::Published
    );
    assert_eq!(transport.signs.load(Ordering::SeqCst), 1);
    let ids = transport.ids.lock().unwrap();
    assert_eq!(ids.len(), 3);
    assert!(ids.iter().all(|id| id == &ids[0]));
    drop(ids);
    task.abort();
}

#[tokio::test]
async fn lifecycle_backpressure_rolls_back_transition_with_no_loss_or_premature_baseline() {
    let f = Fixture::new().await;
    f.observe(Some(false), f.now - 1).await;
    sqlx::query("WITH RECURSIVE n(x) AS (SELECT 1 UNION ALL SELECT x+1 FROM n WHERE x<2048) INSERT INTO notice_events(owner,identity,purpose,context_json,relays_json,created_at) SELECT ?,CAST(x AS TEXT),'pause','{}','[\"wss://relay.damus.io/\"]',0 FROM n")
        .bind(&f.owner).execute(&f.db).await.unwrap();
    sqlx::query("INSERT INTO notice_destinations(owner,identity,destination,unsigned_json) SELECT owner,identity,'note','{}' FROM notice_events").execute(&f.db).await.unwrap();
    assert!(
        f.store
            .observe_lifecycle(
                &f.owner,
                Observation {
                    state: &Fixture::state(Some(true), f.now),
                    discovery_available: false,
                    timezone: "America/Denver",
                    relays: &f.relays,
                    now: f.now
                }
            )
            .await
            .is_err()
    );
    let baseline: bool = sqlx::query_scalar("SELECT override_enabled FROM lifecycle_observer")
        .fetch_one(&f.db)
        .await
        .unwrap();
    assert!(!baseline);
    assert_eq!(f.count().await, 2048);
}

#[tokio::test]
async fn lifecycle_invitation_two_hour_boundary_is_durable_and_nonpublic_or_stale_reading_is_quiet()
{
    let f = Fixture::new().await;
    f.stream("live", "one", f.now).await;
    let mut state = Fixture::state(Some(true), f.now);
    state.nostr_enabled = false;
    f.store
        .observe_lifecycle(
            &f.owner,
            Observation {
                state: &state,
                discovery_available: true,
                timezone: "America/Denver",
                relays: &f.relays,
                now: f.now,
            },
        )
        .await
        .unwrap();
    state.nostr_enabled = true;
    f.store
        .observe_lifecycle(
            &f.owner,
            Observation {
                state: &state,
                discovery_available: true,
                timezone: "America/Denver",
                relays: &f.relays,
                now: f.now + 6,
            },
        )
        .await
        .unwrap();
    assert_eq!(f.count().await, 0);
    f.observe(Some(false), f.now).await;
    assert_eq!(f.count().await, 1);
    f.stream("live", "one", f.now + 7199).await;
    f.observe(Some(false), f.now + 7199).await;
    assert_eq!(f.count().await, 1);
    let reopened = Store::connect(&f.url).await.unwrap();
    reopened
        .observe_lifecycle(
            &f.owner,
            Observation {
                state: &Fixture::state(Some(false), f.now + 7200),
                discovery_available: true,
                timezone: "America/Denver",
                relays: &f.relays,
                now: f.now + 7200,
            },
        )
        .await
        .unwrap();
    assert_eq!(f.count().await, 2);
    // An actual previous publication delayed by 100 seconds extends cooldown.
    // Retained ACK evidence rather than queue time alone drives this gate.
    let identity: String =
        sqlx::query_scalar("SELECT identity FROM notice_events ORDER BY seq DESC LIMIT 1")
            .fetch_one(&f.db)
            .await
            .unwrap();
    let destination: String =
        sqlx::query_scalar("SELECT destination FROM notice_destinations WHERE identity=?")
            .bind(&identity)
            .fetch_one(&f.db)
            .await
            .unwrap();
    sqlx::query("INSERT INTO notice_receipts(owner,identity,destination,relay,acknowledged_at) VALUES(?,?,?,?,?)").bind(&f.owner).bind(identity).bind(destination).bind(&f.relays[0]).bind(f.now+7300).execute(&f.db).await.unwrap();
    f.stream("live", "one", f.now + 14400).await;
    f.observe(Some(false), f.now + 14400).await;
    assert_eq!(f.count().await, 2);
    f.observe(Some(false), f.now + 14500).await;
    assert_eq!(f.count().await, 3);
}

#[tokio::test]
async fn chat_opt_out_keeps_lifecycle_notes_without_invitations_or_queued_chat_retries() {
    let f = Fixture::new().await;
    f.stream("live", "retained-chat", f.now - 3).await;
    f.observe(Some(true), f.now - 2).await;
    let no_chat = f.store.clone().with_stream_chat(false);
    // Old chat remains retained; only the existing note can be selected.
    while let Some(job) = no_chat.reserve_notice(&f.owner, f.now).await.unwrap() {
        assert_eq!(job.unsigned.kind, 1);
        assert_eq!(job.destination, "note");
        let signed = VerifiedEvent::verify(test_event(
            1,
            1,
            job.unsigned.created_at,
            &job.unsigned.content,
            job.unsigned.tags.clone(),
        ))
        .unwrap();
        no_chat.save_notice_signature(&job, &signed).await.unwrap();
        let ack = RelayAck::synthetic(&signed, &job.relay);
        no_chat
            .finish_notice(&job, Some(&ack), f.now)
            .await
            .unwrap();
    }
    no_chat
        .observe_lifecycle(
            &f.owner,
            Observation {
                state: &Fixture::state(Some(false), f.now),
                discovery_available: true,
                timezone: "America/Denver",
                relays: &f.relays,
                now: f.now,
            },
        )
        .await
        .unwrap();
    let chats: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM notice_destinations WHERE destination LIKE 'stream:%'",
    )
    .fetch_one(&f.db)
    .await
    .unwrap();
    let invites: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM notice_events WHERE purpose='invitation'")
            .fetch_one(&f.db)
            .await
            .unwrap();
    assert_eq!(chats, 1); // Retain the original pause chat without publishing it.
    assert_eq!(invites, 0);
    let resume = no_chat
        .reserve_notice(&f.owner, f.now + 1)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(resume.purpose, "resume");
    assert_eq!(resume.unsigned.kind, 1);
}
