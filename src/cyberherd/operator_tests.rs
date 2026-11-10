use super::*;
use crate::{
    cyberherd::{
        conversation::Context,
        http_auth,
        membership::{Action as InteractionAction, FeederAvailability, Profile, Settings, Target},
        store::{Interaction, Store},
    },
    nostr_event::test_event,
};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use sha2::{Digest, Sha256};
use sqlx::SqlitePool;
fn key(n: u8) -> String {
    test_event(n, 0, 100, "", vec![]).pubkey
}
fn command(revision: i64, action: Action) -> Command {
    Command {
        id: uuid::Uuid::new_v4(),
        expected_revision: revision,
        action,
    }
}
fn auth(cmd: &Command, actor: u8, at: i64) -> http_auth::OperatorAuthorization {
    let body = serde_json::to_vec(cmd).unwrap();
    let event = test_event(
        actor,
        27235,
        at as u64,
        "",
        vec![
            vec![
                "u".into(),
                "https://example.com/api/v1/cyberherd/operator/command".into(),
            ],
            vec!["method".into(), "POST".into()],
            vec!["payload".into(), hex::encode(Sha256::digest(&body))],
            vec!["nonce".into(), uuid::Uuid::new_v4().to_string()],
        ],
    );
    http_auth::Config {
        public_origin: "https://example.com".into(),
        operators: vec![key(actor)],
    }
    .verify_operator(
        &format!(
            "Nostr {}",
            STANDARD.encode(serde_json::to_vec(&event).unwrap())
        ),
        "/api/v1/cyberherd/operator/command",
        &body,
        at,
    )
    .unwrap()
}
async fn store(root: &tempfile::TempDir) -> (Store, String) {
    let url = format!("sqlite://{}", root.path().join("operator.db").display());
    let store = Store::connect(&url).await.unwrap();
    store
        .initialize(
            &key(1),
            &Settings {
                minimum_sats: 10,
                max_members: 10,
                member_percent: 10,
            },
            100,
        )
        .await
        .unwrap();
    store
        .initialize_core_ingress(
            &key(1),
            &crate::cyberherd::ingress::CoreBinding {
                core_uid: 10001,
                epoch: uuid::Uuid::from_u128(1),
                start_after: 0,
                credit_pool: "herd".into(),
                payees: std::collections::BTreeMap::from([("herd".into(), key(1))]),
            },
        )
        .await
        .unwrap();
    store
        .seed_templates(
            &key(1),
            &crate::cyberherd::templates::Catalog::legacy().unwrap(),
        )
        .await
        .unwrap();
    store
        .seed_templates(
            &key(1),
            &crate::cyberherd::templates::Catalog::delivery_defaults().unwrap(),
        )
        .await
        .unwrap();
    (store, url)
}
fn profile() -> ProfileInput {
    ProfileInput {
        name: "Alice".into(),
        picture: None,
        lud16: Some("alice@example.com".into()),
    }
}
#[tokio::test]
async fn manual_commands_are_exact_revision_scoped_and_never_credit_lifetime_or_replay_social_awards()
 {
    let root = tempfile::TempDir::new().unwrap();
    let (s, url) = store(&root).await;
    // A failed free admission retains evidence; manual activation promotes once.
    let social = Interaction {
        source_id: format!("nostr:{}", "55".repeat(32)),
        source_hash: "66".repeat(32),
        pubkey: key(2),
        profile: Profile::default(),
        action: InteractionAction::Repost,
        occurred_at: 110,
    };
    s.process(&key(1), &social, Target::Current, FeederAvailability::Ready)
        .await
        .unwrap();
    let initial = s.snapshot(&key(1)).await.unwrap();
    let activate = command(
        initial.revision,
        Action::Activate {
            pubkey: key(2),
            profile: profile(),
        },
    );
    let proof = auth(&activate, 3, 120);
    let receipt = s
        .operator_command(
            &key(1),
            &proof,
            &activate,
            env(FeederAvailability::Ready, 120),
        )
        .await
        .unwrap();
    assert!(
        s.operator_command(
            &key(1),
            &proof,
            &activate,
            env(FeederAvailability::Ready, 120)
        )
        .await
        .is_err()
    );
    let reopened = Store::connect(&url).await.unwrap();
    assert_eq!(
        reopened
            .operator_command(
                &key(1),
                &auth(&activate, 3, 121),
                &activate,
                env(FeederAvailability::Unknown, 121)
            )
            .await
            .unwrap(),
        receipt
    );
    let snapshot = s.snapshot(&key(1)).await.unwrap();
    assert!(snapshot.roster.awards[&key(2)].repost);
    assert_eq!(snapshot.roster.awards[&key(2)].points(), 3);
    let amount = command(
        snapshot.revision,
        Action::Amount {
            pubkey: key(2),
            amount_sats: "9007199254740993".into(),
        },
    );
    s.operator_command(
        &key(1),
        &auth(&amount, 3, 122),
        &amount,
        env(FeederAvailability::Ready, 122),
    )
    .await
    .unwrap();
    let snapshot = s.snapshot(&key(1)).await.unwrap();
    assert_eq!(
        snapshot.roster.members[&key(2)].amount_sats,
        9_007_199_254_740_993
    );
    assert_eq!(snapshot.roster.members[&key(2)].lifetime_sats, 0);
    let stale = command(initial.revision, Action::Remove { pubkey: key(2) });
    assert!(
        s.operator_command(
            &key(1),
            &auth(&stale, 3, 123),
            &stale,
            env(FeederAvailability::Ready, 123)
        )
        .await
        .is_err()
    );
    assert_eq!(s.snapshot(&key(1)).await.unwrap(), snapshot);
    let ban = command(snapshot.revision, Action::Ban { pubkey: key(2) });
    s.operator_command(
        &key(1),
        &auth(&ban, 3, 124),
        &ban,
        env(FeederAvailability::Unknown, 124),
    )
    .await
    .unwrap();
    let mut active = command(
        snapshot.revision + 1,
        Action::Activate {
            pubkey: key(2),
            profile: profile(),
        },
    );
    assert!(
        s.operator_command(
            &key(1),
            &auth(&active, 3, 125),
            &active,
            env(FeederAvailability::Ready, 125)
        )
        .await
        .is_err()
    );
    let unban = command(snapshot.revision + 1, Action::Unban { pubkey: key(2) });
    s.operator_command(
        &key(1),
        &auth(&unban, 3, 126),
        &unban,
        env(FeederAvailability::Unknown, 126),
    )
    .await
    .unwrap();
    active.expected_revision += 1;
    s.operator_command(
        &key(1),
        &auth(&active, 3, 127),
        &active,
        env(FeederAvailability::Ready, 127),
    )
    .await
    .unwrap();
    let last = s.snapshot(&key(1)).await.unwrap();
    assert_eq!(last.roster.awards[&key(2)].points(), 3);
    assert_eq!(last.roster.members[&key(2)].amount_sats, 0);
    let mut changed = activate.clone();
    changed.action = Action::Remove { pubkey: key(2) };
    assert!(
        s.operator_command(
            &key(1),
            &auth(&changed, 3, 128),
            &changed,
            env(FeederAvailability::Ready, 128)
        )
        .await
        .is_err()
    );
    let db = SqlitePool::connect(&url).await.unwrap();
    assert!(
        sqlx::query("DELETE FROM operator_http_audit")
            .execute(&db)
            .await
            .is_err()
    );
}
#[tokio::test]
async fn failed_operator_transaction_is_retryable_and_reset_preserves_receipts_and_other_owners() {
    let root = tempfile::TempDir::new().unwrap();
    let (s, url) = store(&root).await;
    s.initialize(
        &key(4),
        &Settings {
            minimum_sats: 0,
            max_members: 1,
            member_percent: 0,
        },
        100,
    )
    .await
    .unwrap();
    let cmd = command(
        0,
        Action::Activate {
            pubkey: key(2),
            profile: profile(),
        },
    );
    let proof = auth(&cmd, 3, 120);
    let db = SqlitePool::connect(&url).await.unwrap();
    sqlx::query("CREATE TRIGGER fixture_operator_failure BEFORE INSERT ON operator_http_audit BEGIN SELECT RAISE(ABORT,'fixture'); END").execute(&db).await.unwrap();
    assert!(
        s.operator_command(&key(1), &proof, &cmd, env(FeederAvailability::Ready, 120))
            .await
            .is_err()
    );
    assert_eq!(s.snapshot(&key(1)).await.unwrap().revision, 0);
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM http_auth_nonces")
            .fetch_one(&db)
            .await
            .unwrap(),
        0
    );
    sqlx::query("DROP TRIGGER fixture_operator_failure")
        .execute(&db)
        .await
        .unwrap();
    s.operator_command(&key(1), &proof, &cmd, env(FeederAvailability::Ready, 120))
        .await
        .unwrap();
    let reset = command(1, Action::Reset);
    s.operator_command(
        &key(1),
        &auth(&reset, 3, 130),
        &reset,
        env(FeederAvailability::Unknown, 130),
    )
    .await
    .unwrap();
    let snapshot = s.snapshot(&key(1)).await.unwrap();
    assert_eq!(snapshot.cycle, 2);
    assert_eq!(snapshot.cycle_start, 130);
    assert!(!snapshot.roster.members[&key(2)].active);
    assert_eq!(s.snapshot(&key(4)).await.unwrap().revision, 0);
    assert!(
        s.operator_command(&key(1), &proof, &cmd, env(FeederAvailability::Ready, 130))
            .await
            .is_err()
    ); // consumed event remains protected
    assert_eq!(
        s.operator_command(
            &key(1),
            &auth(&cmd, 3, 131),
            &cmd,
            env(FeederAvailability::Unknown, 131)
        )
        .await
        .unwrap()
        .revision,
        1
    ); // exact old receipt never reactivates
    assert_eq!(s.snapshot(&key(1)).await.unwrap(), snapshot);
    assert!(
        !s.events_after(&key(1), 0, 100)
            .await
            .unwrap()
            .iter()
            .any(|e| matches!(
                e.event_type.as_str(),
                "payment_received" | "feeder_confirmed"
            ))
    );
}
#[tokio::test]
async fn settings_zero_pool_template_opt_out_and_paused_activation_remain_atomic() {
    let root = tempfile::TempDir::new().unwrap();
    let (s, _) = store(&root).await;
    let settings = command(
        0,
        Action::Settings {
            minimum_sats: "0".into(),
            max_members: 1,
            member_percent: 0,
        },
    );
    s.operator_command(
        &key(1),
        &auth(&settings, 3, 120),
        &settings,
        env(FeederAvailability::Unknown, 120),
    )
    .await
    .unwrap();
    let activate = command(
        1,
        Action::Activate {
            pubkey: key(2),
            profile: profile(),
        },
    );
    assert!(
        s.operator_command(
            &key(1),
            &auth(&activate, 3, 121),
            &activate,
            env(FeederAvailability::Paused, 121)
        )
        .await
        .is_err()
    );
    s.operator_command(
        &key(1),
        &auth(&activate, 3, 122),
        &activate,
        env(FeederAvailability::Ready, 122),
    )
    .await
    .unwrap();
    assert_eq!(
        s.snapshot(&key(1))
            .await
            .unwrap()
            .roster
            .shares(&s.snapshot(&key(1)).await.unwrap().settings)
            .unwrap()[&key(2)],
        0
    );
    let extra = command(
        2,
        Action::Activate {
            pubkey: key(4),
            profile: profile(),
        },
    );
    assert!(
        s.operator_command(
            &key(1),
            &auth(&extra, 3, 123),
            &extra,
            env(FeederAvailability::Ready, 123)
        )
        .await
        .is_err()
    );
    let disable = command(
        2,
        Action::Template {
            edit: crate::cyberherd::store::TemplateEdit::DisableCategory {
                category: "member_added".into(),
            },
        },
    );
    s.operator_command(
        &key(1),
        &auth(&disable, 3, 124),
        &disable,
        env(FeederAvailability::Unknown, 124),
    )
    .await
    .unwrap();
    s.seed_templates(
        &key(1),
        &crate::cyberherd::templates::Catalog::legacy().unwrap(),
    )
    .await
    .unwrap();
    assert!(
        s.templates(&key(1))
            .await
            .unwrap()
            .category("member_added")
            .is_none()
    );
    let bad = command(
        3,
        Action::Template {
            edit: crate::cyberherd::store::TemplateEdit::Set {
                category: "member_added".into(),
                variant: "custom".into(),
                text: "{unsafe.field}".into(),
            },
        },
    );
    assert!(
        s.operator_command(
            &key(1),
            &auth(&bad, 3, 125),
            &bad,
            env(FeederAvailability::Unknown, 125)
        )
        .await
        .is_err()
    );
    assert_eq!(s.snapshot(&key(1)).await.unwrap().revision, 3);
    let private = s
        .operator_template_page(&key(1), "member_added", 0, Some(3))
        .await
        .unwrap();
    assert_eq!(private["enabled"], false);
}

