//! Real gateway processes; all OpenHAB commands terminate at this loopback mock.
use std::{
    process::{Child, Command, Stdio},
    sync::{Arc, Mutex},
    time::Duration,
};

use axum::{
    Router,
    extract::{Path, State},
    routing::{get, post},
};
use reqwest::Client;
use tempfile::TempDir;
use tokio::net::TcpListener;
use uuid::Uuid;

#[derive(Clone, Default)]
struct Owner {
    commands: Arc<Mutex<Vec<String>>>,
    ack: Arc<Mutex<String>>,
    auto_ack: bool,
}

async fn item(State(owner): State<Owner>, Path(item): Path<String>) -> String {
    match item.as_str() {
        "FeederOverride" => "OFF".into(),
        "LightningGoatsCanaryRemoteEnabled" => "ON".into(),
        "LightningGoatsCanaryAck" => owner.ack.lock().unwrap().clone(),
        _ => "UNDEF".into(),
    }
}

async fn command(State(owner): State<Owner>, body: String) {
    if owner.auto_ack {
        *owner.ack.lock().unwrap() = body.clone();
    }
    owner.commands.lock().unwrap().push(body);
}

async fn mock_owner(owner: Owner) -> (tokio::task::JoinHandle<()>, String) {
    let app = Router::new()
        .route("/rest/items/{item}/state", get(item))
        .route("/rest/items/{item}", post(command))
        .with_state(owner);
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/", listener.local_addr().unwrap());
    (
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() }),
        url,
    )
}

