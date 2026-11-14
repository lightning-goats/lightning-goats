#[path = "support/invoices.rs"]
#[allow(dead_code)]
mod invoices;
#[path = "support/nostr.rs"]
mod nostr;

use std::{
    collections::BTreeMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::{SystemTime, UNIX_EPOCH},
};

use axum::{
    Json, Router,
    extract::{Path, State},
    routing::{get, post},
};
use lightning_goats::{
    config::{LightningAddressConfig, LnurlConfig, NostrConfig, ZapConfig},
    ledger::{LedgerStore, SettlementOutcome},
    lnurl::{LnurlService, LnurlServiceError},
    nostr::NakClient,
    nostr_event::VerifiedEvent,
    strike::{StrikeClient, StrikeCompletedReceiveEvent, StrikeRuntime, StrikeWebhookVerifier},
    zap_receipts::receipt_step,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tempfile::TempDir;
use uuid::Uuid;

#[derive(Clone, Default)]
struct Provider {
    calls: Arc<AtomicUsize>,
    requests: Arc<Mutex<BTreeMap<Uuid, Value>>>,
    receives: Arc<Mutex<BTreeMap<Uuid, Value>>>,
}

async fn create(State(state): State<Provider>, Json(body): Json<Value>) -> Json<Value> {
    state.calls.fetch_add(1, Ordering::SeqCst);
    let id = Uuid::new_v4();
    let receive_id = Uuid::new_v4();
    let hash: [u8; 32] = Sha256::digest(id.as_bytes()).into();
    let amount = body["bolt11"]["amount"]["amount"].as_str().unwrap();
    let (whole, fraction) = amount.split_once('.').unwrap();
    let msat = whole.parse::<u64>().unwrap() * 100_000_000_000 + fraction.parse::<u64>().unwrap();
    let desc = body["bolt11"]["descriptionHash"].as_str().unwrap();
    let invoice = invoices::invoice_with_payment_hash(
        msat,
        desc,
        SystemTime::now().duration_since(UNIX_EPOCH).unwrap(),
        hash,
    );
    let request = json!({"receiveRequestId":id,"created":"2026-11-09T00:00:00Z","targetCurrency":"BTC","bolt11":{"invoice":invoice,"requestedAmount":{"amount":amount,"currency":"BTC"},"btcAmount":amount,"descriptionHash":desc,"paymentHash":hex::encode(hash)}});
    let receive = json!({"receiveId":receive_id,"receiveRequestId":id,"state":"COMPLETED","type":"LIGHTNING","amountReceived":{"amount":amount,"currency":"BTC"},"lightning":{"invoice":invoice,"paymentHash":hex::encode(hash),"descriptionHash":desc}});
    state.requests.lock().unwrap().insert(id, request.clone());
    state.receives.lock().unwrap().insert(id, receive);
    Json(request)
}

async fn request(Path(id): Path<Uuid>, State(state): State<Provider>) -> Json<Value> {
    Json(state.requests.lock().unwrap()[&id].clone())
}
async fn receives(Path(id): Path<Uuid>, State(state): State<Provider>) -> Json<Value> {
    Json(json!({"items":[state.receives.lock().unwrap()[&id].clone()]}))
}

struct Fixture {
    directory: TempDir,
    database_url: String,
    ledger: LedgerStore,
    service: LnurlService,
    runtime: StrikeRuntime,
    provider: Provider,
    server: tokio::task::JoinHandle<()>,
    payee: String,
    signer: String,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.server.abort();
    }
}

