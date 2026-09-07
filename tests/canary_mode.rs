use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use axum::{
    Json, Router,
    extract::State,
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
};
use lightning_goats::{
    config::RuntimeMode,
    domain::payment::SettledPayment,
    feeder::{FeedWorkerStep, run_feed_step},
    gateway::{FeederSafety, GatewayClient},
    ledger::{LedgerStore, SettlementOutcome},
};
use tempfile::TempDir;
use tokio::net::TcpListener;

#[derive(Clone)]
struct GatewayState {
    requests: Arc<AtomicUsize>,
}

async fn safety_handler() -> Json<FeederSafety> {
    Json(FeederSafety {
        override_enabled: false,
        remote_enabled: true,
    })
}

async fn feed_handler(State(state): State<GatewayState>) -> impl IntoResponse {
    state.requests.fetch_add(1, Ordering::SeqCst);
    StatusCode::NO_CONTENT
}

#[tokio::test]
async fn canary_mode_drains_harmless_gateway_without_nostr_capability() {
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

    let requests = Arc::new(AtomicUsize::new(0));
    let app = Router::new()
        .route("/v1/feeder/override", get(safety_handler))
        .route("/v1/feeder/request/{request_id}", post(feed_handler))
        .with_state(GatewayState {
            requests: Arc::clone(&requests),
        });
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let gateway = GatewayClient::new(&format!("http://{address}/")).unwrap();

    assert!(matches!(
        run_feed_step(&ledger, &gateway, 1_000, RuntimeMode::Canary)
            .await
            .unwrap(),
        FeedWorkerStep::Fed {
            remaining_sats: 1_340,
            ..
        }
    ));
    assert!(matches!(
        run_feed_step(&ledger, &gateway, 1_000, RuntimeMode::Canary)
            .await
            .unwrap(),
        FeedWorkerStep::Fed {
            remaining_sats: 340,
            ..
        }
    ));

    assert_eq!(requests.load(Ordering::SeqCst), 2);
    assert_eq!(ledger.feed_credit_sats().await.unwrap(), 340);
}
