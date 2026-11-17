//! Real loopback HTTP owner/gateway with disposable stores; no runtime credentials.
use super::*;
use axum::extract::State;
use serde_json::Value;
use sqlx::SqlitePool;
use std::sync::{Arc, Mutex};
use tempfile::TempDir;
use tokio::net::TcpListener;

#[derive(Default)]
struct Owner {
    remote: bool,
    ack: String,
    commands: Vec<String>,
}
type Shared = Arc<Mutex<Owner>>;
async fn item(State(owner): State<Shared>, AxumPath(item): AxumPath<String>) -> String {
    let owner = owner.lock().unwrap();
    match item.as_str() {
        "Remote" => if owner.remote { "ON" } else { "OFF" }.into(),
        "Override" => "OFF".into(),
        "LightningGoatsCanaryAck" => owner.ack.clone(),
        _ => "NULL".into(),
    }
}
async fn command(State(owner): State<Shared>, body: String) {
    owner.lock().unwrap().commands.push(body);
}
struct Fixture {
    _dir: TempDir,
    owner: Shared,
    owner_url: String,
    db: String,
    witness_url: String,
    generation: Uuid,
    pool: SqlitePool,
    witness_pool: SqlitePool,
    tasks: Vec<tokio::task::JoinHandle<()>>,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        for task in &self.tasks {
            task.abort();
        }
    }
}
impl Fixture {
    async fn new() -> Self {
        let dir = TempDir::new().unwrap();
        let db = format!("sqlite://{}/gateway.db", dir.path().display());
        let witness_url = format!("sqlite://{}/witness.db", dir.path().display());
        let pool = crate::sqlite::connect_durable(&db, 1).await.unwrap();
        GatewayStore::connect(&db).await.unwrap();
        let witness_pool = crate::sqlite::connect_durable(&witness_url, 1)
            .await
            .unwrap();
        sqlx::query("CREATE TABLE owner_witness_metadata(version INTEGER, generation TEXT)")
            .execute(&witness_pool)
            .await
            .unwrap();
        sqlx::query("CREATE TABLE owner_witness(request_id TEXT PRIMARY KEY, gateway_required INTEGER NOT NULL CHECK(gateway_required IN(0,1)))").execute(&witness_pool).await.unwrap();
        let generation = Uuid::new_v4();
        sqlx::query("INSERT INTO owner_witness_metadata VALUES(1,?)")
            .bind(generation.to_string())
            .execute(&witness_pool)
            .await
            .unwrap();
        let owner = Arc::new(Mutex::new(Owner {
            remote: true,
            ack: "NULL".into(),
            commands: vec![],
        }));
        let app = Router::new()
            .route("/rest/items/{item}/state", get(item))
            .route("/rest/items/LightningGoatsCanaryRequest", post(command))
            .with_state(owner.clone());
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let owner_url = format!("http://{}/", listener.local_addr().unwrap());
        let task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        Self {
            _dir: dir,
            owner,
            owner_url,
            db,
            witness_url,
            generation,
            pool,
            witness_pool,
            tasks: vec![task],
        }
    }
    async fn gateway(&mut self) -> Result<String> {
        let witness = OwnerWitness::open(&self.witness_url, self.generation).await?;
        let store = GatewayStore::connect(&self.db).await?;
        witness
            .validate_coverage(&store.request_identities().await?)
            .await?;
        let openhab = OpenHabClient::new(
            &TrustedOpenHabConfig {
                url: self.owner_url.clone(),
                request_item: "LightningGoatsCanaryRequest".into(),
                ack_item: "LightningGoatsCanaryAck".into(),
                protocol: crate::openhab::OwnerProtocol::UuidCanary,
                override_item: "Override".into(),
                remote_enabled_item: "Remote".into(),
                temperature_item: None,
            },
            "synthetic-only".into(),
        )?;
        let weather = WeatherAdapter::new(
            "http://127.0.0.1:1/get_received_data",
            Duration::from_secs(300),
            store.clone(),
        )?;
        let gateway = TrustedGateway {
            state: GatewayState {
                openhab,
                store,
                weather,
                witness: Some(witness),
                ack_timeout: Duration::from_millis(100),
                ack_poll: Duration::from_millis(50),
                min_feed_interval: Duration::from_secs(5),
                max_feeds_per_hour: 60,
            },
            listen: "127.0.0.1:0".parse().unwrap(),
        };
        let listener = TcpListener::bind(gateway.listen()).await?;
        let origin = format!("http://{}", listener.local_addr()?);
        self.tasks.push(tokio::spawn(async move {
            axum::serve(listener, gateway.router()).await.unwrap()
        }));
        Ok(origin)
    }
    fn commands(&self) -> usize {
        self.owner.lock().unwrap().commands.len()
    }
}
async fn request(origin: &str, id: Uuid, method: reqwest::Method) -> (u16, Value) {
    let response = reqwest::Client::new()
        .request(method, format!("{origin}/v1/feeder/request/{id}"))
        .send()
        .await
        .unwrap();
    (response.status().as_u16(), response.json().await.unwrap())
}

