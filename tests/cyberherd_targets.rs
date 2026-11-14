use std::collections::BTreeSet;

use bitcoin::secp256k1::{Keypair, Message, Secp256k1, SecretKey};
use lightning_goats::{
    cyberherd::{
        calendar::{DEFAULT_TIMEZONE, DayWindow},
        membership::{Action, FeederAvailability, Profile, Settings, Target},
        social::{self, SocialEnvironment, SocialEvent, SocialStep},
        store::{Interaction, Status, Store},
        targets::{
            AuthenticatedProfile, ProviderCache, StreamAuthentication, TargetPolicy, Targets,
        },
    },
    nostr::SignedNostrEvent,
    nostr_event::{VerifiedEvent, canonical_bytes},
};
use serde_json::json;
use sha2::{Digest, Sha256};
use tempfile::TempDir;

fn key(marker: u8) -> Keypair {
    Keypair::from_secret_key(
        &Secp256k1::new(),
        &SecretKey::from_slice(&[marker; 32]).unwrap(),
    )
}
fn public(marker: u8) -> String {
    key(marker).x_only_public_key().0.to_string()
}
fn signed(marker: u8, kind: u64, at: i64, tags: Vec<Vec<String>>, content: &str) -> VerifiedEvent {
    let mut event = SignedNostrEvent {
        id: String::new(),
        pubkey: public(marker),
        created_at: at as u64,
        kind,
        tags,
        content: content.into(),
        sig: String::new(),
    };
    let digest: [u8; 32] = Sha256::digest(canonical_bytes(&event).unwrap()).into();
    event.id = hex::encode(digest);
    event.sig = Secp256k1::new()
        .sign_schnorr_no_aux_rand(&Message::from_digest(digest), &key(marker))
        .to_string();
    VerifiedEvent::verify(event).unwrap()
}
fn tag(values: &[&str]) -> Vec<String> {
    values.iter().map(|s| (*s).into()).collect()
}
fn settings() -> Settings {
    Settings {
        minimum_sats: 10,
        max_members: 10,
        member_percent: 10,
    }
}
async fn store(now: i64) -> (TempDir, String, Store) {
    let root = TempDir::new().unwrap();
    let url = format!("sqlite://{}", root.path().join("cyberherd.db").display());
    let store = Store::connect(&url).await.unwrap();
    store
        .initialize(&public(1), &settings(), now)
        .await
        .unwrap();
    (root, url, store)
}

#[tokio::test]
async fn ended_head_and_negative_profile_updates_survive_reopen_and_old_event_replay() {
    let now = 1_794_225_600;
    let (_root, url, store) = store(now).await;
    let owner = public(1);
    let mut cache = ProviderCache::default();
    let mut versions = Vec::new();
    for (at, status) in [(now - 20, "live"), (now - 10, "ended")] {
        let StreamAuthentication::Accepted(stream) = cache
            .authenticate(
                signed(
                    1,
                    30311,
                    at,
                    vec![tag(&["d", "show"]), tag(&["status", status])],
                    "",
                ),
                &owner,
                now,
            )
            .await
            .unwrap()
        else {
            panic!("own signed stream rejected")
        };
        assert!(store.remember_stream(&owner, &stream).await.unwrap());
        versions.push(stream);
    }
    let old = signed(
        2,
        0,
        now - 20,
        vec![],
        &json!({"lud16":"tester@example.com"}).to_string(),
    );
    let new = signed(2, 0, now - 10, vec![], "{}");
    for event in [old.clone(), new.clone()] {
        let profile = AuthenticatedProfile::parse(event, &public(2), now).unwrap();
        assert!(store.remember_profile(&owner, &profile).await.unwrap());
    }
    drop(store);
    let store = Store::connect(&url).await.unwrap();
    assert!(!store.remember_stream(&owner, &versions[0]).await.unwrap());
    assert!(
        !store
            .remember_profile(
                &owner,
                &AuthenticatedProfile::parse(old, &public(2), now).unwrap()
            )
            .await
            .unwrap()
    );
    let profile = store
        .profile(&owner, &public(2), now)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(profile.event().event().id, new.event().id);
    assert!(profile.profile().lud16.is_none());
    let policy = TargetPolicy {
        project_pubkey: owner.clone(),
        tracked_tags: vec!["LightningGoats".into()],
    };
    let targets = store
        .target_projection(
            &policy,
            &DayWindow::at(DEFAULT_TIMEZONE, now).unwrap(),
            &[versions[0].event().event().id.clone()],
            BTreeSet::new(),
            now,
        )
        .await
        .unwrap();
    assert!(targets.current().is_empty());
    assert_eq!(
        targets
            .classify(&[versions[0].event().event().id.clone()], &[], true)
            .0,
        Target::Other
    );
    assert!(
        store
            .stream_alias(&owner, &versions[0].event().event().id, now)
            .await
            .unwrap()
            .is_some()
    );
}

