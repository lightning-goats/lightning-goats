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
    config["feeder"]["ack_timeout_seconds"] = 1.into();
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
        assert!(
            !client
                .post(format!("{base}/v1/feeder/request/{id}"))
                .send()
                .await
                .unwrap()
                .status()
                .is_success()
        );
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
        204
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
        204
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
        204
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