#[tokio::test]
async fn witness_hit_store_miss_is_error_before_safety_refusal_or_unknown_get() {
    let mut f = Fixture::new().await;
    let url = f.gateway().await.unwrap();
    let id = Uuid::new_v4();
    f.owner.lock().unwrap().remote = false;
    sqlx::query("INSERT INTO owner_witness VALUES(?,0)")
        .bind(id.to_string())
        .execute(&f.witness_pool)
        .await
        .unwrap();
    for method in [reqwest::Method::POST, reqwest::Method::GET] {
        let (code, body) = request(&url, id, method).await;
        assert_eq!(code, 502);
        assert_eq!(body["status"], "ERROR");
    }
    assert_eq!(f.commands(), 0);
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM feeder_refusals")
            .fetch_one(&f.pool)
            .await
            .unwrap(),
        0
    );
}

#[tokio::test]
async fn commit_failure_preserves_pending_and_never_dispatches_or_repairs() {
    let mut f = Fixture::new().await;
    let url = f.gateway().await.unwrap();
    let id = Uuid::new_v4();
    sqlx::query("CREATE TRIGGER deny_witness BEFORE INSERT ON owner_witness BEGIN SELECT RAISE(FAIL,'injected'); END").execute(&f.witness_pool).await.unwrap();
    assert_eq!(request(&url, id, reqwest::Method::POST).await.0, 502);
    assert_eq!(f.commands(), 0);
    assert_eq!(
        sqlx::query_scalar::<_, String>("SELECT status FROM feeder_requests WHERE request_id=?")
            .bind(id.to_string())
            .fetch_one(&f.pool)
            .await
            .unwrap(),
        "pending"
    );
    sqlx::query("DROP TRIGGER deny_witness")
        .execute(&f.witness_pool)
        .await
        .unwrap();
    for target in [id, Uuid::new_v4()] {
        assert_eq!(request(&url, target, reqwest::Method::POST).await.0, 502);
    }
    assert!(f.gateway().await.is_err());
    assert_eq!(f.commands(), 0);
}

#[tokio::test]
async fn held_restart_recovery_and_refusals_preserve_one_owner_post() {
    let mut f = Fixture::new().await;
    let url = f.gateway().await.unwrap();
    let id = Uuid::new_v4();
    assert_eq!(
        request(&url, id, reqwest::Method::POST).await.1["status"],
        "ambiguous"
    );
    assert_eq!(f.commands(), 1);
    let refused = Uuid::new_v4();
    assert_eq!(request(&url, refused, reqwest::Method::POST).await.0, 423);
    f.tasks.pop().unwrap().abort();
    let reopened = f.gateway().await.unwrap();
    assert_eq!(
        request(&reopened, id, reqwest::Method::GET).await.1["status"],
        "pending"
    );
    f.owner.lock().unwrap().ack = id.to_string();
    for method in [reqwest::Method::GET, reqwest::Method::POST] {
        let (code, body) = request(&reopened, id, method).await;
        assert_eq!(code, 200);
        assert_eq!(body["status"], "confirmed");
    }
    assert_eq!(
        request(&reopened, refused, reqwest::Method::POST).await.0,
        423
    );
    assert_eq!(f.commands(), 1);
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM owner_witness")
            .fetch_one(&f.witness_pool)
            .await
            .unwrap(),
        2
    );
}

#[tokio::test]
async fn either_direction_of_partial_restore_blocks_get_post_and_startup() {
    for lose_gateway in [false, true] {
        let mut f = Fixture::new().await;
        let url = f.gateway().await.unwrap();
        let id = Uuid::new_v4();
        request(&url, id, reqwest::Method::POST).await;
        assert_eq!(f.commands(), 1);
        if lose_gateway {
            sqlx::query("DELETE FROM feeder_request_events")
                .execute(&f.pool)
                .await
                .unwrap();
            sqlx::query("DELETE FROM feeder_requests")
                .execute(&f.pool)
                .await
                .unwrap();
        } else {
            sqlx::query("DELETE FROM owner_witness")
                .execute(&f.witness_pool)
                .await
                .unwrap();
        }
        for method in [reqwest::Method::GET, reqwest::Method::POST] {
            assert_eq!(request(&url, id, method).await.0, 502);
        }
        assert_eq!(
            request(&url, Uuid::new_v4(), reqwest::Method::POST).await.0,
            502
        );
        assert!(f.gateway().await.is_err());
        assert_eq!(f.commands(), 1);
    }
}