fn env(feeder: FeederAvailability, now: i64) -> Environment {
    Environment {
        feeder,
        context: Context::default(),
        ordering: Some(CoreFence {
            epoch: uuid::Uuid::from_u128(1),
            core_uid: 10001,
            through: 0,
            observed_at: now,
        }),
        now,
    }
}

#[tokio::test]
async fn pending_feeding_and_stale_core_observation_hold_operator_edits_without_consuming_their_identity()
 {
    let root = tempfile::TempDir::new().unwrap();
    let (s, url) = store(&root).await;
    let cmd = command(
        0,
        Action::Settings {
            minimum_sats: "20".into(),
            max_members: 10,
            member_percent: 10,
        },
    );
    let proof = auth(&cmd, 3, 120);
    let db = SqlitePool::connect(&url).await.unwrap();
    sqlx::query("INSERT INTO ingress(owner,source_key,kind,payload_json) VALUES(?,'fixture-feeding','feeding','{}')").bind(key(1)).execute(&db).await.unwrap();
    assert!(
        s.operator_command(&key(1), &proof, &cmd, env(FeederAvailability::Unknown, 120))
            .await
            .is_err()
    );
    assert_eq!(s.snapshot(&key(1)).await.unwrap().revision, 0);
    sqlx::query("UPDATE ingress SET state='done' WHERE source_key='fixture-feeding'")
        .execute(&db)
        .await
        .unwrap();
    let mut stale = env(FeederAvailability::Unknown, 120);
    stale.ordering.as_mut().unwrap().observed_at = 114;
    assert!(
        s.operator_command(&key(1), &proof, &cmd, stale)
            .await
            .is_err()
    );
    s.operator_command(&key(1), &proof, &cmd, env(FeederAvailability::Unknown, 120))
        .await
        .unwrap();
    assert_eq!(s.snapshot(&key(1)).await.unwrap().settings.minimum_sats, 20);
}

