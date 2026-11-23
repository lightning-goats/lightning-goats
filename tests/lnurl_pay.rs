#[allow(dead_code)]
#[path = "support/invoices.rs"]
mod invoices;

use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};

use lightning_goats::{
    config::{BreezCollectorConfig, LightningAddressConfig, LnurlConfig},
    ledger::LedgerStore,
    lnurl::{LnurlService, LnurlServiceError},
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tempfile::TempDir;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::UnixListener,
};

#[derive(Clone)]
struct CollectorFixture {
    calls: Arc<AtomicUsize>,
    last_body: Arc<Mutex<Option<Value>>>,
    invalid_invoice: bool,
}

async fn serve_collector(listener: UnixListener, state: CollectorFixture) {
    while let Ok((mut stream, _)) = listener.accept().await {
        let len = stream.read_u32().await.unwrap();
        assert!(len <= 65_536);
        let mut bytes = vec![0; len as usize];
        stream.read_exact(&mut bytes).await.unwrap();
        let body: Value = serde_json::from_slice(&bytes).unwrap();
        let index = state.calls.fetch_add(1, Ordering::SeqCst);
        let amount = body["amount_sat"].as_u64().unwrap();
        let description = body["description"].as_str().unwrap();
        let mut hash = [0x22; 32];
        hash[..8].copy_from_slice(&(index as u64).to_be_bytes());
        let commitment = if state.invalid_invoice {
            "unrelated description"
        } else {
            description
        };
        let invoice = invoices::invoice_with_expiry(
            amount * 1000,
            &hex::encode(Sha256::digest(commitment)),
            SystemTime::now().duration_since(UNIX_EPOCH).unwrap(),
            hash,
            Duration::from_secs(900),
        );
        *state.last_body.lock().unwrap() = Some(body);
        let reply = serde_json::to_vec(&json!({"invoice":invoice})).unwrap();
        stream
            .write_u32(reply.len().try_into().unwrap())
            .await
            .unwrap();
        stream.write_all(&reply).await.unwrap();
    }
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

async fn service(state: CollectorFixture) -> (TempDir, LnurlService) {
    let directory = TempDir::new().unwrap();
    let ledger = LedgerStore::connect(&format!(
        "sqlite://{}",
        directory.path().join("lnurl-integration.db").display()
    ))
    .await
    .unwrap();
    let socket = directory.path().join("project.sock");
    let listener = UnixListener::bind(&socket).unwrap();
    tokio::spawn(serve_collector(listener, state));
    let collector = BreezCollectorConfig {
        socket,
        peer_uid: tokio::net::UnixStream::pair()
            .unwrap()
            .0
            .peer_cred()
            .unwrap()
            .uid(),
    };
    let service = LnurlService::new(
        &LnurlConfig {
            public_base_url: "https://lightning-goats.com/".to_owned(),
            invoice_expiry_seconds: 900,
            collector: Some(collector),
            zap: None,
        },
        &addresses(),
        ledger,
    )
    .unwrap();
    (directory, service)
}

#[tokio::test]
async fn callback_commits_to_exact_discovery_metadata_through_collector() {
    let state = CollectorFixture {
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
    assert_eq!(body["user"], "dexter");
    assert_eq!(body["credit_pool"], "herd");
    assert_eq!(body["amount_sat"], 1000);
    assert_eq!(body["description"], discovery.metadata);
    assert_eq!(body["zap"], false);
    let signed = callback
        .pr
        .parse::<lightning_invoice::Bolt11Invoice>()
        .unwrap();
    let lightning_invoice::Bolt11InvoiceDescriptionRef::Hash(commitment) = signed.description()
    else {
        panic!("missing LNURL metadata commitment");
    };
    assert_eq!(commitment.0.to_string(), expected_hash);
}

#[tokio::test]
async fn unknown_and_invalid_amount_requests_never_call_collector() {
    let state = CollectorFixture {
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
async fn signed_invoice_with_wrong_metadata_never_leaves_callback_or_enters_issued_store() {
    let state = CollectorFixture {
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
    let issued: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM breez_project_invoices WHERE invoice IS NOT NULL")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(issued, 0);
}

#[tokio::test]
async fn dotted_user_discovery_callback_and_persistence_share_one_identity() {
    let state = CollectorFixture {
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
    let user: String = sqlx::query_scalar("SELECT json_extract(intent,'$.user') FROM breez_project_invoices WHERE invoice IS NOT NULL")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(user, "goat.name");
}

#[tokio::test]
async fn issuance_budget_is_shared_across_users_and_invalid_inputs_do_not_spend_it() {
    let state = CollectorFixture {
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
        service
            .callback(if index % 2 == 0 { "herd" } else { "dexter" }, 1_000_000)
            .await
            .unwrap();
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
