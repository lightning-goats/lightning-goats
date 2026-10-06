//! Local mock MoneroPay only. No chain, wallet, price, payment or physical action.
use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
};
use lightning_goats::monero_bridge::{Bridge, Config, CreateReceive, Snapshot, SyncReport, now};
use serde_json::{Value, json};
use sqlx::Row;
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tempfile::TempDir;
use tokio::task::JoinHandle;
use uuid::Uuid;

const ADDRESS: &str = "84WsptnLmjTYQjm52SMkhQWsepprkcchNguxdyLkURTSW1WLo3tShTnCRvepijbc2X8GAKPGxJK9hfQhLHzoKSxh7y8Yqrg";
#[derive(Default)]
struct Mock {
    creates: AtomicUsize,
    reads: AtomicUsize,
    mode: AtomicUsize,
    request: Mutex<Value>,
    address: Mutex<String>,
    bindings: Mutex<std::collections::BTreeMap<String, Value>>,
    receipts: Mutex<Vec<Value>>,
}
async fn create(State(m): State<Arc<Mock>>, Json(r): Json<Value>) -> Response {
    m.creates.fetch_add(1, Ordering::SeqCst);
    *m.request.lock().unwrap() = r.clone();
    match m.mode.load(Ordering::SeqCst) {
        1 => return StatusCode::SERVICE_UNAVAILABLE.into_response(),
        2 => tokio::time::sleep(Duration::from_secs(2)).await,
        _ => {}
    }
    let configured = m.address.lock().unwrap().clone();
    let address = if configured.is_empty() {
        ADDRESS.to_owned()
    } else {
        configured
    };
    m.bindings
        .lock()
        .unwrap()
        .insert(address.clone(), r.clone());
    Json(json!({"address":if m.mode.load(Ordering::SeqCst)==3 {"not-an-address".to_owned()}else{address}, "amount":r["amount"], "description":r["description"], "created_at":"2026-09-15T00:00:00Z"})).into_response()
}
async fn receive(State(m): State<Arc<Mock>>, Path(a): Path<String>) -> Response {
    m.reads.fetch_add(1, Ordering::SeqCst);
    assert!(m.bindings.lock().unwrap().contains_key(&a));
    match m.mode.load(Ordering::SeqCst) {
        4 => {
            return (
                StatusCode::TEMPORARY_REDIRECT,
                [("location", "http://127.0.0.1:9/steal")],
            )
                .into_response();
        }
        5 => {
            return (
                [("content-type", "application/json")],
                "x".repeat(1024 * 1024 + 1),
            )
                .into_response();
        }
        8 => return ([("content-type", "application/json")], "{bad json").into_response(),
        9 => return StatusCode::TOO_MANY_REQUESTS.into_response(),
        10 => tokio::time::sleep(Duration::from_millis(200)).await,
        _ => {}
    }
    let r = m.bindings.lock().unwrap().get(&a).unwrap().clone();
    let ts = m.receipts.lock().unwrap().clone();
    let total: u64 = ts.iter().map(|t| t["amount"].as_u64().unwrap()).sum();
    let unlocked: u64 = ts
        .iter()
        .filter(|t| t["locked"] == false)
        .map(|t| t["amount"].as_u64().unwrap())
        .sum();
    Json(json!({"amount":{"expected":r["amount"],"covered":{"total":total,"unlocked":unlocked}},"complete":unlocked>=r["amount"].as_u64().unwrap(),"description":if m.mode.load(Ordering::SeqCst)==6 {json!("wrong-intent")}else{r["description"].clone()},"created_at":"2026-09-15T00:00:00Z","transactions":ts})).into_response()
}
async fn health(State(m): State<Arc<Mock>>) -> Response {
    Json(json!({"status":200,"services":{"walletrpc":m.mode.load(Ordering::SeqCst)!=7,"postgresql":true}})).into_response()
}
struct Fixture {
    _dir: TempDir,
    config: Config,
    mock: Arc<Mock>,
    bridge: Bridge,
    task: JoinHandle<()>,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.task.abort();
    }
}
impl Fixture {
    async fn new() -> Self {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        let dir = TempDir::new().unwrap();
        let m = Arc::new(Mock::default());
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = l.local_addr().unwrap().port();
        let app = Router::new()
            .route("/receive", post(create))
            .route("/receive/{address}", get(receive))
            .route("/health", get(health))
            .with_state(m.clone());
        let task = tokio::spawn(async move {
            axum::serve(l, app).await.unwrap();
        });
        let token = dir.path().join("token");
        std::fs::write(&token, "f".repeat(64)).unwrap();
        std::fs::set_permissions(&token, std::fs::Permissions::from_mode(0o600)).unwrap();
        let config = Config {
            listen: "127.0.0.1:18991".parse().unwrap(),
            callback_listen: "127.0.0.1:18992".parse().unwrap(),
            database_url: format!("sqlite://{}/bridge.db", dir.path().display()),
            provider_url: format!("http://127.0.0.1:{port}/"),
            network: "mainnet".to_owned(),
            account_scope: "a".repeat(64),
            api_token_file: token.clone(),
            sync_report_file: dir.path().join("sync"),
            sync_report_owner_uid: std::fs::metadata(token).unwrap().uid(),
            create_enabled: true,
            max_amount_atomic: 10000,
            max_intents: 100,
            max_receipts: 256,
            max_creates_per_hour: 100,
            max_work: 16,
            max_http: 16,
            poll_seconds: 5,
            timeout_seconds: 1,
            freshness_seconds: 30,
            max_wallet_lag: 0,
        };
        Self::report(&config, now(), 100, true);
        let bridge = Bridge::open(config.clone()).await.unwrap();
        Self {
            _dir: dir,
            config,
            mock: m,
            bridge,
            task,
        }
    }
    fn report(c: &Config, at: i64, height: u64, ok: bool) {
        use std::os::unix::fs::PermissionsExt;
        let r = SyncReport {
            version: "monero-sync-v1".to_owned(),
            network: c.network.clone(),
            account_scope: c.account_scope.clone(),
            provider_url: c.provider_url.clone(),
            checked_at: at,
            wallet_height: height,
            daemon_height: height,
            synchronized: ok,
        };
        std::fs::write(&c.sync_report_file, serde_json::to_vec(&r).unwrap()).unwrap();
        std::fs::set_permissions(&c.sync_report_file, std::fs::Permissions::from_mode(0o640))
            .unwrap();
    }
    fn request(&self) -> CreateReceive {
        CreateReceive {
            intent_id: Uuid::new_v4(),
            expected_atomic: 100,
            address_user: "nova".into(),
            expires_at: now() + 600,
        }
    }
    async fn db(&self) -> sqlx::SqlitePool {
        sqlx::SqlitePool::connect(&self.config.database_url)
            .await
            .unwrap()
    }
}
fn receipt(amount: u64, locked: bool) -> Value {
    json!({"amount":amount,"confirmations":if locked{0}else{10},"double_spend_seen":false,"fee":20,"height":if locked{0}else{80},"timestamp":"2000-01-01T00:00:00Z","tx_hash":"1".repeat(64),"unlock_time":0,"locked":locked})
}

