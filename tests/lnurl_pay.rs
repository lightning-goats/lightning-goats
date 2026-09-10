#[path = "support/invoices.rs"]
mod invoices;

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
    invalid_invoice: bool,
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
    let (whole, fraction) = amount.split_once('.').unwrap();
    assert_eq!(fraction.len(), 11);
    let amount_msat =
        whole.parse::<u64>().unwrap() * 100_000_000_000 + fraction.parse::<u64>().unwrap();
    let invoice = if state.invalid_invoice {
        "lnbc10u1lightninggoats-test".into()
    } else {
        invoices::invoice(amount_msat, &description_hash)
    };
    Json(json!({
        "receiveRequestId": "0191382f-387c-4eec-bc74-980872bfc5e5",
        "created": "2026-09-07T18:00:00Z",
        "targetCurrency": "BTC",
        "bolt11": {
            "invoice": invoice,
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
    [
        "herd",
        "dexter",
        "rowan",
        "cosmo",
        "newton",
        "nova",
        "goat.name",
    ]
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
        invalid_invoice: false,
    };
    let (_directory, service) = service(state.clone()).await;
    let discovery = service.discovery("dexter").unwrap();
    let expected_hash = hex::encode(Sha256::digest(discovery.metadata.as_bytes()));

    let callback = service.callback("dexter", 1_000_000).await.unwrap();
    assert_eq!(
        callback
            .pr
            .parse::<lightning_invoice::Bolt11Invoice>()
            .unwrap()
            .amount_milli_satoshis(),
        Some(1_000_000)
    );
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
        invalid_invoice: false,
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

#[tokio::test]
async fn invalid_provider_invoice_never_leaves_callback_or_enters_issued_store() {
    let state = StrikeMockState {
        calls: Arc::new(AtomicUsize::new(0)),
        last_body: Arc::new(Mutex::new(None)),
        invalid_invoice: true,
    };
    let (directory, service) = service(state.clone()).await;
    assert!(matches!(
        service.callback("goat.name", 1_000_000).await,
        Err(LnurlServiceError::Provider(_))
    ));
    assert_eq!(state.calls.load(Ordering::SeqCst), 1);
    let pool = sqlx::SqlitePool::connect(&format!(
        "sqlite://{}",
        directory.path().join("lnurl-integration.db").display()
    ))
    .await
    .unwrap();
    let issued: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM strike_receive_requests")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(issued, 0);
}

#[tokio::test]
async fn dotted_user_discovery_callback_and_persistence_share_one_identity() {
    let state = StrikeMockState {
        calls: Arc::new(AtomicUsize::new(0)),
        last_body: Arc::new(Mutex::new(None)),
        invalid_invoice: false,
    };
    let (directory, service) = service(state.clone()).await;
    let discovery = service.discovery("goat.name").unwrap();
    assert!(discovery.callback.ends_with("/goat.name/callback"));
    let result = service.callback("goat.name", 1_000_000).await.unwrap();
    let invoice: lightning_invoice::Bolt11Invoice = result.pr.parse().unwrap();
    assert_eq!(invoice.amount_milli_satoshis(), Some(1_000_000));
    let pool = sqlx::SqlitePool::connect(&format!(
        "sqlite://{}",
        directory.path().join("lnurl-integration.db").display()
    ))
    .await
    .unwrap();
    let user: String = sqlx::query_scalar("SELECT address_user FROM strike_receive_requests")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(user, "goat.name");
}

#[tokio::test]
async fn issuance_budget_is_shared_across_users_and_invalid_inputs_do_not_spend_it() {
    let state = StrikeMockState {
        calls: Arc::new(AtomicUsize::new(0)),
        last_body: Arc::new(Mutex::new(None)),
        invalid_invoice: false,
    };
    let (_directory, service) = service(state.clone()).await;
    for _ in 0..40 {
        assert!(matches!(
            service.callback("unknown", 1_000_000).await,
            Err(LnurlServiceError::UnknownUser)
        ));
        assert!(matches!(
            service.callback("herd", 1_001).await,
            Err(LnurlServiceError::InvalidAmount(_))
        ));
    }
    assert_eq!(state.calls.load(Ordering::SeqCst), 0);
    for index in 0..30 {
        // This provider deliberately reuses an ID; conflicting issuance may
        // fail persistence but must still consume the provider-attempt budget.
        let result = service
            .clone()
            .callback(if index % 2 == 0 { "herd" } else { "dexter" }, 1_000_000)
            .await;
        assert!(!matches!(result, Err(LnurlServiceError::Busy)));
    }
    assert!(matches!(
        service.callback("goat.name", 1_000_000).await,
        Err(LnurlServiceError::Busy)
    ));
    assert_eq!(state.calls.load(Ordering::SeqCst), 30);
}

#[test]
fn configured_dot_segments_fail_while_dotted_names_remain_valid() {
    let mut config: lightning_goats::config::AppConfig =
        toml::from_str(include_str!("../deploy/config.canary.toml.example")).unwrap();
    let index = config.lightning_address.len();
    config
        .lightning_address
        .push(config.lightning_address[0].clone());
    for user in [".", ".."] {
        config.lightning_address[index].user = user.into();
        assert!(config.validate().is_err());
    }
    config.lightning_address[index].user = "goat.name".into();
    config.validate().unwrap();
}
