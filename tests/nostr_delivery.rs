#![cfg(unix)]

use std::{fs, os::unix::fs::PermissionsExt, time::Duration};

use lightning_goats::{
    config::{NostrConfig, RuntimeMode},
    ledger::LedgerStore,
    messaging::{process_next_message, run_outbox_publisher},
    nostr::NakClient,
    presentation::MessageRenderer,
};
use serde_json::{Value, json};
use tempfile::TempDir;
use tokio::time::{sleep, timeout};

const TEST_KEY: &str = "synthetic-client-key-never-real";

struct Fixture {
    root: TempDir,
    config: NostrConfig,
}

impl Fixture {
    fn new(mode: &str) -> Self {
        let root = TempDir::new().unwrap();
        let executable = root.path().join("nak-stub");
        fs::write(&executable, include_str!("fixtures/nak_stub.py")).unwrap();
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
        let runtime = root.path().join("runtime");
        fs::create_dir(&runtime).unwrap();
        fs::write(runtime.join("mode"), mode).unwrap();
        let config = NostrConfig {
            nak_path: executable,
            nak_config_path: runtime,
            bunker_pubkey: "ab".repeat(32),
            relays: vec!["wss://relay.invalid".to_owned()],
            bunker_relays: None,
        };
        Self { root, config }
    }

    fn client(&self) -> NakClient {
        NakClient::new(&self.config, TEST_KEY.to_owned()).unwrap()
    }

    fn trace(&self) -> Vec<Value> {
        fs::read_to_string(self.config.nak_config_path.join("calls.jsonl"))
            .unwrap_or_default()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect()
    }

    fn database(&self) -> String {
        format!("sqlite://{}", self.root.path().join("ledger.db").display())
    }
}

fn tags() -> Vec<Vec<String>> {
    vec![vec!["t".to_owned(), "LightningGoats".to_owned()]]
}

#[tokio::test]
async fn signing_transport_and_announcement_relays_are_independent() {
    let mut fixture = Fixture::new("success");
    fixture.config.bunker_relays = Some(vec!["wss://bunker.invalid".to_owned()]);
    fixture.config.relays = vec![
        "wss://publication-one.invalid".to_owned(),
        "wss://publication-two.invalid".to_owned(),
    ];
    let client = fixture.client();
    let event = client.sign_kind1("separate routes", tags()).await.unwrap();
    client.publish_signed(&event).await.unwrap();
    let calls = fixture.trace();
    let sign = calls.iter().find(|call| call["phase"] == "sign").unwrap();
    let publish = calls
        .iter()
        .find(|call| call["phase"] == "publish")
        .unwrap();
    assert_eq!(sign["bunker_relays"], json!(["wss://bunker.invalid"]));
    assert_eq!(publish["relay_args"], json!(fixture.config.relays));
    assert_eq!(publish["bunker_relays"], json!([]));
    assert_eq!(publish["payload"], serde_json::to_value(event).unwrap());
}

#[tokio::test]
async fn exact_message_round_trips_and_only_signing_gets_client_credentials() {
    let fixture = Fixture::new("success");
    let client = fixture.client();
    let event = client
        .sign_kind1("250 sats received 🐐", tags())
        .await
        .unwrap();
    assert_eq!(event.content, "250 sats received 🐐");
    assert_eq!(event.tags, tags());
    client.publish_signed(&event).await.unwrap();
    let calls = fixture.trace();
    assert_eq!(calls.len(), 4);
    assert_eq!(calls[0]["phase"], "sign");
    assert_eq!(calls[1]["phase"], "verify");
    assert_eq!(calls[2]["phase"], "verify");
    assert_eq!(calls[3]["phase"], "publish");
    for call in calls {
        let signing = call["phase"] == "sign";
        assert_eq!(call["has_client_key"], signing);
        assert_eq!(call["has_signer_uri"], signing);
    }
}

#[tokio::test]
async fn signer_cannot_change_requested_content_or_tags() {
    for mode in ["mutated_content", "mutated_tags"] {
        let fixture = Fixture::new(mode);
        let error = fixture
            .client()
            .sign_kind1("intended message", tags())
            .await
            .expect_err(mode);
        assert!(format!("{error:#}").contains("requested content or tags"));
        assert_eq!(
            fixture.trace().len(),
            1,
            "must reject before verify/publication"
        );
    }
}