impl Fixture {
    async fn new(zaps: bool) -> Self {
        let provider = Provider::default();
        let app = Router::new()
            .route("/v1/receive-requests", post(create))
            .route("/v1/receive-requests/{id}", get(request))
            .route("/v1/receive-requests/{id}/receives", get(receives))
            .with_state(provider.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}/", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        let directory = TempDir::new().unwrap();
        let database_url = format!("sqlite://{}/receive.db", directory.path().display());
        let ledger = LedgerStore::connect(&database_url).await.unwrap();
        let runtime = StrikeRuntime::new(
            StrikeClient::new(&base, "synthetic-receive-only-key".into()).unwrap(),
            StrikeWebhookVerifier::new("synthetic-webhook-key".into()).unwrap(),
        );
        let payee = nostr::signed(2, 1, 100, "", vec![]).pubkey;
        let signer = nostr::signed(3, 1, 100, "", vec![]).pubkey;
        let users = ["herd", "dexter", "rowan", "cosmo", "newton", "nova"];
        let config = LnurlConfig {
            public_base_url: "https://lightning-goats.com/".into(),
            invoice_expiry_seconds: 300,
            zap: zaps.then(|| ZapConfig {
                receipt_pubkey: signer.clone(),
                payee_pubkeys: users
                    .iter()
                    .enumerate()
                    .map(|(i, user)| {
                        (
                            user.to_string(),
                            nostr::signed(2 + i as u8, 1, 100, "", vec![]).pubkey,
                        )
                    })
                    .collect::<BTreeMap<_, _>>(),
                alias_origins: vec!["https://lnb.bolverker.com/".into()],
            }),
        };
        let addresses = users
            .into_iter()
            .map(|user| LightningAddressConfig {
                user: user.into(),
                display_name: user.into(),
                description: "Feed the goats".into(),
                credit_pool: "herd".into(),
                min_sendable_msat: 1000,
                max_sendable_msat: 1_000_000_000,
            })
            .collect::<Vec<_>>();
        let service =
            LnurlService::new(&config, &addresses, runtime.clone(), ledger.clone()).unwrap();
        Self {
            directory,
            database_url,
            ledger,
            service,
            runtime,
            provider,
            server,
            payee,
            signer,
        }
    }

    fn raw_zap(&self) -> String {
        let tags = vec![
            vec!["p".into(), self.payee.clone()],
            vec!["amount".into(), "10000".into()],
            vec!["relays".into(), "wss://relay.damus.io/".into()],
            vec!["e".into(), "ab".repeat(32)],
        ];
        serde_json::to_string_pretty(&nostr::signed(1, 9734, 100, "Thank you ⚡", tags)).unwrap()
    }

    fn event(&self, id: Uuid) -> StrikeCompletedReceiveEvent {
        StrikeCompletedReceiveEvent {
            event_id: Uuid::new_v4(),
            receive_request_id: id,
            receive_id: serde_json::from_value(
                self.provider.receives.lock().unwrap()[&id]["receiveId"].clone(),
            )
            .unwrap(),
        }
    }

    fn issued_id(&self, invoice: &str) -> Uuid {
        *self
            .provider
            .requests
            .lock()
            .unwrap()
            .iter()
            .find(|(_, value)| value["bolt11"]["invoice"] == invoice)
            .unwrap()
            .0
    }

    async fn issue_and_credit(&self, raw: &str) -> Uuid {
        let response = self
            .service
            .callback_with_zap("herd", 10000, Some(raw), None)
            .await
            .unwrap();
        let id = self.issued_id(&response.pr);
        assert!(matches!(
            self.runtime
                .reconcile_and_credit(&self.ledger, &self.event(id))
                .await
                .unwrap(),
            SettlementOutcome::Credited { sats: 10, .. }
        ));
        id
    }
}

#[tokio::test]
async fn invalid_requests_unknown_users_and_amounts_never_contact_the_provider() {
    let f = Fixture::new(true).await;
    let raw = f.raw_zap();
    assert!(matches!(
        f.service
            .callback_with_zap("unknown", 10000, Some(&raw), None)
            .await,
        Err(LnurlServiceError::UnknownUser)
    ));
    for (amount, text) in [
        (10000, "malformed".to_owned()),
        (11000, raw.clone()),
        (10001, raw.clone()),
        (0, raw.clone()),
        (10000, raw.replace("Thank you", "Different")),
    ] {
        assert!(
            f.service
                .callback_with_zap("herd", amount, Some(&text), None)
                .await
                .is_err()
        );
    }
    let other = serde_json::to_string(&nostr::signed(
        1,
        9734,
        100,
        "",
        vec![
            vec!["p".into(), f.signer.clone()],
            vec!["relays".into(), "wss://relay.damus.io/".into()],
        ],
    ))
    .unwrap();
    assert!(
        f.service
            .callback_with_zap("herd", 10000, Some(&other), None)
            .await
            .is_err()
    );
    let wrong_lnurl = bitcoin::bech32::encode::<bitcoin::bech32::Bech32>(
        bitcoin::bech32::Hrp::parse("lnurl").unwrap(),
        b"https://lightning-goats.com/.well-known/lnurlp/dexter",
    )
    .unwrap();
    assert!(
        f.service
            .callback_with_zap("herd", 10000, Some(&raw), Some(&wrong_lnurl))
            .await
            .is_err()
    );
    assert_eq!(f.provider.calls.load(Ordering::SeqCst), 0);
    assert_eq!(f.ledger.feed_credit_sats().await.unwrap(), 0);
}