#[tokio::test]
async fn manual_notices_freeze_one_share_footer_and_clear_empty_rosters_without_reset_announcements()
 {
    let root = tempfile::TempDir::new().unwrap();
    let (s, _) = store(&root).await;
    let note = crate::cyberherd::targets::ConversationTarget {
        event_id: "ab".repeat(32),
        kind: 1,
        coordinate: None,
        payee_pubkey: key(1),
    };
    let context = Context {
        note: Some(note),
        stream: None,
    };
    let cmd = command(
        0,
        Action::Activate {
            pubkey: key(2),
            profile: profile(),
        },
    );
    let mut e = env(FeederAvailability::Ready, 120);
    e.context = context;
    s.operator_command(&key(1), &auth(&cmd, 3, 120), &cmd, e)
        .await
        .unwrap();
    let amount = command(
        1,
        Action::Amount {
            pubkey: key(2),
            amount_sats: "34".into(),
        },
    );
    s.operator_command(
        &key(1),
        &auth(&amount, 3, 121),
        &amount,
        env(FeederAvailability::Ready, 121),
    )
    .await
    .unwrap();
    let remove = command(2, Action::Remove { pubkey: key(2) });
    s.operator_command(
        &key(1),
        &auth(&remove, 3, 122),
        &remove,
        env(FeederAvailability::Unknown, 122),
    )
    .await
    .unwrap();
    let events = s.events_after(&key(1), 0, 100).await.unwrap();
    let catalog = s.templates(&key(1)).await.unwrap();
    let notices: Vec<_> = events
        .iter()
        .filter_map(|e| crate::cyberherd::delivery::render(e, &catalog).unwrap())
        .collect();
    assert_eq!(notices.len(), 3);
    assert!(notices[0].accepted);
    assert!(notices[1].requires_joining);
    assert!(notices[2].requires_joining);
    assert!(notices[1].overlay.contains("34"));
    assert!(!notices[1].overlay.contains("received"));
    assert!(notices[0].overlay.contains("Alice"));
    assert!(!notices[0].overlay.contains("nostr:npub"));
    assert_eq!(notices[0].nostr.matches("nostr:npub").count(), 1); // footer names have no extra mentions
    assert!(notices[2].overlay.contains("No active CyberHerd members."));
    assert_eq!(notices[0].conversation, notices[2].conversation);
    let reset = command(3, Action::Reset);
    s.operator_command(
        &key(1),
        &auth(&reset, 3, 123),
        &reset,
        env(FeederAvailability::Unknown, 123),
    )
    .await
    .unwrap();
    assert!(
        crate::cyberherd::delivery::render(
            s.events_after(&key(1), 0, 100)
                .await
                .unwrap()
                .last()
                .unwrap(),
            &catalog
        )
        .unwrap()
        .is_none()
    );
}

