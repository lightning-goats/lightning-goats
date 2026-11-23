//! Real HTTP -> native intent/invoice -> authoritative mock settlement; no workers.
use super::*;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{UnixListener, UnixStream},
};

#[allow(dead_code)]
#[path = "../tests/support/invoices.rs"]
mod invoice_fixture;
#[path = "../tests/support/nostr.rs"]
mod nostr_fixture;

async fn serve_contact_collector(
    listener: UnixListener,
    id: uuid::Uuid,
    creates: Arc<AtomicUsize>,
) {
    for step in 0..3 {
        let (mut stream, _) = listener.accept().await.unwrap();
        let len = stream.read_u32().await.unwrap();
        assert!(len <= 65536);
        let mut data = vec![0; len as usize];
        stream.read_exact(&mut data).await.unwrap();
        let body: serde_json::Value = serde_json::from_slice(&data).unwrap();
        let reply = if step == 0 {
            assert_eq!(body["contact_id"], id.to_string());
            creates.fetch_add(1, Ordering::SeqCst);
            let commitment = body["description_hash"].as_str().unwrap();
            let created = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap();
            let invoice = invoice_fixture::invoice_with_expiry(
                100_000,
                commitment,
                created,
                [0x22; 32],
                Duration::from_secs(900),
            );
            serde_json::json!({"invoice":invoice})
        } else {
            assert_eq!(body, serde_json::json!({"contact_claim_status":id}));
            serde_json::json!({"request_id":id,"amount_sat":100,"payment_hash":"22".repeat(32),
                "proof_digest":(step==2).then(||"ab".repeat(32))})
        };
        let data = serde_json::to_vec(&reply).unwrap();
        stream
            .write_u32(data.len().try_into().unwrap())
            .await
            .unwrap();
        stream.write_all(&data).await.unwrap();
    }
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
    let id = uuid::Uuid::new_v4();
    let creates = Arc::new(AtomicUsize::new(0));
    let socket = directory.path().join("collector.sock");
    let listener = UnixListener::bind(&socket).unwrap();
    let collector = lightning_goats::config::BreezCollectorConfig {
        socket,
        peer_uid: UnixStream::pair().unwrap().0.peer_cred().unwrap().uid(),
    };
    let provider_task = tokio::spawn(serve_contact_collector(listener, id, creates.clone()));
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
    config.strike = None;
    config.lnurl.invoice_expiry_seconds = 900;
    config.lnurl.wallet_backend = lightning_goats::config::WalletBackend::Breez;
    config.lnurl.breez_collector = Some(collector.clone());
    config.validate().unwrap();
    let origin = config
        .lnurl
        .public_base_url
        .trim_end_matches('/')
        .to_owned();
    let lnurl =
        LnurlService::new(&config.lnurl, &config.lightning_address, ledger.clone()).unwrap();
    let state = AppState {
        cashu: None,
        config: Arc::new(config),
        ledger: ledger.clone(),
        gateway: GatewayClient::new("http://127.0.0.1:9/").unwrap(),
        strike: None,
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
    assert_eq!(creates.load(Ordering::SeqCst), 0);

    for _ in 0..2 {
        let response = request("/api/v1/contact", &body).send().await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()["cache-control"], "no-store");
        let result: serde_json::Value = response.json().await.unwrap();
        assert_eq!(result["state"], "issued");
        assert_eq!(result["fee_sats"], 100);
        assert!(!result.to_string().contains("visitor@example.org"));
    }
    assert_eq!(creates.load(Ordering::SeqCst), 1);
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
    let db = sqlx::SqlitePool::connect(&format!(
        "sqlite://{}",
        directory.path().join("core.db").display()
    ))
    .await
    .unwrap();
    for paid in [false, true] {
        sqlx::query("UPDATE breez_project_invoices SET next_poll=0 WHERE id=?")
            .bind(id.to_string())
            .execute(&db)
            .await
            .unwrap();
        assert!(ledger.poll_breez_claim(&collector).await.unwrap());
        if !paid {
            assert!(ledger.reserve_contact_delivery().await.unwrap().is_none());
        }
    }
    assert!(!ledger.poll_breez_claim(&collector).await.unwrap());
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
    provider_task.await.unwrap();
}