#[tokio::test]
async fn ordinary_lnurl_keeps_metadata_hash_and_opt_in_discovery_is_explicit() {
    let f = Fixture::new(false).await;
    let discovery = f.service.discovery("herd").unwrap();
    let json = serde_json::to_value(&discovery).unwrap();
    assert!(json.get("allowsNostr").is_none());
    assert!(json.get("nostrPubkey").is_none());
    assert!(matches!(
        f.service
            .callback_with_zap("herd", 10000, Some(&f.raw_zap()), None)
            .await,
        Err(LnurlServiceError::InvalidZap)
    ));
    assert_eq!(f.provider.calls.load(Ordering::SeqCst), 0);
    let response = f.service.callback("herd", 10000).await.unwrap();
    let id = f.issued_id(&response.pr);
    let stored = f.ledger.strike_receive_request(id).await.unwrap().unwrap();
    assert_eq!(
        stored.description_hash,
        hex::encode(Sha256::digest(discovery.metadata.as_bytes()))
    );
    assert!(f.ledger.zap_request(id).await.unwrap().is_none());
    let enabled = Fixture::new(true).await;
    let discovery = serde_json::to_value(enabled.service.discovery("herd").unwrap()).unwrap();
    assert_eq!(discovery["allowsNostr"], true);
    assert_eq!(discovery["nostrPubkey"], enabled.signer);
}

#[tokio::test]
async fn each_registered_goat_binds_its_payee_and_preserves_the_shared_credit_pool() {
    let f = Fixture::new(true).await;
    for (i, user) in ["herd", "dexter", "rowan", "cosmo", "newton", "nova"]
        .into_iter()
        .enumerate()
    {
        let payee = nostr::signed(2 + i as u8, 1, 100, "", vec![]).pubkey;
        let raw = serde_json::to_string(&nostr::signed(
            1,
            9734,
            100,
            "",
            vec![
                vec!["p".into(), payee.clone()],
                vec!["amount".into(), "10000".into()],
                vec!["relays".into(), "wss://relay.damus.io/".into()],
            ],
        ))
        .unwrap();
        let response = f
            .service
            .callback_with_zap(user, 10000, Some(&raw), None)
            .await
            .unwrap();
        let id = f.issued_id(&response.pr);
        let stored = f.ledger.strike_receive_request(id).await.unwrap().unwrap();
        assert_eq!(stored.address_user, user);
        assert_eq!(stored.credit_pool, "herd");
        assert_eq!(
            f.ledger.zap_request(id).await.unwrap().unwrap().payee(),
            payee
        );
        f.runtime
            .reconcile_and_credit(&f.ledger, &f.event(id))
            .await
            .unwrap();
    }
    assert_eq!(f.ledger.feed_credit_sats().await.unwrap(), 60);
    let before = f.provider.calls.load(Ordering::SeqCst);
    assert!(
        f.service
            .callback_with_zap("dexter", 10000, Some(&f.raw_zap()), None)
            .await
            .is_err()
    );
    assert_eq!(f.provider.calls.load(Ordering::SeqCst), before);
}

#[tokio::test]
async fn a_small_zap_has_a_protocol_receipt_without_a_payment_announcement() {
    let f = Fixture::new(true).await;
    let raw = serde_json::to_string(&nostr::signed(
        1,
        9734,
        100,
        "",
        vec![
            vec!["p".into(), f.payee.clone()],
            vec!["amount".into(), "1000".into()],
            vec!["relays".into(), "wss://relay.damus.io/".into()],
        ],
    ))
    .unwrap();
    let response = f
        .service
        .callback_with_zap("herd", 1000, Some(&raw), None)
        .await
        .unwrap();
    let id = f.issued_id(&response.pr);
    f.runtime
        .reconcile_and_credit(&f.ledger, &f.event(id))
        .await
        .unwrap();
    assert_eq!(f.ledger.feed_credit_sats().await.unwrap(), 1);
    let event = f.ledger.events_after(0, 100).await.unwrap().remove(0);
    let rendered = lightning_goats::presentation::MessageRenderer::embedded()
        .unwrap()
        .render(&event, 1000)
        .unwrap();
    assert!(rendered.nostr_content.is_none());
    assert!(rendered.overlay_message.is_none());
    assert!(f.ledger.reserve_zap_receipt().await.unwrap().is_some());
}

