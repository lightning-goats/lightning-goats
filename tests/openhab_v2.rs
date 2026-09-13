//! V2 is explicit and recovery reads committed receipts without dispatching.
use axum::{
    Json, Router,
    extract::{Path, Query, State},
    routing::{get, post},
};
use chrono::{DateTime, Utc};
use lightning_goats::openhab::{OpenHabClient, OwnerOutcome, OwnerProtocol, TrustedOpenHabConfig};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::SystemTime,
};
use tokio::net::TcpListener;
use uuid::Uuid;

#[derive(Default)]
struct Data {
    ledger: String,
    ack: String,
    rows: Vec<Value>,
    pages: Vec<HashMap<String, String>>,
    commands: Vec<Value>,
    response: Option<Value>,
}
type Shared = Arc<Mutex<Data>>;
async fn state(State(s): State<Shared>, Path(item): Path<String>) -> String {
    let s = s.lock().unwrap();
    match item.as_str() {
        "Ledger" => s.ledger.clone(),
        "Result" => s.ack.clone(),
        "Override" => "OFF".into(),
        "Remote" => "ON".into(),
        _ => "UNDEF".into(),
    }
}
async fn command(State(s): State<Shared>, body: String) {
    s.lock()
        .unwrap()
        .commands
        .push(serde_json::from_str(&body).unwrap());
}
async fn history(State(s): State<Shared>, Query(q): Query<HashMap<String, String>>) -> Json<Value> {
    let mut s = s.lock().unwrap();
    s.pages.push(q.clone());
    if let Some(response) = &s.response {
        return Json(response.clone());
    }
    let page = q["page"].parse::<usize>().unwrap();
    let rows: Vec<_> = s.rows.iter().skip(page * 8).take(8).cloned().collect();
    Json(json!({"name":"Ledger", "datapoints":rows.len().to_string(), "data":rows}))
}
async fn fixture() -> (Shared, TrustedOpenHabConfig, tokio::task::JoinHandle<()>) {
    let s = Arc::new(Mutex::new(Data {
        ledger: ledger(vec![]),
        ack: "NULL".into(),
        ..Default::default()
    }));
    let app = Router::new()
        .route("/rest/items/{item}/state", get(state))
        .route("/rest/items/Request", post(command))
        .route("/rest/persistence/items/Ledger", get(history))
        .with_state(s.clone());
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let config = TrustedOpenHabConfig {
        url: format!("http://{}/", listener.local_addr().unwrap()),
        request_item: "Request".into(),
        ack_item: "Result".into(),
        protocol: OwnerProtocol::FeederRequestV2 {
            ledger_item: "Ledger".into(),
        },
        override_item: "Override".into(),
        remote_enabled_item: "Remote".into(),
        temperature_item: None,
    };
    (
        s,
        config,
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() }),
    )
}
fn receipt(id: Uuid) -> Value {
    let at = DateTime::<Utc>::from(SystemTime::now()) - chrono::Duration::seconds(60);
    json!({"version":"feeder-request-v2","requestId":id.to_string(),"status":"complete","reason":"complete",
        "at":(at-chrono::Duration::seconds(2)).to_rfc3339(),"updatedAt":at.to_rfc3339()})
}
fn ledger(entries: Vec<Value>) -> String {
    json!({"version":"feeder-request-ledger/v2","entries":entries}).to_string()
}
fn row(entry: &Value) -> Value {
    json!({"time":DateTime::parse_from_rfc3339(entry["updatedAt"].as_str().unwrap()).unwrap().timestamp_millis()+100,"state":ledger(vec![entry.clone()])})
}
fn client(c: &TrustedOpenHabConfig) -> OpenHabClient {
    OpenHabClient::new(c, "synthetic-only".into()).unwrap()
}

#[tokio::test]
async fn explicit_configuration_and_request_envelope_preserve_legacy_forms() {
    for protocol in [
        "\"feeder_request_v1\"",
        "\"uuid_canary\"",
        "{ feeder_request_v2 = { ledger_item = \"Ledger\" } }",
    ] {
        let raw = format!(
            "url='http://127.0.0.1:8080/'\nrequest_item='Request'\nack_item='Result'\noverride_item='Override'\nremote_enabled_item='Remote'\nprotocol={protocol}"
        );
        let _: TrustedOpenHabConfig = toml::from_str(&raw).unwrap();
    }
    let (s, mut c, task) = fixture().await;
    let id = Uuid::new_v4();
    client(&c).command_feeder_request(id).await.unwrap();
    let sent = s.lock().unwrap().commands[0].clone();
    assert_eq!(sent.as_object().unwrap().len(), 3);
    assert_eq!(sent["version"], "feeder-request-v2");
    assert_eq!(sent["requestId"], id.to_string());
    assert!(
        (DateTime::<Utc>::from(SystemTime::now())
            - DateTime::parse_from_rfc3339(sent["requestedAt"].as_str().unwrap())
                .unwrap()
                .with_timezone(&Utc))
        .num_seconds()
        .abs()
            < 5
    );
    for bad in ["Request", "Result", "Override", "Remote", "../Ledger"] {
        c.protocol = OwnerProtocol::FeederRequestV2 {
            ledger_item: bad.into(),
        };
        assert!(OpenHabClient::new(&c, "synthetic".into()).is_err());
    }
    task.abort();
}