#[tokio::test]
async fn raw_social_inbox_retries_after_restart_and_completed_events_never_reopen() {
    let now = 1_794_225_600;
    let (_root, url, store) = store(now).await;
    let owner = public(1);
    let reaction = signed(2, 7, now, vec![tag(&["e", &"12".repeat(32)])], "+");
    assert!(store.enqueue_social(&owner, &reaction, now).await.unwrap());
    assert!(!store.enqueue_social(&owner, &reaction, now).await.unwrap());
    assert_eq!(store.social_batch(&owner, now, 10).await.unwrap().len(), 1);
    store
        .finish_social(&owner, &reaction.event().id, false, now)
        .await
        .unwrap();
    assert!(
        store
            .social_batch(&owner, now, 10)
            .await
            .unwrap()
            .is_empty()
    );
    drop(store);
    let store = Store::connect(&url).await.unwrap();
    assert_eq!(
        store.social_batch(&owner, now + 5, 10).await.unwrap().len(),
        1
    );
    store
        .finish_social(&owner, &reaction.event().id, true, now + 5)
        .await
        .unwrap();
    assert!(
        !store
            .enqueue_social(&owner, &reaction, now + 6)
            .await
            .unwrap()
    );
    store
        .finish_social(&owner, &reaction.event().id, false, now + 6)
        .await
        .unwrap();
    assert!(
        store
            .social_batch(&owner, now + 1000, 10)
            .await
            .unwrap()
            .is_empty()
    );
    assert!(
        store
            .enqueue_social(&owner, &signed(1, 7, now, vec![], "+"), now)
            .await
            .is_err()
    );
    assert!(
        store
            .enqueue_social(&owner, &signed(2, 1311, now, vec![], "chat"), now)
            .await
            .is_err()
    );
}

#[tokio::test]
async fn mountain_reset_catches_up_once_and_keeps_lifetime_and_source_receipts() {
    let now = chrono::DateTime::parse_from_rfc3339("2026-11-01T18:00:00Z")
        .unwrap()
        .timestamp();
    let day = DayWindow::at(DEFAULT_TIMEZONE, now).unwrap();
    let (_root, url, store) = store(day.start - 100).await;
    let owner = public(1);
    let interaction = Interaction {
        source_id: "settlement:isolated-fixture".into(),
        source_hash: "12".repeat(32),
        pubkey: public(2),
        profile: Profile {
            name: "Tester".into(),
            picture: None,
            lud16: Some("tester@example.com".into()),
        },
        action: Action::Zap { amount_sats: 10 },
        occurred_at: day.start - 10,
    };
    store
        .process(
            &owner,
            &interaction,
            Target::Current,
            FeederAvailability::Ready,
        )
        .await
        .unwrap();
    assert_eq!(
        store
            .catch_up_calendar(&owner, DEFAULT_TIMEZONE, false, now)
            .await
            .unwrap()
            .cycle,
        1
    );
    let reset = store
        .catch_up_calendar(&owner, DEFAULT_TIMEZONE, true, now)
        .await
        .unwrap();
    assert_eq!(reset.cycle, 2);
    assert_eq!(reset.cycle_start, day.start);
    assert_eq!(reset.roster.members[&public(2)].amount_sats, 0);
    assert_eq!(reset.roster.members[&public(2)].lifetime_sats, 10);
    assert_eq!(
        store
            .frozen_interaction(&owner, &interaction.source_id)
            .await
            .unwrap(),
        Some(interaction.clone())
    );
    drop(store);
    let store = Store::connect(&url).await.unwrap();
    assert_eq!(
        store
            .catch_up_calendar(&owner, DEFAULT_TIMEZONE, true, now + 3600)
            .await
            .unwrap(),
        reset
    );
    store
        .process(
            &owner,
            &interaction,
            Target::Current,
            FeederAvailability::Ready,
        )
        .await
        .unwrap();
    assert_eq!(store.snapshot(&owner).await.unwrap(), reset);
}

