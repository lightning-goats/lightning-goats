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