#[tokio::test]
async fn current_completion_waits_for_commit_then_recovers_without_notification_or_post() {
    let (s, c, task) = fixture().await;
    let id = Uuid::new_v4();
    let e = receipt(id);
    s.lock().unwrap().ledger = ledger(vec![e.clone()]);
    assert_eq!(
        client(&c).feeder_result(id).await.unwrap(),
        OwnerOutcome::Ambiguous
    );
    s.lock().unwrap().rows = vec![row(&e)];
    assert_eq!(
        client(&c).feeder_result(id).await.unwrap(),
        OwnerOutcome::Complete
    );
    // A recreated client has no in-memory receipt and must independently read JDBC.
    assert_eq!(
        client(&c).feeder_result(id).await.unwrap(),
        OwnerOutcome::Complete
    );
    assert!(s.lock().unwrap().commands.is_empty());
    for q in &s.lock().unwrap().pages {
        for (key, value) in [
            ("serviceId", "jdbc"),
            ("boundary", "false"),
            ("itemState", "false"),
            ("displayState", "false"),
            ("pagelength", "8"),
        ] {
            assert_eq!(q[key], value);
        }
    }
    task.abort();
}

#[tokio::test]
async fn notification_alone_pending_and_failed_receipts_never_confirm() {
    let (s, c, task) = fixture().await;
    let id = Uuid::new_v4();
    let mut e = receipt(id);
    e.as_object_mut().unwrap().remove("updatedAt");
    for (status, reason, expected) in [
        ("complete", "complete", OwnerOutcome::Ambiguous),
        ("accepted", "accepted", OwnerOutcome::Pending),
        (
            "failed",
            "completion_persist_uncertain",
            OwnerOutcome::Ambiguous,
        ),
        ("denied", "ledger_full", OwnerOutcome::Rejected),
    ] {
        e["status"] = json!(status);
        e["reason"] = json!(reason);
        s.lock().unwrap().ack = e.to_string();
        assert_eq!(client(&c).feeder_result(id).await.unwrap(), expected);
    }
    e["status"] = json!("complete");
    e["reason"] = json!("execution_error");
    s.lock().unwrap().ack = e.to_string();
    assert!(client(&c).feeder_result(id).await.is_err());
    e["version"] = json!("feeder-request-v1");
    s.lock().unwrap().ack = e.to_string();
    assert!(client(&c).feeder_result(id).await.is_err());
    assert!(s.lock().unwrap().commands.is_empty());
    task.abort();
}

#[tokio::test]
async fn pagination_requires_a_bounded_end_and_rejects_contradictions() {
    let (s, c, task) = fixture().await;
    let id = Uuid::new_v4();
    let e = receipt(id);
    s.lock().unwrap().ledger = ledger(vec![e.clone()]);
    s.lock().unwrap().rows = vec![row(&e); 9];
    assert_eq!(
        client(&c).feeder_result(id).await.unwrap(),
        OwnerOutcome::Complete
    );
    assert_eq!(s.lock().unwrap().pages.len(), 2);
    s.lock().unwrap().rows = vec![row(&e); 64];
    assert!(client(&c).feeder_result(id).await.is_err());
    let mut accepted = e.clone();
    accepted["status"] = json!("accepted");
    accepted["reason"] = json!("accepted");
    accepted.as_object_mut().unwrap().remove("updatedAt");
    let mut conflict = row(&e);
    conflict["state"] = json!(ledger(vec![accepted]));
    s.lock().unwrap().rows = vec![row(&e), conflict];
    assert!(client(&c).feeder_result(id).await.is_err());
    let mut reversed = row(&e);
    reversed["time"] = json!(reversed["time"].as_i64().unwrap() - 1);
    s.lock().unwrap().rows = vec![row(&e), reversed];
    assert!(client(&c).feeder_result(id).await.is_err());
    for response in [
        json!({"name":"Other","datapoints":"1","data":[row(&e)]}),
        json!({"name":"Ledger","datapoints":"2","data":[row(&e)]}),
        json!({"name":"Ledger","datapoints":"1","data":[row(&e)],"extra":true}),
    ] {
        s.lock().unwrap().response = Some(response);
        assert!(client(&c).feeder_result(id).await.is_err());
    }
    assert!(s.lock().unwrap().commands.is_empty());
    task.abort();
}