#[tokio::test]
async fn exact_request_alias_binding_settlement_duplicates_and_distinct_invoices_are_preserved() {
    let f = Fixture::new(true).await;
    let raw = f.raw_zap();
    let alias = bitcoin::bech32::encode::<bitcoin::bech32::Bech32>(
        bitcoin::bech32::Hrp::parse("lnurl").unwrap(),
        b"https://lnb.bolverker.com/.well-known/lnurlp/herd",
    )
    .unwrap()
    .to_ascii_uppercase();
    let response = f
        .service
        .callback_with_zap("herd", 10000, Some(&raw), Some(&alias))
        .await
        .unwrap();
    let id = f.issued_id(&response.pr);
    let stored = f.ledger.strike_receive_request(id).await.unwrap().unwrap();
    assert_eq!(
        stored.description_hash,
        hex::encode(Sha256::digest(raw.as_bytes()))
    );
    assert_eq!(
        f.ledger.zap_request(id).await.unwrap().unwrap().raw_json(),
        raw
    );
    assert!(f.ledger.reserve_zap_receipt().await.unwrap().is_none());
    f.runtime
        .reconcile_and_credit(&f.ledger, &f.event(id))
        .await
        .unwrap();
    assert_eq!(
        f.runtime
            .reconcile_and_credit(&f.ledger, &f.event(id))
            .await
            .unwrap(),
        SettlementOutcome::Duplicate
    );
    assert_eq!(f.ledger.feed_credit_sats().await.unwrap(), 10);
    let first = f.ledger.reserve_zap_receipt().await.unwrap().unwrap();
    assert_eq!(first.request.raw_json(), raw);
    let second_store = LedgerStore::connect(&f.database_url).await.unwrap();
    assert!(second_store.reserve_zap_receipt().await.unwrap().is_none());
    let second_id = f.issue_and_credit(&raw).await;
    assert_ne!(id, second_id);
    assert_eq!(f.ledger.feed_credit_sats().await.unwrap(), 20);
    let second = second_store.reserve_zap_receipt().await.unwrap().unwrap();
    assert_ne!(first.settlement_id, second.settlement_id);
    assert_ne!(first.invoice, second.invoice);
    assert_eq!(
        first.request.event().event().id,
        second.request.event().event().id
    );
}

#[tokio::test]
async fn signed_receipt_retry_survives_reopen_with_the_exact_same_bytes() {
    let f = Fixture::new(true).await;
    f.issue_and_credit(&f.raw_zap()).await;
    let job = f.ledger.reserve_zap_receipt().await.unwrap().unwrap();
    let signed = VerifiedEvent::verify(nostr::signed(
        3,
        9735,
        job.paid_at,
        "",
        job.request.receipt_tags(&job.invoice).unwrap(),
    ))
    .unwrap();
    let raw = f
        .ledger
        .save_signed_zap_receipt(&job, &signed)
        .await
        .unwrap();
    let mut conflicting = job.clone();
    conflicting.paid_at += 1;
    let different = VerifiedEvent::verify(nostr::signed(
        3,
        9735,
        conflicting.paid_at,
        "",
        conflicting
            .request
            .receipt_tags(&conflicting.invoice)
            .unwrap(),
    ))
    .unwrap();
    assert!(
        f.ledger
            .save_signed_zap_receipt(&conflicting, &different)
            .await
            .is_err()
    );
    f.ledger
        .finish_zap_receipt_step(&job, false, false)
        .await
        .unwrap();
    let db = sqlx::SqlitePool::connect(&f.database_url).await.unwrap();
    sqlx::query("UPDATE zap_receipt_relays SET next_attempt=0")
        .execute(&db)
        .await
        .unwrap();
    db.close().await;
    let reopened = LedgerStore::connect(&f.database_url).await.unwrap();
    let retry = reopened.reserve_zap_receipt().await.unwrap().unwrap();
    assert_eq!(retry.signed_json.as_deref(), Some(raw.as_str()));
    assert_eq!(retry.paid_at, job.paid_at);
    assert_eq!(retry.request.raw_json(), job.request.raw_json());
    reopened
        .finish_zap_receipt_step(&retry, true, false)
        .await
        .unwrap();
    assert!(reopened.reserve_zap_receipt().await.unwrap().is_none());
    assert_eq!(reopened.feed_credit_sats().await.unwrap(), 10);
}