#[test]
fn social_parser_counts_standard_likes_and_requires_consistent_signed_reposts() {
    let now = 1_794_225_600;
    let owner = public(1);
    let note = signed(1, 1, now, vec![], "Project note");
    for content in ["+", ""] {
        assert_eq!(
            SocialEvent::parse(
                signed(2, 7, now, vec![tag(&["e", &note.event().id])], content),
                &owner,
                now
            )
            .unwrap()
            .unwrap()
            .action(),
            Action::Like
        );
    }
    for content in ["-", "🐐", ":goat:", "great show", " + "] {
        assert!(
            SocialEvent::parse(
                signed(2, 7, now, vec![tag(&["e", &note.event().id])], content),
                &owner,
                now
            )
            .unwrap()
            .is_none()
        );
    }
    let body = serde_json::to_string(note.event()).unwrap();
    assert!(
        SocialEvent::parse(
            signed(2, 6, now, vec![tag(&["e", &note.event().id])], &body),
            &owner,
            now
        )
        .unwrap()
        .is_some()
    );
    assert!(
        SocialEvent::parse(
            signed(2, 6, now, vec![tag(&["e", &"12".repeat(32)])], &body),
            &owner,
            now
        )
        .is_err()
    );
    assert!(
        SocialEvent::parse(
            signed(
                2,
                6,
                now,
                vec![tag(&["e", &note.event().id])],
                "a quoted sentence"
            ),
            &owner,
            now
        )
        .is_err()
    );
    assert!(
        SocialEvent::parse(
            signed(2, 16, now, vec![tag(&["e", &note.event().id])], ""),
            &owner,
            now
        )
        .is_err()
    );
    assert!(
        SocialEvent::parse(signed(2, 16, now, vec![], &body), &owner, now)
            .unwrap()
            .is_none()
    );
    let goal = signed(
        1,
        9041,
        now,
        vec![
            tag(&["amount", "1000"]),
            tag(&["relays", "wss://relay.example"]),
        ],
        "Goal",
    );
    let goal_body = serde_json::to_string(goal.event()).unwrap();
    assert!(
        SocialEvent::parse(signed(2, 16, now, vec![], &goal_body), &owner, now)
            .unwrap()
            .is_some()
    );
    assert!(
        SocialEvent::parse(signed(2, 1311, now, vec![], "+"), &owner, now)
            .unwrap()
            .is_none()
    );
    assert!(
        SocialEvent::parse(
            signed(1, 7, now, vec![tag(&["e", &note.event().id])], "+"),
            &owner,
            now
        )
        .unwrap()
        .is_none()
    );
}