#[tokio::test]
async fn subprocess_and_parse_errors_do_not_echo_credentials() {
    for mode in ["nonzero_secret", "malformed_secret"] {
        let fixture = Fixture::new(mode);
        let error = fixture
            .client()
            .sign_kind1("message", tags())
            .await
            .unwrap_err();
        assert!(
            !format!("{error:#}").contains(TEST_KEY),
            "{mode}: {error:#}"
        );
    }
}

#[tokio::test]
async fn excessive_stdout_and_stderr_fail_before_accepting_the_event() {
    for mode in ["stdout_oversize", "stderr_oversize"] {
        let fixture = Fixture::new(mode);
        let error = timeout(
            Duration::from_secs(3),
            fixture.client().sign_kind1("message", tags()),
        )
        .await
        .expect("bounded process handling should finish")
        .unwrap_err();
        assert!(
            format!("{error:#}").contains("output exceeds"),
            "{mode}: {error:#}"
        );
    }
}

#[tokio::test]
async fn changed_publish_echo_is_not_treated_as_delivery_success() {
    let fixture = Fixture::new("mutated_publish");
    let client = fixture.client();
    let event = client.sign_kind1("message", tags()).await.unwrap();
    let error = client.publish_signed(&event).await.unwrap_err();
    assert!(format!("{error:#}").contains("different persisted event"));
}

#[tokio::test]
async fn failed_signature_verification_never_reaches_publication() {
    let fixture = Fixture::new("verify_failure");
    assert!(
        fixture
            .client()
            .sign_kind1("message", tags())
            .await
            .is_err()
    );
    assert_eq!(fixture.trace().len(), 2);
}

#[tokio::test]
async fn signing_failure_preserves_source_cursor_and_empty_outbox() {
    let fixture = Fixture::new("mutated_content");
    let ledger = LedgerStore::connect(&fixture.database()).await.unwrap();
    ledger
        .append_event(
            "payment_received",
            &json!({
                "amount_sats":250, "feed_credit_sats":250, "address_user":"herd"
            }),
        )
        .await
        .unwrap();
    assert!(
        process_next_message(
            &ledger,
            Some(&fixture.client()),
            &MessageRenderer::embedded().unwrap(),
            1000,
            RuntimeMode::Active
        )
        .await
        .is_err()
    );
    assert_eq!(ledger.message_cursor().await.unwrap(), 0);
    assert!(ledger.next_outbox_entry().await.unwrap().is_none());
}

#[tokio::test]
async fn informational_events_never_invoke_the_signer() {
    let fixture = Fixture::new("success");
    let ledger = LedgerStore::connect(&fixture.database()).await.unwrap();
    let renderer = MessageRenderer::embedded().unwrap();
    for (kind, payload) in [
        ("interface_info", json!({})),
        ("weather_status", json!({"message":"Sunny at the goats."})),
    ] {
        ledger.append_event(kind, &payload).await.unwrap();
        process_next_message(
            &ledger,
            Some(&fixture.client()),
            &renderer,
            1000,
            RuntimeMode::Active,
        )
        .await
        .unwrap();
    }
    assert!(fixture.trace().is_empty());
    assert!(ledger.next_outbox_entry().await.unwrap().is_none());
    assert_eq!(ledger.message_cursor().await.unwrap(), 2);
}