struct Process(Child);
impl Drop for Process {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

async fn gateway(directory: &TempDir, owner_url: &str, index: usize) -> (Process, String) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    drop(listener);
    let mut config: toml::Value =
        toml::from_str(include_str!("../deploy/gateway/config.canary.toml.example")).unwrap();
    config["service"]["listen"] = address.to_string().into();
    config["database"]["url"] =
        format!("sqlite://{}", directory.path().join("gateway.db").display()).into();
    config["openhab"]["url"] = owner_url.into();
    let path = directory.path().join(format!("gateway-{index}.toml"));
    std::fs::write(&path, toml::to_string(&config).unwrap()).unwrap();
    std::fs::write(
        directory.path().join("openhab-token"),
        "synthetic-mock-only",
    )
    .unwrap();
    let mut child = Process(
        Command::new(env!("CARGO_BIN_EXE_lightning-goats-gateway"))
            .arg("--config")
            .arg(path)
            .env("CREDENTIALS_DIRECTORY", directory.path())
            .env("TOKIO_WORKER_THREADS", "2")
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap(),
    );
    let base = format!("http://{address}");
    let client = Client::new();
    for _ in 0..100 {
        assert!(
            child.0.try_wait().unwrap().is_none(),
            "gateway exited during startup"
        );
        if client.get(format!("{base}/healthz")).send().await.is_ok() {
            return (child, base);
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("gateway did not start");
}

#[tokio::test]
async fn distinct_ids_across_processes_timeout_and_restart_never_resend() {
    let owner = Owner::default();
    let (mock, owner_url) = mock_owner(owner.clone()).await;
    let directory = TempDir::new().unwrap();
    let (first, first_url) = gateway(&directory, &owner_url, 1).await;
    let (second, second_url) = gateway(&directory, &owner_url, 2).await;
    let client = Client::new();
    let a = Uuid::new_v4();
    let b = Uuid::new_v4();
    let (a_response, b_response) = tokio::join!(
        client
            .post(format!("{first_url}/v1/feeder/request/{a}"))
            .send(),
        client
            .post(format!("{second_url}/v1/feeder/request/{b}"))
            .send()
    );
    assert!(!a_response.unwrap().status().is_success());
    assert!(!b_response.unwrap().status().is_success());
    assert_eq!(
        owner.commands.lock().unwrap().len(),
        1,
        "distinct UUIDs must share one durable reservation"
    );
    let submitted = owner.commands.lock().unwrap()[0].clone();
    drop(first);
    drop(second);
    let (_restarted, base) = gateway(&directory, &owner_url, 3).await;
    for id in [submitted.clone(), Uuid::new_v4().to_string()] {
        let response = client
            .post(format!("{base}/v1/feeder/request/{id}"))
            .send()
            .await
            .unwrap();
        let value: serde_json::Value = response.json().await.unwrap();
        assert_ne!(value["status"], "confirmed");
    }
    assert_eq!(owner.commands.lock().unwrap().len(), 1);
    *owner.ack.lock().unwrap() = submitted.clone();
    assert!(
        client
            .get(format!("{base}/v1/feeder/request/{submitted}"))
            .send()
            .await
            .unwrap()
            .status()
            .is_success()
    );
    assert_eq!(
        client
            .post(format!("{base}/v1/feeder/request/{submitted}"))
            .send()
            .await
            .unwrap()
            .status(),
        200
    );
    assert_eq!(
        client
            .post(format!("{base}/v1/feeder/request/{}", Uuid::new_v4()))
            .send()
            .await
            .unwrap()
            .status(),
        429
    );
    assert_eq!(owner.commands.lock().unwrap().len(), 1);
    mock.abort();
}

#[tokio::test]
async fn database_failures_before_dispatch_and_after_completion_fail_closed() {
    let owner = Owner {
        auto_ack: true,
        ..Owner::default()
    };
    let (mock, owner_url) = mock_owner(owner.clone()).await;
    let directory = TempDir::new().unwrap();
    let (_gateway, base) = gateway(&directory, &owner_url, 1).await;
    let pool = sqlx::SqlitePool::connect(&format!(
        "sqlite://{}",
        directory.path().join("gateway.db").display()
    ))
    .await
    .unwrap();
    sqlx::query("CREATE TRIGGER fail_intent BEFORE INSERT ON feeder_requests BEGIN SELECT RAISE(FAIL, 'injected disk failure'); END").execute(&pool).await.unwrap();
    let client = Client::new();
    assert!(
        !client
            .post(format!("{base}/v1/feeder/request/{}", Uuid::new_v4()))
            .send()
            .await
            .unwrap()
            .status()
            .is_success()
    );
    assert!(owner.commands.lock().unwrap().is_empty());
    sqlx::query("DROP TRIGGER fail_intent")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("CREATE TRIGGER fail_confirmation BEFORE UPDATE ON feeder_requests BEGIN SELECT RAISE(FAIL, 'injected disk failure'); END").execute(&pool).await.unwrap();
    let id = Uuid::new_v4();
    assert!(
        !client
            .post(format!("{base}/v1/feeder/request/{id}"))
            .send()
            .await
            .unwrap()
            .status()
            .is_success()
    );
    assert_eq!(owner.commands.lock().unwrap().len(), 1);
    assert!(
        !client
            .post(format!("{base}/v1/feeder/request/{}", Uuid::new_v4()))
            .send()
            .await
            .unwrap()
            .status()
            .is_success()
    );
    assert_eq!(
        owner.commands.lock().unwrap().len(),
        1,
        "confirmation write failure retains capacity"
    );
    sqlx::query("DROP TRIGGER fail_confirmation")
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(
        client
            .post(format!("{base}/v1/feeder/request/{id}"))
            .send()
            .await
            .unwrap()
            .status(),
        200
    );
    assert_eq!(owner.commands.lock().unwrap().len(), 1);
    mock.abort();
}

#[tokio::test]
async fn rolling_hour_cap_survives_restart_and_releases_only_expired_capacity() {
    let owner = Owner {
        auto_ack: true,
        ..Owner::default()
    };
    let (mock, owner_url) = mock_owner(owner.clone()).await;
    let directory = TempDir::new().unwrap();
    let (first, _) = gateway(&directory, &owner_url, 1).await;
    let pool = sqlx::SqlitePool::connect(&format!(
        "sqlite://{}",
        directory.path().join("gateway.db").display()
    ))
    .await
    .unwrap();
    for _ in 0..60 {
        sqlx::query("INSERT INTO feeder_requests(request_id,status,created_at,updated_at) VALUES (?, 'acknowledged', unixepoch()-30, unixepoch()-30)")
            .bind(Uuid::new_v4().to_string()).execute(&pool).await.unwrap();
    }
    drop(first);
    let (_second, base) = gateway(&directory, &owner_url, 2).await;
    let client = Client::new();
    assert_eq!(
        client
            .post(format!("{base}/v1/feeder/request/{}", Uuid::new_v4()))
            .send()
            .await
            .unwrap()
            .status(),
        429
    );
    assert!(owner.commands.lock().unwrap().is_empty());
    // Advance one synthetic history row beyond the rolling window, avoiding a
    // wall-clock change on the host. The remaining 59 still consume capacity.
    sqlx::query("UPDATE feeder_requests SET created_at=unixepoch()-3601, updated_at=unixepoch()-3601 WHERE request_id=(SELECT request_id FROM feeder_requests LIMIT 1)")
        .execute(&pool).await.unwrap();
    let id = Uuid::new_v4();
    assert_eq!(
        client
            .post(format!("{base}/v1/feeder/request/{id}"))
            .send()
            .await
            .unwrap()
            .status(),
        200
    );
    assert_eq!(owner.commands.lock().unwrap().len(), 1);
    let events: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM feeder_request_events WHERE request_id=?")
            .bind(id.to_string())
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(
        events, 2,
        "reservation and acknowledgement have durable audit records"
    );
    assert_eq!(
        client
            .post(format!("{base}/v1/feeder/request/{}", Uuid::new_v4()))
            .send()
            .await
            .unwrap()
            .status(),
        429
    );
    assert_eq!(owner.commands.lock().unwrap().len(), 1);
    mock.abort();
}

async fn credited(directory: &TempDir, sats: u64) -> lightning_goats::ledger::LedgerStore {
    let ledger = lightning_goats::ledger::LedgerStore::connect(&format!(
        "sqlite://{}",
        directory.path().join("daemon.db").display()
    ))
    .await
    .unwrap();
    ledger
        .record_payment(&lightning_goats::domain::payment::SettledPayment {
            source: "mock".into(),
            source_id: "synthetic-credit".into(),
            payment_hash: None,
            address_user: "herd".into(),
            credit_pool: "herd".into(),
            amount_msat: sats * 1000,
            settled_at: None,
            context_json: None,
        })
        .await
        .unwrap();
    ledger
}

async fn daemon(directory: &TempDir, gateway_url: &str) -> Process {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    drop(listener);
    let mut config: toml::Value =
        toml::from_str(include_str!("../deploy/config.canary.toml.example")).unwrap();
    config["service"]["listen"] = address.to_string().into();
    config["database"]["url"] =
        format!("sqlite://{}", directory.path().join("daemon.db").display()).into();
    config["strike"]["api_url"] = "https://127.0.0.1:9/".into();
    config["gateway"]["url"] = gateway_url.into();
    config["informational"]["interface_info_enabled"] = false.into();
    config["informational"]["weather_enabled"] = false.into();
    let path = directory.path().join("daemon.toml");
    std::fs::write(&path, toml::to_string(&config).unwrap()).unwrap();
    for name in ["strike-api-key", "strike-webhook-secret"] {
        std::fs::write(directory.path().join(name), "synthetic-mock-only").unwrap();
    }
    let mut process = Process(
        Command::new(env!("CARGO_BIN_EXE_lightning-goatsd"))
            .arg("--config")
            .arg(path)
            .env("CREDENTIALS_DIRECTORY", directory.path())
            .env("TOKIO_WORKER_THREADS", "2")
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap(),
    );
    let client = Client::new();
    for _ in 0..100 {
        assert!(process.0.try_wait().unwrap().is_none());
        if client
            .get(format!("http://{address}/healthz"))
            .send()
            .await
            .is_ok()
        {
            return process;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("daemon startup timed out");
}

async fn wait_credit(ledger: &lightning_goats::ledger::LedgerStore, sats: u64) {
    for _ in 0..300 {
        if ledger.feed_credit_sats().await.unwrap() == sats {
            return;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!(
        "credit did not reach {sats}; actual {}",
        ledger.feed_credit_sats().await.unwrap()
    );
}

#[tokio::test]
async fn shipped_examples_real_daemon_gateway_two_confirmed_commands_leave_340() {
    let owner = Owner {
        auto_ack: true,
        ..Owner::default()
    };
    let (mock, owner_url) = mock_owner(owner.clone()).await;
    let directory = TempDir::new().unwrap();
    let ledger = credited(&directory, 2340).await;
    let (_gateway, base) = gateway(&directory, &owner_url, 1).await;
    let process = daemon(&directory, &base).await;
    wait_credit(&ledger, 340).await;
    drop(process);
    assert_eq!(owner.commands.lock().unwrap().len(), 2);
    let events = ledger.events_after(0, 100).await.unwrap();
    assert_eq!(
        events
            .iter()
            .filter(|e| e.event_type == "feeder_confirmed")
            .count(),
        2
    );
    let _restarted = daemon(&directory, &base).await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(ledger.feed_credit_sats().await.unwrap(), 340);
    assert_eq!(owner.commands.lock().unwrap().len(), 2);
    mock.abort();
}

#[tokio::test]
async fn daemon_restart_and_confirmation_failure_poll_original_uuid_without_resend() {
    let owner = Owner::default();
    let (mock, owner_url) = mock_owner(owner.clone()).await;
    let directory = TempDir::new().unwrap();
    let ledger = credited(&directory, 1000).await;
    let (_gateway, base) = gateway(&directory, &owner_url, 1).await;
    let first = daemon(&directory, &base).await;
    for _ in 0..100 {
        if !owner.commands.lock().unwrap().is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let original = owner.commands.lock().unwrap()[0].clone();
    drop(first); // response/confirmation lost after mock command
    let pool = sqlx::SqlitePool::connect(&format!(
        "sqlite://{}",
        directory.path().join("daemon.db").display()
    ))
    .await
    .unwrap();
    sqlx::query("CREATE TRIGGER fail_debit BEFORE UPDATE ON feed_attempts WHEN NEW.status='confirmed' BEGIN SELECT RAISE(FAIL, 'injected confirmation failure'); END")
        .execute(&pool).await.unwrap();
    let second = daemon(&directory, &base).await;
    *owner.ack.lock().unwrap() = original.clone();
    tokio::time::sleep(Duration::from_secs(6)).await;
    assert_eq!(ledger.feed_credit_sats().await.unwrap(), 1000);
    assert_eq!(
        owner.commands.lock().unwrap().as_slice(),
        &[original.clone()]
    );
    assert_eq!(
        ledger
            .unresolved_feed_attempt()
            .await
            .unwrap()
            .unwrap()
            .id
            .to_string(),
        original
    );
    drop(second);
    sqlx::query("DROP TRIGGER fail_debit")
        .execute(&pool)
        .await
        .unwrap();
    let _third = daemon(&directory, &base).await;
    wait_credit(&ledger, 0).await;
    assert_eq!(owner.commands.lock().unwrap().as_slice(), &[original]);
    assert_eq!(
        ledger
            .events_after(0, 100)
            .await
            .unwrap()
            .iter()
            .filter(|e| e.event_type == "feeder_confirmed")
            .count(),
        1
    );
    mock.abort();
}

#[tokio::test]
async fn lost_rate_refusal_recovers_by_get_and_cooldown_survives_daemon_restart() {
    use lightning_goats::gateway::{FeedOutcome, GatewayClient};
    let owner = Owner {
        auto_ack: true,
        ..Owner::default()
    };
    let (mock, owner_url) = mock_owner(owner.clone()).await;
    let directory = TempDir::new().unwrap();
    let ledger = credited(&directory, 1000).await;
    let (_gateway, base) = gateway(&directory, &owner_url, 1).await;
    let pool = sqlx::SqlitePool::connect(&format!(
        "sqlite://{}",
        directory.path().join("gateway.db").display()
    ))
    .await
    .unwrap();
    for _ in 0..60 {
        sqlx::query("INSERT INTO feeder_requests(request_id,status,created_at,updated_at) VALUES (?, 'acknowledged', unixepoch()-30, unixepoch()-30)")
            .bind(Uuid::new_v4().to_string()).execute(&pool).await.unwrap();
    }
    let id = ledger.begin_feed_attempt(1000).await.unwrap().unwrap();
    let client = GatewayClient::new(&base).unwrap();
    assert_eq!(
        client.request_feed(id).await.unwrap().status,
        FeedOutcome::NotDispatched
    );
    // Discard the response before changing the daemon ledger, then restart.
    let first = daemon(&directory, &base).await;
    for _ in 0..100 {
        if ledger.unresolved_feed_attempt().await.unwrap().is_none() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(ledger.unresolved_feed_attempt().await.unwrap().is_none());
    assert_eq!(ledger.feed_credit_sats().await.unwrap(), 1000);
    drop(first);
    sqlx::query(
        "UPDATE feeder_requests SET updated_at=unixepoch()-3601,created_at=unixepoch()-3601",
    )
    .execute(&pool)
    .await
    .unwrap();
    let _second = daemon(&directory, &base).await;
    tokio::time::sleep(Duration::from_secs(3)).await;
    assert!(
        owner.commands.lock().unwrap().is_empty(),
        "restart must preserve refusal cooldown"
    );
    assert_eq!(
        client.request_feed(id).await.unwrap().status,
        FeedOutcome::NotDispatched,
        "refused UUID is terminal even after capacity frees"
    );
    let daemon_pool = sqlx::SqlitePool::connect(&format!(
        "sqlite://{}",
        directory.path().join("daemon.db").display()
    ))
    .await
    .unwrap();
    sqlx::query("UPDATE feeder_cooldown SET resume_after=unixepoch()-1")
        .execute(&daemon_pool)
        .await
        .unwrap();
    wait_credit(&ledger, 0).await;
    assert_eq!(owner.commands.lock().unwrap().len(), 1);
    assert_ne!(owner.commands.lock().unwrap()[0], id.to_string());
    mock.abort();
}