#[tokio::test]
async fn social_recovery_freezes_profile_and_conversation_atomically_and_awards_once() {
    let now = 1_794_225_600;
    let (_root, url, store) = store(now - 100).await;
    let owner = public(1);
    let policy = TargetPolicy {
        project_pubkey: owner.clone(),
        tracked_tags: vec!["CyberHerd".into()],
    };
    let note = signed(
        1,
        1,
        now - 10,
        vec![tag(&["t", "CyberHerd"])],
        "Today's note",
    );
    store
        .remember_project_event(&policy, &note, now)
        .await
        .unwrap();
    let event = signed(
        2,
        7,
        now,
        vec![tag(&["e", &note.event().id]), tag(&["p", &owner])],
        "+",
    );
    assert!(
        social::stage(&store, &owner, event.clone(), now)
            .await
            .unwrap()
    );
    let social = SocialEvent::parse(event, &owner, now).unwrap().unwrap();
    let day = DayWindow::at(DEFAULT_TIMEZONE, now).unwrap();
    let targets = store
        .target_projection(
            &policy,
            &day,
            &[note.event().id.clone()],
            BTreeSet::new(),
            now,
        )
        .await
        .unwrap();
    let env = |profile, feeder| SocialEnvironment {
        targets: &targets,
        discovery_available: true,
        feeder,
        profile,
        now,
    };
    assert_eq!(
        social::process(
            &store,
            &owner,
            &social,
            env(None, FeederAvailability::Paused)
        )
        .await
        .unwrap(),
        SocialStep::AwaitingProfile
    );
    assert!(
        store
            .frozen_interaction(&owner, &format!("nostr:{}", social.event().event().id))
            .await
            .unwrap()
            .is_none()
    );
    let profile = AuthenticatedProfile::parse(
        signed(
            2,
            0,
            now,
            vec![],
            &json!({"name":"Original","lud16":"tester@example.com"}).to_string(),
        ),
        &public(2),
        now,
    )
    .unwrap();
    let SocialStep::Recorded(unavailable) = social::process(
        &store,
        &owner,
        &social,
        SocialEnvironment {
            discovery_available: false,
            ..env(Some(&profile), FeederAvailability::Ready)
        },
    )
    .await
    .unwrap() else {
        panic!("unavailable discovery must remain deferred")
    };
    assert_eq!(unavailable.status, Status::Deferred);
    assert!(
        store
            .snapshot(&owner)
            .await
            .unwrap()
            .roster
            .members
            .is_empty()
    );
    let SocialStep::Recorded(deferred) = social::process(
        &store,
        &owner,
        &social,
        env(Some(&profile), FeederAvailability::Paused),
    )
    .await
    .unwrap() else {
        panic!("expected durable deferral")
    };
    assert_eq!(deferred.status, Status::Deferred);
    assert!(
        store
            .snapshot(&owner)
            .await
            .unwrap()
            .roster
            .members
            .is_empty()
    );
    assert!(store.events_after(&owner, 0, 100).await.unwrap().is_empty());
    drop(store);
    let store = Store::connect(&url).await.unwrap();
    let negative =
        AuthenticatedProfile::parse(signed(2, 0, now + 1, vec![], "{}"), &public(2), now).unwrap();
    let SocialStep::Recorded(applied) = social::process(
        &store,
        &owner,
        &social,
        env(Some(&negative), FeederAvailability::Ready),
    )
    .await
    .unwrap() else {
        panic!("expected applied interaction")
    };
    assert_eq!(applied.status, Status::Applied);
    let snapshot = store.snapshot(&owner).await.unwrap();
    assert_eq!(snapshot.roster.members[&public(2)].profile.name, "Original");
    assert!(snapshot.roster.awards[&public(2)].like);
    let events = store.events_after(&owner, 0, 100).await.unwrap();
    assert_eq!(events.len(), 1);
    let payload: serde_json::Value = serde_json::from_str(&events[0].snapshot_json).unwrap();
    assert_eq!(payload["context"]["matched"]["event_id"], note.event().id);
    assert_eq!(payload["context"]["destinations"][0]["kind"], 1);
    assert_eq!(
        social::process(
            &store,
            &owner,
            &social,
            env(None, FeederAvailability::Ready)
        )
        .await
        .unwrap(),
        SocialStep::Recorded(applied)
    );
    assert_eq!(store.snapshot(&owner).await.unwrap(), snapshot);
    assert_eq!(store.events_after(&owner, 0, 100).await.unwrap().len(), 1);
    assert!(
        store
            .social_batch(&owner, now + 100, 10)
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn unknown_targets_wait_and_last_reaction_reference_cannot_reward_an_ancestor() {
    let now = 1_794_225_600;
    let (_root, _url, store) = store(now - 100).await;
    let owner = public(1);
    let policy = TargetPolicy {
        project_pubkey: owner.clone(),
        tracked_tags: vec!["CyberHerd".into()],
    };
    let note = signed(
        1,
        1,
        now - 10,
        vec![tag(&["t", "CyberHerd"])],
        "Today's note",
    );
    let foreign = signed(3, 1, now - 10, vec![], "Unrelated target");
    let social = SocialEvent::parse(
        signed(
            2,
            7,
            now,
            vec![
                tag(&["e", &note.event().id]),
                tag(&["e", &foreign.event().id]),
            ],
            "+",
        ),
        &owner,
        now,
    )
    .unwrap()
    .unwrap();
    let day = DayWindow::at(DEFAULT_TIMEZONE, now).unwrap();
    let unknown = Targets::project(
        policy.clone(),
        &day,
        vec![note.clone()],
        vec![],
        BTreeSet::new(),
        now,
    )
    .unwrap();
    assert_eq!(social.classify(&unknown, true).0, Target::Unavailable);
    let resolved = Targets::project(
        policy,
        &day,
        vec![note, foreign],
        vec![],
        BTreeSet::new(),
        now,
    )
    .unwrap();
    assert_eq!(social.classify(&resolved, true).0, Target::Other);
    social::stage(&store, &owner, social.event().clone(), now)
        .await
        .unwrap();
    let result = social::process(
        &store,
        &owner,
        &social,
        SocialEnvironment {
            targets: &resolved,
            discovery_available: true,
            feeder: FeederAvailability::Ready,
            profile: None,
            now,
        },
    )
    .await
    .unwrap();
    let SocialStep::Recorded(receipt) = result else {
        panic!("stale target should discard without profile lookup")
    };
    assert_eq!(receipt.status, Status::Discarded);
    assert!(
        store
            .snapshot(&owner)
            .await
            .unwrap()
            .roster
            .members
            .is_empty()
    );
    assert!(store.events_after(&owner, 0, 100).await.unwrap().is_empty());
}

#[tokio::test]
async fn matching_coordinate_does_not_turn_unverified_stream_version_into_an_alias() {
    let now = 1_794_225_600;
    let owner = public(1);
    let mut cache = ProviderCache::default();
    let current = signed(
        1,
        30311,
        now - 10,
        vec![tag(&["d", "show"]), tag(&["status", "live"])],
        "",
    );
    let StreamAuthentication::Accepted(current) =
        cache.authenticate(current, &owner, now).await.unwrap()
    else {
        panic!("own stream rejected")
    };
    let unverified_version = signed(
        1,
        30311,
        now - 20,
        vec![tag(&["d", "show"]), tag(&["status", "live"])],
        "",
    );
    let targets = Targets::project(
        TargetPolicy {
            project_pubkey: owner.clone(),
            tracked_tags: vec!["CyberHerd".into()],
        },
        &DayWindow::at(DEFAULT_TIMEZONE, now).unwrap(),
        vec![unverified_version.clone()],
        vec![*current.clone()],
        BTreeSet::new(),
        now,
    )
    .unwrap();
    let social = SocialEvent::parse(
        signed(
            2,
            7,
            now,
            vec![
                tag(&["e", &unverified_version.event().id]),
                tag(&["a", current.coordinate()]),
            ],
            "+",
        ),
        &owner,
        now,
    )
    .unwrap()
    .unwrap();
    assert_eq!(social.classify(&targets, true).0, Target::Unavailable);
    let repost = SocialEvent::parse(
        signed(2, 16, now, vec![tag(&["a", current.coordinate()])], ""),
        &owner,
        now,
    )
    .unwrap()
    .unwrap();
    assert_eq!(repost.classify(&targets, true).0, Target::Current);
}
