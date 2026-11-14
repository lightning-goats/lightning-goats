//! Real HTTP -> native intent/invoice -> authoritative mock settlement; no workers.
use super::*;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

#[path = "../tests/support/invoices.rs"]
mod invoice_fixture;
#[path = "../tests/support/nostr.rs"]
mod nostr_fixture;

#[derive(Clone)]
struct Provider {
    creates: Arc<AtomicUsize>,
    paid: Arc<AtomicBool>,
    request_id: uuid::Uuid,
    receive_id: uuid::Uuid,
    invoice: Arc<tokio::sync::Mutex<Option<(String, String)>>>,
}
async fn create_invoice(
    State(p): State<Provider>,
    Json(body): Json<serde_json::Value>,
) -> Json<serde_json::Value> {
    p.creates.fetch_add(1, Ordering::SeqCst);
    assert_eq!(body["bolt11"]["amount"]["amount"], "0.00000100000");
    assert_eq!(body["targetCurrency"], "BTC");
    let hash = body["bolt11"]["descriptionHash"]
        .as_str()
        .unwrap()
        .to_owned();
    let invoice = invoice_fixture::invoice(100_000, &hash);
    *p.invoice.lock().await = Some((hash.clone(), invoice.clone()));
    Json(
        serde_json::json!({"receiveRequestId":p.request_id,"targetCurrency":"BTC","bolt11":{"invoice":invoice,"descriptionHash":hash,"paymentHash":"22".repeat(32),"requestedAmount":{"amount":"0.00000100000","currency":"BTC"},"btcAmount":"0.00000100000"}}),
    )
}
async fn read_invoice(State(p): State<Provider>) -> Json<serde_json::Value> {
    let (hash, invoice) = p.invoice.lock().await.clone().unwrap();
    Json(
        serde_json::json!({"receiveRequestId":p.request_id,"targetCurrency":"BTC","bolt11":{"invoice":invoice,"descriptionHash":hash,"paymentHash":"22".repeat(32),"requestedAmount":{"amount":"0.00000100000","currency":"BTC"},"btcAmount":"0.00000100000"}}),
    )
}
async fn read_receive(State(p): State<Provider>) -> Json<serde_json::Value> {
    let (hash, invoice) = p.invoice.lock().await.clone().unwrap();
    Json(
        serde_json::json!({"items":[{"receiveRequestId":p.request_id,"receiveId":p.receive_id,
        "state":if p.paid.load(Ordering::SeqCst){"COMPLETED"}else{"PENDING"},"type":"LIGHTNING",
        "amountReceived":{"amount":"0.00000100000","currency":"BTC"},
        "amountCredited":{"amount":"0.00000100000","currency":"BTC"},
        "lightning":{"invoice":invoice,"descriptionHash":hash,"paymentHash":"22".repeat(32)}}]}),
    )
}