#[test]
fn share_footer_lists_ineligible_active_members_at_zero_and_honors_each_template_opt_out() {
    let owner = key(1);
    let member = key(2);
    let profile = Profile {
        name: "Alice".into(),
        picture: None,
        lud16: None,
    };
    let mut roster = crate::cyberherd::membership::Roster::default();
    roster.members.insert(
        member.clone(),
        crate::cyberherd::membership::Member {
            profile: profile.clone(),
            active: true,
            amount_sats: 34,
            lifetime_sats: 34,
        },
    );
    let snapshot = crate::cyberherd::store::Snapshot {
        owner: owner.clone(),
        settings: Settings {
            minimum_sats: 10,
            max_members: 10,
            member_percent: 10,
        },
        roster,
        cycle: 1,
        cycle_start: 100,
        revision: 1,
    };
    let notice = Notice {
        member,
        profile,
        category: "member_added".into(),
        snapshot,
        allocation_changed: true,
        context: Context::default(),
        conversation: None,
        activation: true,
    };
    let event = crate::cyberherd::store::StoredEvent {
        seq: 1,
        owner,
        cycle: 1,
        source_id: "operator:footer".into(),
        event_type: "operator_member".into(),
        snapshot_json: serde_json::to_string(&notice).unwrap(),
    };
    let catalog = crate::cyberherd::templates::Catalog::legacy().unwrap();
    let message = crate::cyberherd::delivery::render(&event, &catalog)
        .unwrap()
        .unwrap();
    assert!(message.overlay.contains("Alice"));
    assert!(message.overlay.contains("0%"));
    assert!(!message.overlay.contains("No active"));
    for category in [
        "cyber_herd_share_header",
        "cyber_herd_share_row",
        "cyber_herd_share_empty",
    ] {
        let mut variants = catalog.categories().clone();
        variants.remove(category);
        let opt_out = crate::cyberherd::templates::Catalog::from_json(
            &serde_json::to_string(&variants).unwrap(),
        )
        .unwrap();
        let message = crate::cyberherd::delivery::render(&event, &opt_out)
            .unwrap()
            .unwrap();
        assert!(!message.overlay.contains("Current CyberHerd shares"));
        assert!(!message.overlay.contains("0%"));
        assert!(message.overlay.contains("Alice"));
    }
}

