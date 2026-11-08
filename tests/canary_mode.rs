use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};

use axum::{
    Json, Router,
    extract::{Path, State},
    response::IntoResponse,
    routing::{get, post},
};
use lightning_goats::{
    config::RuntimeMode,
    domain::payment::SettledPayment,
    feeder::{FeedWorkerStep, run_feed_step},
    gateway::{FeedOutcome, FeedRequestStatus, FeederSafety, GatewayClient},
    ledger::{LedgerStore, SettlementOutcome},
};
use tempfile::TempDir;
use tokio::net::TcpListener;

#[derive(Clone)]
struct GatewayState {
    requests: Arc<AtomicUsize>,
    override_enabled: Arc<AtomicBool>,
}

async fn safety_handler(State(state): State<GatewayState>) -> Json<FeederSafety> {
    Json(FeederSafety {
        override_enabled: state.override_enabled.load(Ordering::SeqCst),
        remote_enabled: true,
    })
}

async fn feed_handler(
    State(state): State<GatewayState>,
    Path(id): Path<uuid::Uuid>,
) -> impl IntoResponse {
    state.requests.fetch_add(1, Ordering::SeqCst);
    Json(FeedRequestStatus {
        request_id: id,
        status: FeedOutcome::Confirmed,
        refusal: None,
    })
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
            override_enabled: Arc::new(AtomicBool::new(false)),
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

#[tokio::test]
async fn operator_credit_respects_override_and_real_worker_debits_without_payment_provider() {
    use lightning_goats::ledger::{OperatorCreditGrant, OperatorCreditStatus};
    let root = TempDir::new().unwrap();
    let ledger = LedgerStore::connect(&format!(
        "sqlite://{}",
        root.path().join("ledger.db").display()
    ))
    .await
    .unwrap();
    let grant = OperatorCreditGrant {
        id: "feeding-check".into(),
        amount_sats: 2340,
        address_user: "herd".into(),
        reason: "Harmless gateway fixture".into(),
    };
    ledger.grant_operator_credit(&grant, false).await.unwrap();
    let requests = Arc::new(AtomicUsize::new(0));
    let gate = Arc::new(AtomicBool::new(true));
    let app = Router::new()
        .route("/v1/feeder/override", get(safety_handler))
        .route("/v1/feeder/request/{request_id}", post(feed_handler))
        .with_state(GatewayState {
            requests: Arc::clone(&requests),
            override_enabled: Arc::clone(&gate),
        });
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let gateway = GatewayClient::new(&format!("http://{address}/")).unwrap();
    assert!(matches!(
        run_feed_step(&ledger, &gateway, 1000, RuntimeMode::Canary)
            .await
            .unwrap(),
        FeedWorkerStep::OverrideBlocked { feeds_due: 2 }
    ));
    assert_eq!(requests.load(Ordering::SeqCst), 0);
    assert_eq!(ledger.feed_credit_sats().await.unwrap(), 2340);
    gate.store(false, Ordering::SeqCst);
    for expected in [1340, 340] {
        assert!(
            matches!(run_feed_step(&ledger,&gateway,1000,RuntimeMode::Canary).await.unwrap(),FeedWorkerStep::Fed{remaining_sats,..} if remaining_sats==expected)
        );
    }
    assert_eq!(requests.load(Ordering::SeqCst), 2);
    assert_eq!(
        ledger
            .grant_operator_credit(&grant, false)
            .await
            .unwrap()
            .status,
        OperatorCreditStatus::Duplicate
    );
    assert_eq!(
        run_feed_step(&ledger, &gateway, 1000, RuntimeMode::Canary)
            .await
            .unwrap(),
        FeedWorkerStep::Idle
    );
    assert_eq!(ledger.feed_credit_sats().await.unwrap(), 340);
    server.abort();
}