#[tokio::test]
async fn failed_publication_reopens_and_retries_exact_persisted_event_without_resigning() {
    let fixture = Fixture::new("publish_fail_once");
    let database = fixture.database();
    let ledger = LedgerStore::connect(&database).await.unwrap();
    ledger
        .append_event(
            "payment_received",
            &json!({
                "amount_sats":250, "feed_credit_sats":250, "address_user":"dexter"
            }),
        )
        .await
        .unwrap();
    process_next_message(
        &ledger,
        Some(&fixture.client()),
        &MessageRenderer::embedded().unwrap(),
        1000,
        RuntimeMode::Active,
    )
    .await
    .unwrap();
    let saved = ledger.next_outbox_entry().await.unwrap().unwrap();
    let worker = tokio::spawn(run_outbox_publisher(ledger.clone(), fixture.client()));
    timeout(Duration::from_secs(5), async {
        loop {
            let entry = ledger.next_outbox_entry().await.unwrap().unwrap();
            if entry.attempts == 1 {
                break;
            }
            sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    worker.abort();
    let _ = worker.await;
    drop(ledger);

    let ledger = LedgerStore::connect(&database).await.unwrap();
    let recovered = ledger.next_outbox_entry().await.unwrap().unwrap();
    assert_eq!(saved.signed_event_json, recovered.signed_event_json);
    assert_eq!(saved.event_id, recovered.event_id);
    let worker = tokio::spawn(run_outbox_publisher(ledger.clone(), fixture.client()));
    timeout(Duration::from_secs(5), async {
        while ledger.next_outbox_entry().await.unwrap().is_some() {
            sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    worker.abort();
    let _ = worker.await;
    let calls = fixture.trace();
    assert_eq!(calls.iter().filter(|c| c["phase"] == "sign").count(), 1);
    let publications: Vec<_> = calls.iter().filter(|c| c["phase"] == "publish").collect();
    assert_eq!(publications.len(), 2);
    assert_eq!(publications[0]["payload"], publications[1]["payload"]);
    assert_eq!(publications[0]["payload"]["id"], saved.event_id);
}

#[tokio::test]
async fn below_minimum_never_signs_and_operator_credit_uses_normal_outbox() {
    let fixture = Fixture::new("success");
    let ledger = LedgerStore::connect(&fixture.database()).await.unwrap();
    let renderer = MessageRenderer::embedded().unwrap();
    for kind in ["payment_received", "operator_credit_added"] {
        ledger
            .append_event(
                kind,
                &json!({"amount_sats":9,"feed_credit_sats":9,"address_user":"herd"}),
            )
            .await
            .unwrap();
        assert!(matches!(
            process_next_message(
                &ledger,
                Some(&fixture.client()),
                &renderer,
                1000,
                RuntimeMode::Active
            )
            .await
            .unwrap(),
            lightning_goats::messaging::MessageProcessorStep::NonPublicSkipped { .. }
        ));
    }
    assert!(fixture.trace().is_empty());
    assert!(ledger.next_outbox_entry().await.unwrap().is_none());
    ledger
        .append_event(
            "operator_credit_added",
            &json!({"amount_sats":10,"feed_credit_sats":19,"address_user":"dexter"}),
        )
        .await
        .unwrap();
    process_next_message(
        &ledger,
        Some(&fixture.client()),
        &renderer,
        1000,
        RuntimeMode::Active,
    )
    .await
    .unwrap();
    let outbox = ledger.next_outbox_entry().await.unwrap().unwrap();
    let signed: Value = serde_json::from_str(&outbox.signed_event_json).unwrap();
    let message = signed["content"].as_str().unwrap();
    assert!(message.contains("10"));
    assert!(!message.contains("TEST"));
    assert!(!message.contains("operator"));
    assert_eq!(
        fixture
            .trace()
            .iter()
            .filter(|call| call["phase"] == "sign")
            .count(),
        1
    );
}

#[tokio::test]
async fn feeding_funding_payment_signs_only_confirmed_feeding_across_restart() {
    let fixture = Fixture::new("success");
    let url = fixture.database();
    let ledger = LedgerStore::connect(&url).await.unwrap();
    ledger
        .grant_operator_credit(
            &lightning_goats::ledger::OperatorCreditGrant {
                id: "initial-partial-credit".into(),
                amount_sats: 305,
                address_user: "herd".into(),
                reason: "Isolated messaging regression fixture".into(),
            },
            false,
        )
        .await
        .unwrap();
    ledger.advance_message_cursor(1).await.unwrap();
    ledger
        .update_feeding_presentation(
            1000,
            RuntimeMode::Active,
            Some(lightning_goats::gateway::FeederSafety {
                override_enabled: false,
                remote_enabled: true,
            }),
        )
        .await
        .unwrap();
    ledger
        .record_payment(&lightning_goats::domain::payment::SettledPayment {
            source: "strike".into(),
            source_id: "isolated-funding-payment".into(),
            payment_hash: None,
            address_user: "herd".into(),
            credit_pool: "herd".into(),
            amount_msat: 1_000_000,
            settled_at: None,
            context_json: None,
        })
        .await
        .unwrap();
    let renderer = MessageRenderer::embedded().unwrap();
    assert_eq!(
        process_next_message(&ledger, None, &renderer, 1000, RuntimeMode::Active)
            .await
            .unwrap(),
        lightning_goats::messaging::MessageProcessorStep::NonPublicSkipped { seq: 2 }
    );
    assert!(ledger.next_outbox_entry().await.unwrap().is_none());
    assert_eq!(ledger.feed_credit_sats().await.unwrap(), 1305);
    // An intent alone is not a success announcement and consumes no credit.
    let attempt = ledger.begin_feed_attempt(1000).await.unwrap().unwrap();
    assert_eq!(
        process_next_message(&ledger, None, &renderer, 1000, RuntimeMode::Active)
            .await
            .unwrap(),
        lightning_goats::messaging::MessageProcessorStep::Idle
    );
    assert_eq!(ledger.feed_credit_sats().await.unwrap(), 1305);
    ledger.confirm_feed_attempt(attempt).await.unwrap();
    drop(ledger);
    let ledger = LedgerStore::connect(&url).await.unwrap();
    assert_eq!(
        process_next_message(
            &ledger,
            Some(&fixture.client()),
            &renderer,
            1000,
            RuntimeMode::Active
        )
        .await
        .unwrap(),
        lightning_goats::messaging::MessageProcessorStep::Enqueued { seq: 3 }
    );
    assert_eq!(ledger.feed_credit_sats().await.unwrap(), 305);
    let outbox = ledger.next_outbox_entry().await.unwrap().unwrap();
    let signed: Value = serde_json::from_str(&outbox.signed_event_json).unwrap();
    assert!(signed["content"].as_str().unwrap().contains("695"));
    assert_eq!(
        fixture
            .trace()
            .iter()
            .filter(|call| call["phase"] == "sign")
            .count(),
        1
    );
    assert_eq!(
        process_next_message(&ledger, None, &renderer, 1000, RuntimeMode::Active)
            .await
            .unwrap(),
        lightning_goats::messaging::MessageProcessorStep::Idle
    );
    assert_eq!(
        ledger
            .next_outbox_entry()
            .await
            .unwrap()
            .unwrap()
            .signed_event_json,
        outbox.signed_event_json
    );
}

#[tokio::test]
async fn paused_payment_publication_freezes_queue_context_and_weather_stays_nonpublic() {
    let fixture = Fixture::new("success");
    let ledger = LedgerStore::connect(&fixture.database()).await.unwrap();
    ledger
        .update_feeding_presentation(
            1000,
            RuntimeMode::Active,
            Some(lightning_goats::gateway::FeederSafety {
                override_enabled: true,
                remote_enabled: true,
            }),
        )
        .await
        .unwrap();
    ledger
        .append_event(
            "payment_received",
            &json!({"amount_sats":1000,"feed_credit_sats":2340,"address_user":"herd"}),
        )
        .await
        .unwrap();
    ledger
        .update_feeding_presentation(
            1000,
            RuntimeMode::Active,
            Some(lightning_goats::gateway::FeederSafety {
                override_enabled: false,
                remote_enabled: true,
            }),
        )
        .await
        .unwrap();
    process_next_message(
        &ledger,
        Some(&fixture.client()),
        &MessageRenderer::embedded().unwrap(),
        1000,
        RuntimeMode::Active,
    )
    .await
    .unwrap();
    let signed: Value = serde_json::from_str(
        &ledger
            .next_outbox_entry()
            .await
            .unwrap()
            .unwrap()
            .signed_event_json,
    )
    .unwrap();
    let text = signed["content"].as_str().unwrap();
    assert!(text.contains("Feeding is paused."));
    assert!(text.contains("2 feedings are queued"));
    ledger
        .update_feeding_presentation(
            1000,
            RuntimeMode::Active,
            Some(lightning_goats::gateway::FeederSafety {
                override_enabled: true,
                remote_enabled: true,
            }),
        )
        .await
        .unwrap();
    ledger
        .append_event("weather_status", &json!({"message":"Current weather"}))
        .await
        .unwrap();
    assert!(matches!(
        process_next_message(
            &ledger,
            Some(&fixture.client()),
            &MessageRenderer::embedded().unwrap(),
            1000,
            RuntimeMode::Active
        )
        .await
        .unwrap(),
        lightning_goats::messaging::MessageProcessorStep::NonPublicSkipped { .. }
    ));
    assert_eq!(
        fixture
            .trace()
            .iter()
            .filter(|call| call["phase"] == "sign")
            .count(),
        1
    );
}
