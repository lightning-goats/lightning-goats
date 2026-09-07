use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};

use axum::{Json, Router, extract::State, routing::post};
use lightning_goats::{
    config::{LightningAddressConfig, LnurlConfig},
    ledger::LedgerStore,
    lnurl::{LnurlService, LnurlServiceError},
    strike::{StrikeClient, StrikeRuntime, StrikeWebhookVerifier},
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tempfile::TempDir;
use tokio::net::TcpListener;

#[derive(Clone)]
struct StrikeMockState {
    calls: Arc<AtomicUsize>,
    last_body: Arc<Mutex<Option<Value>>>,
}

async fn create_receive_request(
    State(state): State<StrikeMockState>,
    Json(body): Json<Value>,
) -> Json<Value> {
    state.calls.fetch_add(1, Ordering::SeqCst);
    *state.last_body.lock().unwrap() = Some(body.clone());
    let amount = body["bolt11"]["amount"]["amount"]
        .as_str()
        .unwrap()
        .to_owned();
    let description_hash = body["bolt11"]["descriptionHash"]
        .as_str()
        .unwrap()
        .to_owned();
    Json(json!({
        "receiveRequestId": "0191382f-387c-4eec-bc74-980872bfc5e5",
        "created": "2026-09-07T18:00:00Z",
        "targetCurrency": "BTC",
        "bolt11": {
            "invoice": "lnbc10u1lightninggoats-test",
            "requestedAmount": {"amount": amount, "currency": "BTC"},
            "btcAmount": amount,
            "descriptionHash": description_hash,
            "paymentHash": "22".repeat(32)
        }
    }))
}

async fn spawn_strike_mock(state: StrikeMockState) -> String {
    let app = Router::new()
        .route("/v1/receive-requests", post(create_receive_request))
        .with_state(state);
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    format!("http://{address}/")
}

fn addresses() -> Vec<LightningAddressConfig> {
    ["herd", "dexter", "rowan", "cosmo", "newton", "nova"]
        .into_iter()
        .map(|user| LightningAddressConfig {
            user: user.to_owned(),
            display_name: user.to_owned(),
            description: format!("Feed the Lightning Goats via {user}"),
            credit_pool: "herd".to_owned(),
            min_sendable_msat: 1_000,
            max_sendable_msat: 1_000_000_000,
        })
        .collect()
}

async fn service(state: StrikeMockState) -> (TempDir, LnurlService) {
    let base_url = spawn_strike_mock(state).await;
    let directory = TempDir::new().unwrap();
    let ledger = LedgerStore::connect(&format!(
        "sqlite://{}",
        directory.path().join("lnurl-integration.db").display()
    ))
    .await
    .unwrap();
    let strike = StrikeRuntime::new(
        StrikeClient::new(&base_url, "test-receive-only-key".to_owned()).unwrap(),
        StrikeWebhookVerifier::new("test-webhook-secret".to_owned()).unwrap(),
    );
    let service = LnurlService::new(
        &LnurlConfig {
            public_base_url: "https://lightning-goats.com/".to_owned(),
            invoice_expiry_seconds: 300,
        },
        &addresses(),
        strike,
        ledger,
    )
    .unwrap();
    (directory, service)
}

#[tokio::test]
async fn callback_sends_hash_of_exact_discovery_metadata_to_strike() {
    let state = StrikeMockState {
        calls: Arc::new(AtomicUsize::new(0)),
        last_body: Arc::new(Mutex::new(None)),
    };
    let (_directory, service) = service(state.clone()).await;
    let discovery = service.discovery("dexter").unwrap();
    let expected_hash = hex::encode(Sha256::digest(discovery.metadata.as_bytes()));

    let callback = service.callback("dexter", 1_000_000).await.unwrap();
    assert_eq!(callback.pr, "lnbc10u1lightninggoats-test");
    assert!(callback.routes.is_empty());
    assert_eq!(state.calls.load(Ordering::SeqCst), 1);

    let body = state.last_body.lock().unwrap().clone().unwrap();
    assert_eq!(body["targetCurrency"], "BTC");
    assert_eq!(body["bolt11"]["amount"]["currency"], "BTC");
    assert_eq!(body["bolt11"]["amount"]["amount"], "0.00001000000");
    assert_eq!(body["bolt11"]["descriptionHash"], expected_hash);
}

#[tokio::test]
async fn unknown_and_invalid_amount_requests_never_call_strike() {
    let state = StrikeMockState {
        calls: Arc::new(AtomicUsize::new(0)),
        last_body: Arc::new(Mutex::new(None)),
    };
    let (_directory, service) = service(state.clone()).await;

    assert!(matches!(
        service.callback("attacker", 1_000_000).await,
        Err(LnurlServiceError::UnknownUser)
    ));
    assert!(matches!(
        service.callback("herd", 999).await,
        Err(LnurlServiceError::InvalidAmount(_))
    ));
    assert!(matches!(
        service.callback("herd", 1_001).await,
        Err(LnurlServiceError::InvalidAmount(_))
    ));
    assert_eq!(state.calls.load(Ordering::SeqCst), 0);
    assert!(state.last_body.lock().unwrap().is_none());
}