#[tokio::test]
async fn actual_p2p_credit_is_preserved_without_attesting_a_different_zap_amount() {
    let f = Fixture::new(true).await;
    let response = f
        .service
        .callback_with_zap("herd", 10000, Some(&f.raw_zap()), None)
        .await
        .unwrap();
    let id = f.issued_id(&response.pr);
    {
        let mut receives = f.provider.receives.lock().unwrap();
        let value = receives.get_mut(&id).unwrap();
        value["type"] = "P2P".into();
        value["amountReceived"] = json!({"amount":"0.00000008000","currency":"BTC"});
        value["amountCredited"] = json!({"amount":"0.00000008000","currency":"BTC"});
        value.as_object_mut().unwrap().remove("lightning");
    }
    f.runtime
        .reconcile_and_credit(&f.ledger, &f.event(id))
        .await
        .unwrap();
    assert_eq!(f.ledger.feed_credit_sats().await.unwrap(), 8);
    assert!(f.ledger.reserve_zap_receipt().await.unwrap().is_none());
}

#[tokio::test]
async fn unavailable_signer_does_not_undo_credit_or_erase_the_receipt_intent() {
    let f = Fixture::new(true).await;
    f.issue_and_credit(&f.raw_zap()).await;
    let config = NostrConfig {
        nak_path: "/definitely/missing/nak".into(),
        nak_config_path: f.directory.path().join("nak"),
        bunker_pubkey: f.signer.clone(),
        relays: vec!["wss://relay.damus.io/".into()],
        bunker_relays: None,
    };
    let nak = NakClient::new(&config, "synthetic-client-key".into()).unwrap();
    let error = receipt_step(&f.ledger, &nak).await.unwrap_err();
    assert!(!format!("{error:#}").contains("synthetic-client-key"));
    assert_eq!(f.ledger.feed_credit_sats().await.unwrap(), 10);
    let db = sqlx::SqlitePool::connect(&f.database_url).await.unwrap();
    let count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM zap_receipt_jobs WHERE event_id IS NULL AND signing_attempts=1",
    )
    .fetch_one(&db)
    .await
    .unwrap();
    assert_eq!(count, 1);
    assert_eq!(
        f.ledger
            .events_after(0, 100)
            .await
            .unwrap()
            .iter()
            .filter(|event| event.event_type == "payment_received")
            .count(),
        1
    );
}

#[tokio::test]
async fn immutable_invoice_context_and_atomic_receipt_intent_survive_storage_failure() {
    let f = Fixture::new(true).await;
    let response = f
        .service
        .callback_with_zap("herd", 10000, Some(&f.raw_zap()), None)
        .await
        .unwrap();
    let id = f.issued_id(&response.pr);
    let db = sqlx::SqlitePool::connect(&f.database_url).await.unwrap();
    assert!(
        sqlx::query("UPDATE zap_receive_context SET request_json='{}'")
            .execute(&db)
            .await
            .is_err()
    );
    assert!(
        sqlx::query("DELETE FROM zap_receive_context")
            .execute(&db)
            .await
            .is_err()
    );
    sqlx::query("CREATE TRIGGER fail_receipt BEFORE INSERT ON zap_receipt_jobs BEGIN SELECT RAISE(ABORT,'simulated disk failure'); END").execute(&db).await.unwrap();
    assert!(
        f.runtime
            .reconcile_and_credit(&f.ledger, &f.event(id))
            .await
            .is_err()
    );
    assert_eq!(f.ledger.feed_credit_sats().await.unwrap(), 0);
    assert!(f.ledger.events_after(0, 100).await.unwrap().is_empty());
    sqlx::query("DROP TRIGGER fail_receipt")
        .execute(&db)
        .await
        .unwrap();
    f.runtime
        .reconcile_and_credit(&f.ledger, &f.event(id))
        .await
        .unwrap();
    assert_eq!(f.ledger.feed_credit_sats().await.unwrap(), 10);
    assert!(f.ledger.reserve_zap_receipt().await.unwrap().is_some());
}
