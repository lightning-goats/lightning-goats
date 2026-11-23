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

use lightning_goats::{
    config::{
        BreezCollectorConfig, LightningAddressConfig, LnurlConfig, NostrConfig, WalletBackend,
        ZapConfig,
    },
    ledger::LedgerStore,
    lnurl::{LnurlService, LnurlServiceError},
    nostr::NakClient,
    nostr_event::VerifiedEvent,
    zap_receipts::receipt_step,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tempfile::TempDir;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{UnixListener, UnixStream},
};
use uuid::Uuid;

#[derive(Clone, Default)]
struct Provider {
    calls: Arc<AtomicUsize>,
    requests: Arc<Mutex<BTreeMap<Uuid, Value>>>,
    receives: Arc<Mutex<BTreeMap<Uuid, Value>>>,
}

async fn serve_collector(listener: UnixListener, state: Provider) {
    while let Ok((mut stream, _)) = listener.accept().await {
        let len = stream.read_u32().await.unwrap();
        assert!(len <= 65536);
        let mut bytes = vec![0; len as usize];
        stream.read_exact(&mut bytes).await.unwrap();
        let body: Value = serde_json::from_slice(&bytes).unwrap();
        let reply = if let Some(id) = body["claim_status"].as_str() {
            state.receives.lock().unwrap()[&Uuid::parse_str(id).unwrap()].clone()
        } else {
            state.calls.fetch_add(1, Ordering::SeqCst);
            let id: Uuid = serde_json::from_value(body["request_id"].clone()).unwrap();
            let hash: [u8; 32] = Sha256::digest(id.as_bytes()).into();
            let amount = body["amount_sat"].as_u64().unwrap();
            let description = body["description"].as_str().unwrap();
            let invoice = invoices::invoice_with_expiry(
                amount * 1000,
                &hex::encode(Sha256::digest(description)),
                SystemTime::now().duration_since(UNIX_EPOCH).unwrap(),
                hash,
                std::time::Duration::from_secs(900),
            );
            state
                .requests
                .lock()
                .unwrap()
                .insert(id, json!({"invoice":invoice,"intent":body}));
            state.receives.lock().unwrap().insert(id,json!({"request_id":id,"amount_sat":amount,"payment_hash":hex::encode(hash),"proof_digest":hex::encode(Sha256::digest(hash))}));
            json!({"invoice":invoice})
        };
        let encoded = serde_json::to_vec(&reply).unwrap();
        stream
            .write_u32(encoded.len().try_into().unwrap())
            .await
            .unwrap();
        stream.write_all(&encoded).await.unwrap();
    }
}