#[tokio::test]
async fn contact_http_rejects_bad_admission_and_only_authoritative_fee_queues_private_delivery() {
    let directory = tempfile::TempDir::new().unwrap();
    let ledger = LedgerStore::connect(&format!(
        "sqlite://{}",
        directory.path().join("core.db").display()
    ))
    .await
    .unwrap();
    let p = Provider {
        creates: Arc::new(AtomicUsize::new(0)),
        paid: Arc::new(AtomicBool::new(false)),
        request_id: uuid::Uuid::new_v4(),
        receive_id: uuid::Uuid::new_v4(),
        invoice: Arc::new(tokio::sync::Mutex::new(None)),
    };
    let provider_app = Router::new()
        .route("/v1/receive-requests", post(create_invoice))
        .route(
            &format!("/v1/receive-requests/{}", p.request_id),
            get(read_invoice),
        )
        .route(
            &format!("/v1/receive-requests/{}/receives", p.request_id),
            get(read_receive),
        )
        .with_state(p.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let provider_url = format!("http://{}/", listener.local_addr().unwrap());
    let provider_task =
        tokio::spawn(async move { axum::serve(listener, provider_app).await.unwrap() });
    let strike = StrikeRuntime::new(
        lightning_goats::strike::StrikeClient::new(&provider_url, "fixture-receive-key".into())
            .unwrap(),
        lightning_goats::strike::StrikeWebhookVerifier::new("fixture-webhook".into()).unwrap(),
    );
    let now = lightning_goats::cyberherd::calendar::now().unwrap() as u64;
    let recipient = nostr_fixture::signed(3, 1, now, "", vec![]).pubkey;
    let envelope = nostr_fixture::signed(
        2,
        1059,
        now,
        &STANDARD.encode([vec![2_u8], vec![1; 120]].concat()),
        vec![vec!["p".into(), recipient.clone()]],
    );
    let mut config: AppConfig =
        toml::from_str(include_str!("../deploy/config.canary.toml.example")).unwrap();
    config.service.mode = lightning_goats::config::RuntimeMode::Active;
    config.nostr.bunker_pubkey = recipient.clone();
    config.contact = Some(lightning_goats::contact::Config {
        recipient,
        inbox_relays: vec!["wss://relay.example.org".into()],
    });
    config.validate().unwrap();
    let origin = config
        .lnurl
        .public_base_url
        .trim_end_matches('/')
        .to_owned();
    let lnurl = LnurlService::new(
        &config.lnurl,
        &config.lightning_address,
        strike.clone(),
        ledger.clone(),
    )
    .unwrap();
    let state = AppState {
        config: Arc::new(config),
        ledger: ledger.clone(),
        gateway: GatewayClient::new("http://127.0.0.1:9/").unwrap(),
        strike: strike.clone(),
        lnurl,
        renderer: MessageRenderer::embedded().unwrap(),
        status_slots: Arc::new(tokio::sync::Semaphore::new(4)),
        overlay_slots: Arc::new(tokio::sync::Semaphore::new(4)),
        contact_slots: Arc::new(tokio::sync::Semaphore::new(4)),
    };
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move { axum::serve(listener, app(state)).await.unwrap() });
    let client = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(5))
        .build()
        .unwrap();
    let id = uuid::Uuid::new_v4();
    let capability = "ab".repeat(32);
    let body = serde_json::json!({"id":id,"capability":capability,"reply_email":"visitor@example.org","envelope":envelope});
    let request = |path: &str, value: &serde_json::Value| {
        client
            .post(format!("{base}{path}"))
            .header("origin", &origin)
            .json(value)
    };
    let mut invalid = body.clone();
    invalid["reply_email"] = "".into();
    assert_eq!(
        request("/api/v1/contact", &invalid)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::BAD_REQUEST
    );
    invalid = body.clone();
    invalid["fee_sats"] = 1.into();
    assert_eq!(
        request("/api/v1/contact", &invalid)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        request("/api/v1/contact?capability=private", &body)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        client
            .post(format!("{base}/api/v1/contact"))
            .header("origin", "https://wrong.example")
            .json(&body)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(p.creates.load(Ordering::SeqCst), 0);

    for _ in 0..2 {
        let response = request("/api/v1/contact", &body).send().await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()["cache-control"], "no-store");
        let result: serde_json::Value = response.json().await.unwrap();
        assert_eq!(result["state"], "issued");
        assert_eq!(result["fee_sats"], 100);
        assert!(!result.to_string().contains("visitor@example.org"));
    }
    assert_eq!(p.creates.load(Ordering::SeqCst), 1);
    let status = serde_json::json!({"id":id,"capability":capability});
    let wrong = serde_json::json!({"id":id,"capability":"cd".repeat(32)});
    assert_eq!(
        request("/api/v1/contact/status", &wrong)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::NOT_FOUND
    );
    let event = lightning_goats::strike::StrikeCompletedReceiveEvent {
        event_id: uuid::Uuid::new_v4(),
        receive_request_id: p.request_id,
        receive_id: p.receive_id,
    };
    assert!(strike.reconcile_and_credit(&ledger, &event).await.is_err());
    assert!(ledger.reserve_contact_delivery().await.unwrap().is_none());
    p.paid.store(true, Ordering::SeqCst);
    assert_eq!(
        strike.reconcile_and_credit(&ledger, &event).await.unwrap(),
        lightning_goats::ledger::SettlementOutcome::ContactFeeAccepted
    );
    assert_eq!(
        strike.reconcile_and_credit(&ledger, &event).await.unwrap(),
        lightning_goats::ledger::SettlementOutcome::Duplicate
    );
    let result: serde_json::Value = request("/api/v1/contact/status", &status)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(result["state"], "paid");
    assert_eq!(ledger.feed_credit_sats().await.unwrap(), 0);
    assert!(ledger.reserve_contact_delivery().await.unwrap().is_some());
    server.abort();
    provider_task.abort();
}