#[tokio::test]
async fn manual_reset_discards_same_second_deferred_work_atomically_without_erasing_replay_receipts()
 {
    let root = tempfile::TempDir::new().unwrap();
    let (s, url) = store(&root).await;
    let social = Interaction {
        source_id: format!("nostr:{}", "77".repeat(32)),
        source_hash: "88".repeat(32),
        pubkey: key(2),
        profile: Profile::from(&profile()),
        action: InteractionAction::Like,
        occurred_at: 120,
    };
    assert_eq!(
        s.process(
            &key(1),
            &social,
            Target::Current,
            FeederAvailability::Paused
        )
        .await
        .unwrap()
        .status,
        crate::cyberherd::store::Status::Deferred
    );
    let db = SqlitePool::connect(&url).await.unwrap();
    sqlx::query("INSERT INTO ingress(owner,source_key,kind,payload_json) VALUES(?,'fixture-reset-social','social','{}')").bind(key(1)).execute(&db).await.unwrap();
    let reset = command(0, Action::Reset);
    let proof = auth(&reset, 3, 120);
    sqlx::query("CREATE TRIGGER fixture_reset_failure BEFORE INSERT ON operator_http_audit BEGIN SELECT RAISE(ABORT,'fixture'); END").execute(&db).await.unwrap();
    assert!(
        s.operator_command(
            &key(1),
            &proof,
            &reset,
            env(FeederAvailability::Unknown, 120)
        )
        .await
        .is_err()
    );
    assert_eq!(s.deferred(&key(1), 10).await.unwrap().len(), 1);
    assert_eq!(
        sqlx::query_scalar::<_, String>(
            "SELECT state FROM ingress WHERE source_key='fixture-reset-social'"
        )
        .fetch_one(&db)
        .await
        .unwrap(),
        "pending"
    );
    sqlx::query("DROP TRIGGER fixture_reset_failure")
        .execute(&db)
        .await
        .unwrap();
    s.operator_command(
        &key(1),
        &proof,
        &reset,
        env(FeederAvailability::Unknown, 120),
    )
    .await
    .unwrap();
    let reopened = Store::connect(&url).await.unwrap();
    assert!(reopened.deferred(&key(1), 10).await.unwrap().is_empty());
    assert_eq!(
        sqlx::query_scalar::<_, String>(
            "SELECT state FROM ingress WHERE source_key='fixture-reset-social'"
        )
        .fetch_one(&db)
        .await
        .unwrap(),
        "done"
    );
    let receipt = reopened
        .process(&key(1), &social, Target::Current, FeederAvailability::Ready)
        .await
        .unwrap();
    assert_eq!(receipt.status, crate::cyberherd::store::Status::Discarded);
    let snapshot = reopened.snapshot(&key(1)).await.unwrap();
    assert_eq!(snapshot.cycle, 2);
    assert!(snapshot.roster.awards.is_empty());
    assert_eq!(snapshot.roster.active_count(), 0);
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM interactions")
            .fetch_one(&db)
            .await
            .unwrap(),
        1
    );
}
