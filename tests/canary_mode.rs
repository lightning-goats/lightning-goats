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
    domain::payment::SettledPayment,
    feeder::{FeedWorkerStep, run_feed_step},
    ledger::{LedgerStore, SettlementOutcome},
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

    let payment = SettledPayment {
        source: "test".to_owned(),
        source_id: "canary-mode-payment".to_owned(),
        payment_hash: Some(
            "ca11a7ca11a7ca11a7ca11a7ca11a7ca11a7ca11a7ca11a7ca11a7ca11a7ca11".to_owned(),
        ),
        address_user: "herd-canary".to_owned(),
        credit_pool: "herd".to_owned(),
        amount_msat: 2_340_000,
        settled_at: Some(1_700_000_000),
        context_json: None,
    };
    assert!(matches!(
        ledger.record_payment(&payment).await.unwrap(),
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