#[tokio::test]
async fn ledger_limits_versions_duplicate_ids_and_timestamp_conflicts_fail_closed() {
    let (s, c, task) = fixture().await;
    let id = Uuid::new_v4();
    let e = receipt(id);
    let mut wrong = e.clone();
    wrong["version"] = json!("feeder-request-v1");
    let mut missing = e.clone();
    missing.as_object_mut().unwrap().remove("updatedAt");
    let mut backwards = e.clone();
    backwards["updatedAt"] = json!("2020-01-01T00:00:00Z");
    let mut future = e.clone();
    future["updatedAt"] = json!("9999-01-01T00:00:00Z");
    for raw in [
        "NULL".into(),
        ledger(vec![e.clone(), e.clone()]),
        ledger(vec![wrong]),
        ledger(vec![missing]),
        ledger(vec![backwards]),
        ledger(vec![future]),
        "x".repeat(8193),
        ledger((0..33).map(|_| receipt(Uuid::new_v4())).collect()),
    ] {
        s.lock().unwrap().ledger = raw;
        assert!(client(&c).feeder_result(id).await.is_err());
    }
    assert!(s.lock().unwrap().commands.is_empty());
    task.abort();
}

struct Gateway(std::process::Child);
impl Drop for Gateway {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
async fn launch(config: &std::path::Path, credentials: &std::path::Path, url: &str) -> Gateway {
    let child = std::process::Command::new(env!("CARGO_BIN_EXE_lightning-goats-gateway"))
        .args(["--config", config.to_str().unwrap()])
        .env("CREDENTIALS_DIRECTORY", credentials)
        .env("TOKIO_WORKER_THREADS", "2")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let mut process = Gateway(child);
    for _ in 0..100 {
        assert!(process.0.try_wait().unwrap().is_none());
        if reqwest::get(format!("{url}/healthz")).await.is_ok() {
            return process;
        }
        tokio::time::sleep(std::time::Duration::from_millis(30)).await;
    }
    panic!("gateway failed to start");
}
#[tokio::test]
async fn real_gateway_restart_recovers_lost_result_by_get_with_one_total_command() {
    let (s, c, task) = fixture().await;
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("openhab-token"), "synthetic-only").unwrap();
    let socket = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = socket.local_addr().unwrap();
    drop(socket);
    let url = format!("http://{addr}");
    let path = dir.path().join("gateway.toml");
    std::fs::write(
        &path,
        format!(
            r#"
[service]
listen = "{addr}"
[database]
url = "sqlite://{}"
[openhab]
url = "{}"
request_item = "Request"
ack_item = "Result"
protocol = {{ feeder_request_v2 = {{ ledger_item = "Ledger" }} }}
override_item = "Override"
remote_enabled_item = "Remote"
[weather]
url = "http://127.0.0.1:1/get_received_data"
max_stale_seconds = 300
[feeder]
ack_timeout_seconds = 1
ack_poll_milliseconds = 50
min_feed_interval_seconds = 5
max_feeds_per_hour = 60
"#,
            dir.path().join("gateway.db").display(),
            c.url
        ),
    )
    .unwrap();
    let gateway = launch(&path, dir.path(), &url).await;
    let id = Uuid::new_v4();
    let endpoint = format!("{url}/v1/feeder/request/{id}");
    let http = reqwest::Client::new();
    let first = http.post(&endpoint).send().await.unwrap();
    assert_ne!(first.status(), reqwest::StatusCode::OK);
    assert_eq!(s.lock().unwrap().commands.len(), 1);
    drop(gateway);
    let e = receipt(id);
    {
        let mut data = s.lock().unwrap();
        data.ledger = ledger(vec![e.clone()]);
        data.rows = vec![row(&e)];
        data.ack = "NULL".into();
    }
    let restarted = launch(&path, dir.path(), &url).await;
    let response = http.get(&endpoint).send().await.unwrap();
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    let result: Value = response.json().await.unwrap();
    assert_eq!(result["request_id"], id.to_string());
    assert_eq!(result["status"], "confirmed");
    let replay = http.post(&endpoint).send().await.unwrap();
    assert_eq!(replay.status(), reqwest::StatusCode::OK);
    assert_eq!(s.lock().unwrap().commands.len(), 1);
    drop(restarted);
    task.abort();
}
