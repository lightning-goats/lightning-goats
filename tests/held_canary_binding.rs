use axum::{
    Router,
    extract::State,
    routing::{get, post},
};
use lightning_goats::openhab::{OpenHabClient, OwnerProtocol, TrustedOpenHabConfig};
use std::sync::{Arc, Mutex};
use uuid::Uuid;
type MockState = (Arc<Mutex<Vec<String>>>, Arc<Mutex<String>>);

fn config() -> TrustedOpenHabConfig {
    let value: toml::Value = toml::from_str(include_str!(
        "../deploy/gateway/config.held-canary.toml.example"
    ))
    .unwrap();
    value["openhab"].clone().try_into().unwrap()
}
#[test]
fn held_protocol_only_accepts_its_published_generation_pair() {
    let c = config();
    assert!(OpenHabClient::new(&c, "synthetic".into()).is_ok());
    for (request, ack) in [
        ("GoatFeeder_ManualRequest", "GoatFeeder_ManualResult"),
        ("LightningGoatsCanaryRequest", "LightningGoatsCanaryAck"),
        (
            "LightningGoatsHeldCanaryRequest",
            "LightningGoatsHeldCanaryAck",
        ),
        (
            "LightningGoatsHeldCanary2Request",
            "LightningGoatsCanaryAck",
        ),
    ] {
        let mut bad = c.clone();
        bad.request_item = request.into();
        bad.ack_item = ack.into();
        assert!(OpenHabClient::new(&bad, "synthetic".into()).is_err());
    }
    let mut old = c;
    old.protocol = OwnerProtocol::UuidCanary;
    assert!(OpenHabClient::new(&old, "synthetic".into()).is_err());
}
#[tokio::test]
async fn command_is_bare_uuid_and_only_matching_held_receipt_completes() {
    use lightning_goats::openhab::OwnerOutcome;
    let received = Arc::new(Mutex::new(Vec::<String>::new()));
    let ack = Arc::new(Mutex::new("NULL".to_string()));
    async fn command(State(s): State<MockState>, body: String) {
        s.0.lock().unwrap().push(body);
    }
    async fn receipt(State(s): State<MockState>) -> String {
        s.1.lock().unwrap().clone()
    }
    let app = Router::new()
        .route(
            "/rest/items/LightningGoatsHeldCanary2Request",
            post(command),
        )
        .route(
            "/rest/items/LightningGoatsHeldCanary2Ack/state",
            get(receipt),
        )
        .with_state((received.clone(), ack.clone()));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let mut c = config();
    c.url = format!("http://{}/", listener.local_addr().unwrap());
    let task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let client = OpenHabClient::new(&c, "synthetic".into()).unwrap();
    let id = Uuid::new_v4();
    client.command_feeder_request(id).await.unwrap();
    assert_eq!(received.lock().unwrap().as_slice(), [id.to_string()]);
    assert_eq!(
        client.feeder_result(id).await.unwrap(),
        OwnerOutcome::Absent
    );
    *ack.lock().unwrap() = Uuid::new_v4().to_string();
    assert_eq!(
        client.feeder_result(id).await.unwrap(),
        OwnerOutcome::Absent
    );
    *ack.lock().unwrap() = id.to_string();
    assert_eq!(
        client.feeder_result(id).await.unwrap(),
        OwnerOutcome::Complete
    );
    assert_eq!(received.lock().unwrap().len(), 1);
    task.abort();
}