#[tokio::test]
async fn creation_is_durable_idempotent_and_reopens_without_provider_post() {
    let f = Fixture::new().await;
    let r = f.request();
    let first = f.bridge.create(r.clone()).await.unwrap();
    assert!(first.ready);
    assert_eq!(first.address.as_deref(), Some(ADDRESS));
    f.mock.mode.store(1, Ordering::SeqCst);
    let b = Bridge::open(f.config.clone()).await.unwrap();
    let second = b.create(r).await.unwrap();
    assert_eq!(first.generation, second.generation);
    assert_eq!(first.address, second.address);
    assert_eq!(f.mock.creates.load(Ordering::SeqCst), 1);
    assert_eq!(
        sqlx::query("SELECT name FROM sqlite_master WHERE name LIKE '%ledger%'")
            .fetch_all(&f.db().await)
            .await
            .unwrap()
            .len(),
        0
    );
}

#[tokio::test]
async fn concurrent_creations_issue_one_post_and_conflicting_replay_fails() {
    let f = Fixture::new().await;
    let r = f.request();
    let mut tasks = Vec::new();
    for _ in 0..8 {
        let b = f.bridge.clone();
        let r = r.clone();
        tasks.push(tokio::spawn(async move { b.create(r).await }));
    }
    for t in tasks {
        t.await.unwrap().unwrap();
    }
    assert_eq!(f.mock.creates.load(Ordering::SeqCst), 1);
    let mut other = r;
    other.expected_atomic += 1;
    assert!(f.bridge.create(other).await.is_err());
}
#[tokio::test]
async fn unknown_create_responses_never_automatically_retry() {
    for mode in [1, 2, 3] {
        let f = Fixture::new().await;
        let r = f.request();
        f.mock.mode.store(mode, Ordering::SeqCst);
        let s = f.bridge.create(r.clone()).await.unwrap();
        assert_eq!(s.state, "creation_unknown");
        assert!(s.address.is_none());
        f.mock.mode.store(0, Ordering::SeqCst);
        let b = Bridge::open(f.config.clone()).await.unwrap();
        b.create(r).await.unwrap();
        b.poll_once().await.unwrap();
        assert_eq!(f.mock.creates.load(Ordering::SeqCst), 1);
    }
}
#[tokio::test]
async fn pending_unlock_preserves_first_seen_and_never_uses_transaction_time() {
    let f = Fixture::new().await;
    let r = f.request();
    *f.mock.receipts.lock().unwrap() = vec![receipt(50, true)];
    let a = f.bridge.create(r.clone()).await.unwrap();
    assert!(a.ready);
    assert!(a.receipts[0].transaction.locked);
    assert!(a.receipts[0].first_seen_at >= now() - 2);
    *f.mock.receipts.lock().unwrap() = vec![receipt(50, false)];
    f.bridge.reconcile(r.intent_id).await.unwrap();
    let b = f.bridge.status(r.intent_id).await.unwrap();
    assert_eq!(a.receipts[0].first_seen_at, b.receipts[0].first_seen_at);
    assert!(!b.receipts[0].transaction.locked);
}
#[tokio::test]
async fn missing_changed_double_spent_and_relocked_history_hold_permanently() {
    for mode in 0..5 {
        let f = Fixture::new().await;
        let r = f.request();
        *f.mock.receipts.lock().unwrap() = vec![receipt(100, false)];
        f.bridge.create(r.clone()).await.unwrap();
        let mut next = receipt(100, false);
        match mode {
            0 => *f.mock.receipts.lock().unwrap() = vec![],
            1 => {
                next["amount"] = json!(101);
                *f.mock.receipts.lock().unwrap() = vec![next];
            }
            2 => {
                next["double_spend_seen"] = json!(true);
                *f.mock.receipts.lock().unwrap() = vec![next];
            }
            3 => *f.mock.receipts.lock().unwrap() = vec![receipt(100, true)],
            _ => *f.mock.receipts.lock().unwrap() = vec![next.clone(), next],
        }
        f.bridge.reconcile(r.intent_id).await.unwrap();
        let s = f.bridge.status(r.intent_id).await.unwrap();
        assert!(!s.ready);
        assert!(s.hold.is_some());
        assert_eq!(s.receipts[0].transaction.amount, 100);
        *f.mock.receipts.lock().unwrap() = vec![receipt(100, false)];
        let b = Bridge::open(f.config.clone()).await.unwrap();
        b.reconcile(r.intent_id).await.unwrap();
        assert!(b.status(r.intent_id).await.unwrap().hold.is_some());
    }
}
#[tokio::test]
async fn transport_failure_never_exposes_an_unverified_address() {
    for mode in [4, 5, 7, 8, 9] {
        let f = Fixture::new().await;
        f.mock.mode.store(mode, Ordering::SeqCst);
        let r = f.request();
        let s = f.bridge.create(r.clone()).await.unwrap();
        assert!(!s.ready);
        assert!(s.address.is_none());
        f.mock.mode.store(0, Ordering::SeqCst);
        f.bridge.reconcile(r.intent_id).await.unwrap();
        let s = f.bridge.status(r.intent_id).await.unwrap();
        assert!(s.ready);
        assert!(s.address.is_some());
        assert_eq!(f.mock.creates.load(Ordering::SeqCst), 1);
    }
}
#[tokio::test]
async fn wrong_receive_binding_holds_without_returning_address() {
    let f = Fixture::new().await;
    f.mock.mode.store(6, Ordering::SeqCst);
    let s = f.bridge.create(f.request()).await.unwrap();
    assert!(!s.ready);
    assert!(s.hold.is_some());
    assert!(s.address.is_none());
}
#[tokio::test]
async fn stale_missing_or_untrusted_sync_evidence_blocks_new_creates_and_readiness() {
    use std::os::unix::fs::PermissionsExt;
    let f = Fixture::new().await;
    let r = f.request();
    f.bridge.create(r.clone()).await.unwrap();
    Fixture::report(&f.config, now() - 100, 100, true);
    assert!(!f.bridge.status(r.intent_id).await.unwrap().ready);
    assert!(f.bridge.create(f.request()).await.is_err());
    Fixture::report(&f.config, now(), 100, false);
    assert!(f.bridge.create(f.request()).await.is_err());
    Fixture::report(&f.config, now() + 60, 100, true);
    assert!(f.bridge.create(f.request()).await.is_err());
    Fixture::report(&f.config, now(), 100, true);
    std::fs::set_permissions(
        &f.config.sync_report_file,
        std::fs::Permissions::from_mode(0o666),
    )
    .unwrap();
    assert!(f.bridge.create(f.request()).await.is_err());
    std::fs::remove_file(&f.config.sync_report_file).unwrap();
    assert!(f.bridge.create(f.request()).await.is_err());
    assert_eq!(f.mock.creates.load(Ordering::SeqCst), 1);
}
#[tokio::test]
async fn wallet_height_regression_holds_and_never_erases_receipts() {
    let f = Fixture::new().await;
    let r = f.request();
    *f.mock.receipts.lock().unwrap() = vec![receipt(100, false)];
    f.bridge.create(r.clone()).await.unwrap();
    Fixture::report(&f.config, now(), 99, true);
    assert!(!f.bridge.status(r.intent_id).await.unwrap().ready);
    f.bridge.reconcile(r.intent_id).await.unwrap();
    assert!(f.bridge.status(r.intent_id).await.unwrap().hold.is_some());
}
#[tokio::test]
async fn creation_budgets_persist_and_disabling_creation_preserves_existing_reads() {
    let mut f = Fixture::new().await;
    f.config.max_intents = 1;
    let b = Bridge::open(f.config.clone()).await.unwrap();
    let r = f.request();
    b.create(r.clone()).await.unwrap();
    assert!(b.create(f.request()).await.is_err());
    f.config.create_enabled = false;
    let b = Bridge::open(f.config.clone()).await.unwrap();
    assert!(b.create(r).await.is_ok());
    assert!(b.create(f.request()).await.is_err());
}
#[tokio::test]
async fn bridge_refuses_financial_database_or_changed_wallet_namespace() {
    let mut f = Fixture::new().await;
    f.config.account_scope = "b".repeat(64);
    assert!(Bridge::open(f.config.clone()).await.is_err());
    let other = format!("sqlite://{}/financial.db", f._dir.path().display());
    let conn = sqlx::sqlite::SqlitePoolOptions::new()
        .connect_with(
            other
                .parse::<sqlx::sqlite::SqliteConnectOptions>()
                .unwrap()
                .create_if_missing(true),
        )
        .await
        .unwrap();
    sqlx::query("CREATE TABLE ledger(secret TEXT)")
        .execute(&conn)
        .await
        .unwrap();
    f.config.database_url = other;
    assert!(Bridge::open(f.config.clone()).await.is_err());
    let n: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM sqlite_master WHERE name='monero_bridge_meta'")
            .fetch_one(&conn)
            .await
            .unwrap();
    assert_eq!(n, 0);
    sqlx::query("CREATE TABLE monero_bridge_meta(id INTEGER)")
        .execute(&conn)
        .await
        .unwrap();
    assert!(Bridge::open(f.config.clone()).await.is_err());
}
#[tokio::test]
async fn callbacks_are_local_coalesced_hints_without_amount_authority() {
    let f = Fixture::new().await;
    let r = f.request();
    let before = f.bridge.create(r.clone()).await.unwrap();
    let cb = f.mock.request.lock().unwrap()["callback_url"]
        .as_str()
        .unwrap()
        .to_owned();
    let token = cb.rsplit('/').next().unwrap();
    assert!(f.bridge.notify(r.intent_id, "bad").await.is_err());
    for _ in 0..20 {
        f.bridge.notify(r.intent_id, token).await.unwrap();
    }
    let after = f.bridge.status(r.intent_id).await.unwrap();
    assert_eq!(before.revision, after.revision);
    assert!(after.receipts.is_empty());
    let dirty: i64 = sqlx::query_scalar("SELECT dirty FROM monero_bridge_intents")
        .fetch_one(&f.db().await)
        .await
        .unwrap();
    assert_eq!(dirty, 1);
}
#[tokio::test]
async fn stale_lease_result_cannot_overwrite_another_worker() {
    let f = Fixture::new().await;
    let r = f.request();
    f.bridge.create(r.clone()).await.unwrap();
    f.mock.mode.store(10, Ordering::SeqCst);
    let baseline = f.mock.reads.load(Ordering::SeqCst);
    let b = f.bridge.clone();
    let id = r.intent_id;
    let task = tokio::spawn(async move { b.reconcile(id).await });
    tokio::time::timeout(Duration::from_secs(2), async {
        while f.mock.reads.load(Ordering::SeqCst) == baseline {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    sqlx::query("UPDATE monero_bridge_intents SET lease='new-owner',revision=55")
        .execute(&f.db().await)
        .await
        .unwrap();
    task.await.unwrap().unwrap();
    assert_eq!(f.bridge.status(id).await.unwrap().revision, 55);
}
#[tokio::test]
async fn binding_write_failure_leaves_unknown_and_never_reposts() {
    let f = Fixture::new().await;
    sqlx::query("CREATE TRIGGER inject BEFORE UPDATE OF address ON monero_bridge_intents BEGIN SELECT RAISE(ABORT,'fault'); END").execute(&f.db().await).await.unwrap();
    let r = f.request();
    let s = f.bridge.create(r.clone()).await.unwrap();
    assert_eq!(s.state, "creation_unknown");
    sqlx::query("DROP TRIGGER inject")
        .execute(&f.db().await)
        .await
        .unwrap();
    f.bridge.create(r).await.unwrap();
    assert_eq!(f.mock.creates.load(Ordering::SeqCst), 1);
}
#[tokio::test]
async fn failed_snapshot_commit_preserves_old_evidence_atomically() {
    let f = Fixture::new().await;
    let r = f.request();
    let a = f.bridge.create(r.clone()).await.unwrap();
    *f.mock.receipts.lock().unwrap() = vec![receipt(100, false)];
    sqlx::query("CREATE TRIGGER inject BEFORE UPDATE OF snapshot ON monero_bridge_intents BEGIN SELECT RAISE(ABORT,'fault'); END").execute(&f.db().await).await.unwrap();
    assert!(f.bridge.reconcile(r.intent_id).await.is_err());
    let b = f.bridge.status(r.intent_id).await.unwrap();
    assert_eq!(a.revision, b.revision);
    assert!(b.receipts.is_empty());
    assert!(!b.ready);
}
#[tokio::test]
async fn http_requires_auth_and_never_proxies_spend_admin_or_callback_paths() {
    let f = Fixture::new().await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let app = f.bridge.router();
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let c = reqwest::Client::builder().no_proxy().build().unwrap();
    assert_eq!(
        c.get(format!("{url}/healthz"))
            .send()
            .await
            .unwrap()
            .status(),
        401
    );
    for path in [
        "/transfer",
        "/balance",
        "/json_rpc",
        "/v1/receives/../../transfer",
        "/callbacks/a/b",
        "/v1/receives?url=http://evil",
    ] {
        let res = c
            .post(format!("{url}{path}"))
            .bearer_auth("f".repeat(64))
            .send()
            .await
            .unwrap();
        assert!(!res.status().is_success());
    }
    let r = f.request();
    let res = c
        .post(format!("{url}/v1/receives"))
        .bearer_auth("f".repeat(64))
        .json(&r)
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200);
    assert_eq!(res.headers()["cache-control"], "no-store");
    let s: Snapshot = res.json().await.unwrap();
    assert!(s.ready);
    let mut bad = serde_json::to_value(f.request()).unwrap();
    bad["callback_url"] = json!("http://attacker");
    assert_eq!(
        c.post(format!("{url}/v1/receives"))
            .bearer_auth("f".repeat(64))
            .json(&bad)
            .send()
            .await
            .unwrap()
            .status(),
        400
    );
    assert_eq!(
        c.delete(format!("{url}/v1/receives/{}", r.intent_id))
            .bearer_auth("f".repeat(64))
            .send()
            .await
            .unwrap()
            .status(),
        405
    );
    server.abort();
}
#[tokio::test]
async fn no_delete_clock_rollback_and_expired_mappings_are_preserved() {
    let f = Fixture::new().await;
    let r = f.request();
    f.bridge.create(r.clone()).await.unwrap();
    let db = f.db().await;
    assert!(
        sqlx::query("DELETE FROM monero_bridge_intents")
            .execute(&db)
            .await
            .is_err()
    );
    sqlx::query("UPDATE monero_bridge_meta SET clock=?")
        .bind(now() + 100)
        .execute(&db)
        .await
        .unwrap();
    assert!(f.bridge.create(f.request()).await.is_err());
    assert!(!f.bridge.status(r.intent_id).await.unwrap().ready);
    let row = sqlx::query("SELECT request,callback_token FROM monero_bridge_intents")
        .fetch_one(&db)
        .await
        .unwrap();
    assert!(!row.get::<String, _>("callback_token").is_empty());
}

#[tokio::test]
async fn same_tx_in_two_distinct_subaddresses_is_preserved_separately() {
    let f = Fixture::new().await;
    *f.mock.receipts.lock().unwrap() = vec![receipt(50, false)];
    let a = f.request();
    let first = f.bridge.create(a.clone()).await.unwrap();
    // Synthetic address: arbitrary public payload, Keccak typo checksum; no wallet.
    *f.mock.address.lock().unwrap()="82TG61jL1Cq2BDNSgrTE9j3X8QQ5Bnbbs4s3SMTX7y416CxUJqrTLW97YsWGEBnhxH8tnYDcX85QRAEhaAzrTSrZ8A56Ye5".into();
    let b = f.request();
    let second = f.bridge.create(b).await.unwrap();
    assert!(second.ready);
    assert_ne!(first.address, second.address);
    assert_eq!(
        first.receipts[0].transaction.tx_hash,
        second.receipts[0].transaction.tx_hash
    );
    f.bridge.reconcile(a.intent_id).await.unwrap();
    assert!(f.bridge.status(a.intent_id).await.unwrap().ready);
}
#[tokio::test]
async fn cancel_after_post_leaves_an_uncertain_reservation_not_a_retry() {
    let f = Fixture::new().await;
    f.mock.mode.store(2, Ordering::SeqCst);
    let r = f.request();
    let b = f.bridge.clone();
    let request = r.clone();
    let task = tokio::spawn(async move { b.create(request).await });
    tokio::time::timeout(Duration::from_secs(2), async {
        while f.mock.creates.load(Ordering::SeqCst) == 0 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    task.abort();
    let _ = task.await;
    f.mock.mode.store(0, Ordering::SeqCst);
    let b = Bridge::open(f.config.clone()).await.unwrap();
    let s = b.create(r).await.unwrap();
    assert_eq!(s.state, "creating");
    assert!(s.address.is_none());
    assert_eq!(f.mock.creates.load(Ordering::SeqCst), 1);
}
#[tokio::test]
async fn config_rejects_remote_proxy_targets_and_weather_collision() {
    let f = Fixture::new().await;
    for url in [
        "http://10.8.0.6:18993/",
        "http://127.0.0.1:5000/",
        "http://localhost:18993/",
        "http://127.0.0.1:18993/transfer",
        "http://127.0.0.1:18993/?url=x",
        "http://user:secret@127.0.0.1:18993/",
    ] {
        let mut c = f.config.clone();
        c.provider_url = url.into();
        assert!(c.validate().is_err());
    }
    let mut c = f.config.clone();
    c.listen = "0.0.0.0:18991".parse().unwrap();
    assert!(c.validate().is_err());
    c = f.config.clone();
    c.callback_listen = c.listen;
    assert!(c.validate().is_err());
    c = f.config.clone();
    c.database_url = "sqlite://:memory:".into();
    assert!(c.validate().is_err());
}
#[tokio::test]
async fn symlinked_sync_or_credential_files_are_rejected() {
    use std::os::unix::fs::symlink;
    let f = Fixture::new().await;
    let moved = f._dir.path().join("old-sync");
    std::fs::rename(&f.config.sync_report_file, &moved).unwrap();
    symlink(&moved, &f.config.sync_report_file).unwrap();
    assert!(f.bridge.create(f.request()).await.is_err());
    let moved = f._dir.path().join("old-token");
    std::fs::rename(&f.config.api_token_file, &moved).unwrap();
    symlink(&moved, &f.config.api_token_file).unwrap();
    assert!(Bridge::open(f.config.clone()).await.is_err());
}
#[tokio::test]
async fn real_binary_starts_and_restarts_against_local_mock_without_recreating() {
    use tokio::process::Command;
    let mut f = Fixture::new().await;
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    f.config.listen = l.local_addr().unwrap();
    drop(l);
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    f.config.callback_listen = l.local_addr().unwrap();
    drop(l);
    let file = f._dir.path().join("binary.toml");
    std::fs::write(&file, toml::to_string(&f.config).unwrap()).unwrap();
    let client = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(2))
        .build()
        .unwrap();
    let r = f.request();
    let mut previous = None;
    for _ in 0..2 {
        let mut child = Command::new(env!("CARGO_BIN_EXE_lightning-goats-monero-bridge"))
            .arg("--config")
            .arg(&file)
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if client
                    .get(format!("http://{}/healthz", f.config.listen))
                    .bearer_auth("f".repeat(64))
                    .send()
                    .await
                    .is_ok()
                {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .unwrap();
        let s: Snapshot = client
            .post(format!("http://{}/v1/receives", f.config.listen))
            .bearer_auth("f".repeat(64))
            .json(&r)
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(s.address.as_deref(), Some(ADDRESS));
        if let Some(g) = previous {
            assert_eq!(g, s.generation);
        }
        previous = Some(s.generation);
        child.kill().await.unwrap();
        child.wait().await.unwrap();
    }
    assert_eq!(f.mock.creates.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn expired_mapping_is_still_polled_and_late_observation_is_not_backdated() {
    let f = Fixture::new().await;
    let mut r = f.request();
    r.expires_at = now() + 3;
    let s = f.bridge.create(r.clone()).await.unwrap();
    assert!(s.ready);
    tokio::time::timeout(Duration::from_secs(5), async {
        while now() < r.expires_at {
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .unwrap();
    *f.mock.receipts.lock().unwrap() = vec![receipt(100, false)];
    sqlx::query("UPDATE monero_bridge_intents SET last_attempt=0")
        .execute(&f.db().await)
        .await
        .unwrap();
    f.bridge.poll_once().await.unwrap();
    let s = f.bridge.create(r.clone()).await.unwrap();
    assert!(s.ready);
    assert_eq!(s.receipts.len(), 1);
    assert!(s.receipts[0].first_seen_at >= r.expires_at);
    assert_eq!(f.mock.creates.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn receipt_ahead_of_independent_sync_sample_waits_without_a_false_permanent_hold() {
    let f = Fixture::new().await;
    let r = f.request();
    f.bridge.create(r.clone()).await.unwrap();
    let mut t = receipt(100, false);
    t["height"] = json!(101);
    *f.mock.receipts.lock().unwrap() = vec![t];
    f.bridge.reconcile(r.intent_id).await.unwrap();
    let stale = f.bridge.status(r.intent_id).await.unwrap();
    assert!(!stale.ready);
    assert!(stale.hold.is_none());
    assert!(stale.receipts.is_empty());
    Fixture::report(&f.config, now(), 111, true);
    f.bridge.reconcile(r.intent_id).await.unwrap();
    let current = f.bridge.status(r.intent_id).await.unwrap();
    assert!(current.ready);
    assert_eq!(current.receipts.len(), 1);
    assert!(current.hold.is_none());
    assert_eq!(f.mock.creates.load(Ordering::SeqCst), 1);
}
