use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use axum::{
    Router,
    extract::State,
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
};
use lightning_goats::{
    config::RuntimeMode,
    feeder::{FeedWorkerStep, run_feed_step},
    ledger::{LedgerStore, PaidInvoice, SettlementOutcome},
    openhab::OpenHabClient,
};
use tempfile::TempDir;
use tokio::net::TcpListener;

#[derive(Clone)]
struct OpenHabState {
    triggers: Arc<AtomicUsize>,
}

async fn override_handler() -> impl IntoResponse {
    (StatusCode::OK, "OFF")
}

async fn trigger_handler(State(state): State<OpenHabState>) -> impl IntoResponse {
    state.triggers.fetch_add(1, Ordering::SeqCst);
    StatusCode::OK
}

#[tokio::test]
async fn canary_mode_drains_test_rule_without_nostr_capability() {
    assert!(RuntimeMode::Canary.feeder_enabled());
    assert!(!RuntimeMode::Canary.nostr_enabled());

    let directory = TempDir::new().unwrap();
    let database = directory.path().join("canary.db");
    let ledger = LedgerStore::connect(&format!("sqlite://{}", database.display()))
        .await
        .unwrap();
    ledger.initialize_cursor(100).await.unwrap();

    let invoice = PaidInvoice {
        pay_index: 101,
        payment_hash: "canary-mode-payment".to_owned(),
        label: Some("clnaddress:v1:herd-canary:550e8400-e29b-41d4-a716-446655440000".to_owned()),
        amount_msat: 2_340_000,
        settled_at: Some(1_700_000_000),
    };
    assert!(matches!(
        ledger
            .record_settlement(&invoice, "herd-canary")
            .await
            .unwrap(),
        SettlementOutcome::Credited { sats: 2_340, .. }
    ));

    let triggers = Arc::new(AtomicUsize::new(0));
    let app = Router::new()
        .route("/rest/items/FeederOverride/state", get(override_handler))
        .route("/rest/rules/canaryCounter/runnow", post(trigger_handler))
        .with_state(OpenHabState {
            triggers: Arc::clone(&triggers),
        });
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let openhab = OpenHabClient::new(
        &format!("http://{address}/"),
        "token".to_owned(),
        "canaryCounter",
        "FeederOverride",
    )
    .unwrap();

    assert!(matches!(
        run_feed_step(&ledger, &openhab, 1_000, RuntimeMode::Canary)
            .await
            .unwrap(),
        FeedWorkerStep::Fed {
            remaining_sats: 1_340,
            ..
        }
    ));
    assert!(matches!(
        run_feed_step(&ledger, &openhab, 1_000, RuntimeMode::Canary)
            .await
            .unwrap(),
        FeedWorkerStep::Fed {
            remaining_sats: 340,
            ..
        }
    ));

    assert_eq!(triggers.load(Ordering::SeqCst), 2);
    assert_eq!(ledger.feed_credit_sats().await.unwrap(), 340);
}