struct Fixture {
    directory: TempDir,
    database_url: String,
    ledger: LedgerStore,
    service: LnurlService,
    collector: BreezCollectorConfig,
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
        let directory = TempDir::new().unwrap();
        let database_url = format!("sqlite://{}/receive.db", directory.path().display());
        let ledger = LedgerStore::connect(&database_url).await.unwrap();
        let socket = directory.path().join("project.sock");
        let listener = UnixListener::bind(&socket).unwrap();
        let collector = BreezCollectorConfig {
            socket,
            peer_uid: UnixStream::pair().unwrap().0.peer_cred().unwrap().uid(),
        };
        let server = tokio::spawn(serve_collector(listener, provider.clone()));
        let payee = nostr::signed(2, 1, 100, "", vec![]).pubkey;
        let signer = nostr::signed(3, 1, 100, "", vec![]).pubkey;
        let users = ["herd", "dexter", "rowan", "cosmo", "newton", "nova"];
        let config = LnurlConfig {
            public_base_url: "https://lightning-goats.com/".into(),
            invoice_expiry_seconds: 900,
            wallet_backend: WalletBackend::Breez,
            breez_collector: Some(collector.clone()),
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
        let service = LnurlService::new(&config, &addresses, ledger.clone()).unwrap();
        Self {
            directory,
            database_url,
            ledger,
            service,
            collector,
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

    async fn claim(&self, id: Uuid) -> anyhow::Result<bool> {
        let db = sqlx::SqlitePool::connect(&self.database_url).await?;
        sqlx::query("UPDATE breez_project_invoices SET next_poll=0 WHERE id=?")
            .bind(id.to_string())
            .execute(&db)
            .await?;
        db.close().await;
        self.ledger.poll_breez_claim(&self.collector).await
    }

    async fn stored(&self, id: Uuid) -> Value {
        let db = sqlx::SqlitePool::connect(&self.database_url).await.unwrap();
        let raw: String =
            sqlx::query_scalar("SELECT intent FROM breez_project_invoices WHERE id=?")
                .bind(id.to_string())
                .fetch_one(&db)
                .await
                .unwrap();
        db.close().await;
        serde_json::from_str(&raw).unwrap()
    }

    fn issued_id(&self, invoice: &str) -> Uuid {
        *self
            .provider
            .requests
            .lock()
            .unwrap()
            .iter()
            .find(|(_, value)| value["invoice"] == invoice)
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
        assert!(self.claim(id).await.unwrap());
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
    let stored = f.stored(id).await;
    assert_eq!(stored["description"], discovery.metadata);
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
        let stored = f.stored(id).await;
        assert_eq!(stored["user"], user);
        assert_eq!(stored["credit_pool"], "herd");
        assert_eq!(
            f.ledger.zap_request(id).await.unwrap().unwrap().payee(),
            payee
        );
        f.claim(id).await.unwrap();
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
    f.claim(id).await.unwrap();
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
async fn copied_provider_label_and_invoice_context_cannot_stage_a_zap_receipt() {
    let f = Fixture::new(true).await;
    let raw = f.raw_zap();
    let issued = f
        .service
        .callback_with_zap("herd", 10000, Some(&raw), None)
        .await
        .unwrap();
    let id = f.issued_id(&issued.pr);
    let generic = lightning_goats::domain::payment::SettledPayment {
        source: "strike".into(),
        source_id: "separate-native-receipt".into(),
        payment_hash: None,
        address_user: "herd".into(),
        credit_pool: "herd".into(),
        amount_msat: 10000,
        settled_at: None,
        context_json: Some(json!({"receive_request_id":id}).to_string()),
    };
    f.ledger.record_payment(&generic).await.unwrap();
    assert!(f.ledger.reserve_zap_receipt().await.unwrap().is_none());
    f.claim(id).await.unwrap();
    let job = f.ledger.reserve_zap_receipt().await.unwrap().unwrap();
    assert_eq!(job.invoice, issued.pr);
    assert_eq!(job.request.raw_json(), raw);
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
    let stored = f.stored(id).await;
    assert_eq!(
        stored["description_hash"],
        hex::encode(Sha256::digest(raw.as_bytes()))
    );
    assert_eq!(
        f.ledger.zap_request(id).await.unwrap().unwrap().raw_json(),
        raw
    );
    assert!(f.ledger.reserve_zap_receipt().await.unwrap().is_none());
    f.claim(id).await.unwrap();
    assert!(!f.claim(id).await.unwrap());
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
async fn mismatched_mint_claim_cannot_credit_or_attest_a_different_zap_amount() {
    let f = Fixture::new(true).await;
    let response = f
        .service
        .callback_with_zap("herd", 10000, Some(&f.raw_zap()), None)
        .await
        .unwrap();
    let id = f.issued_id(&response.pr);
    f.provider.receives.lock().unwrap().get_mut(&id).unwrap()["amount_sat"] = json!(8);
    assert!(f.claim(id).await.is_err());
    assert_eq!(f.ledger.feed_credit_sats().await.unwrap(), 0);
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
    let terms = f.stored(id).await;
    sqlx::query("CREATE TRIGGER fail_receipt BEFORE INSERT ON zap_receipt_jobs BEGIN SELECT RAISE(ABORT,'simulated disk failure'); END").execute(&db).await.unwrap();
    assert!(f.claim(id).await.is_err());
    assert_eq!(f.ledger.feed_credit_sats().await.unwrap(), 0);
    assert!(f.ledger.events_after(0, 100).await.unwrap().is_empty());
    sqlx::query("DROP TRIGGER fail_receipt")
        .execute(&db)
        .await
        .unwrap();
    f.claim(id).await.unwrap();
    assert_eq!(f.ledger.feed_credit_sats().await.unwrap(), 10);
    assert_eq!(f.stored(id).await, terms);
    let job = f.ledger.reserve_zap_receipt().await.unwrap().unwrap();
    assert_eq!(
        job.request.raw_json(),
        terms["description"].as_str().unwrap()
    );
    assert_eq!(job.invoice, response.pr);
}

#[tokio::test]
async fn breez_callback_preserves_raw_zap_and_claim_recovery_queues_one_receipt() {
    use std::time::Duration;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::{UnixListener, UnixStream};

    let f = Fixture::new(true).await;
    let socket = f.directory.path().join("breez.sock");
    let listener = UnixListener::bind(&socket).unwrap();
    let collector = BreezCollectorConfig {
        socket,
        peer_uid: UnixStream::pair().unwrap().0.peer_cred().unwrap().uid(),
    };
    let config = LnurlConfig {
        public_base_url: "https://lightning-goats.com/".into(),
        invoice_expiry_seconds: 900,
        wallet_backend: WalletBackend::Breez,
        breez_collector: Some(collector.clone()),
        zap: Some(ZapConfig {
            receipt_pubkey: f.signer.clone(),
            payee_pubkeys: BTreeMap::from([("herd".into(), f.payee.clone())]),
            alias_origins: Vec::new(),
        }),
    };
    let address = LightningAddressConfig {
        user: "herd".into(),
        display_name: "Herd".into(),
        description: "Feed the goats".into(),
        credit_pool: "herd".into(),
        min_sendable_msat: 1000,
        max_sendable_msat: 1_000_000_000,
    };
    let service = LnurlService::new(&config, &[address], f.ledger.clone()).unwrap();
    let raw = f.raw_zap();
    let frozen = raw.clone();
    let collector_task = tokio::spawn(async move {
        let mut id = Value::Null;
        for claim_status in [false, true] {
            let (mut stream, _) = listener.accept().await.unwrap();
            let len = stream.read_u32().await.unwrap();
            assert!(len <= 65536);
            let mut bytes = vec![0; len as usize];
            stream.read_exact(&mut bytes).await.unwrap();
            let req: Value = serde_json::from_slice(&bytes).unwrap();
            let response = if claim_status {
                assert_eq!(req, json!({"claim_status":id}));
                json!({"request_id":id,"amount_sat":10,"payment_hash":"22".repeat(32),"proof_digest":"ab".repeat(32)})
            } else {
                assert_eq!(req["description"], frozen);
                assert_eq!(req["zap"], true);
                id = req["request_id"].clone();
                json!({"invoice":invoices::invoice_with_expiry(10000,&hex::encode(Sha256::digest(frozen.as_bytes())),SystemTime::now().duration_since(UNIX_EPOCH).unwrap(),[0x22;32],Duration::from_secs(900))})
            };
            let encoded = serde_json::to_vec(&response).unwrap();
            stream
                .write_u32(encoded.len().try_into().unwrap())
                .await
                .unwrap();
            stream.write_all(&encoded).await.unwrap();
        }
    });
    let issued = service
        .callback_with_zap("herd", 10000, Some(&raw), None)
        .await
        .unwrap();
    assert_eq!(f.provider.calls.load(Ordering::SeqCst), 0);
    assert_eq!(f.ledger.feed_credit_sats().await.unwrap(), 0);
    let ledger = f.ledger.clone();
    let recovery = tokio::spawn(async move { ledger.run_breez_claim_recovery(collector).await });
    let receipt = tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            if let Some(job) = f.ledger.reserve_zap_receipt().await.unwrap() {
                break job;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    recovery.abort();
    collector_task.await.unwrap();
    assert_eq!(receipt.invoice, issued.pr);
    assert_eq!(receipt.request.raw_json(), raw);
    assert_eq!(f.ledger.feed_credit_sats().await.unwrap(), 10);
    assert!(f.ledger.reserve_zap_receipt().await.unwrap().is_none());
}
