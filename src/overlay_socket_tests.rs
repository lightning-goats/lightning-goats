use super::*;
use axum::{
    Router,
    extract::{Query, ws::WebSocketUpgrade},
    response::IntoResponse,
    routing::get,
};
use tokio_tungstenite::{connect_async, tungstenite::Message as ClientMessage};
type Socket =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

struct Fixture {
    _dir: tempfile::TempDir,
    ledger: LedgerStore,
    url: String,
    task: tokio::task::JoinHandle<()>,
    completed: tokio::sync::mpsc::Receiver<String>,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.task.abort();
    }
}
impl Fixture {
    async fn new(timing: Timing) -> Self {
        let dir = tempfile::TempDir::new().unwrap();
        let ledger = LedgerStore::connect(&format!(
            "sqlite://{}",
            dir.path().join("overlay.db").display()
        ))
        .await
        .unwrap();
        let (completed_tx, completed) = tokio::sync::mpsc::channel(16);
        let app = Router::new().route(
            "/ws",
            get({
                let ledger = ledger.clone();
                move |ws: WebSocketUpgrade, Query(resume): Query<OverlayResume>| {
                    let ledger = ledger.clone();
                    let completed_tx = completed_tx.clone();
                    async move {
                        if resume.validate().is_err() {
                            return axum::http::StatusCode::BAD_REQUEST.into_response();
                        }
                        ws.max_frame_size(1024).max_message_size(1024).on_upgrade(
                            move |socket| async move {
                                let outcome = serve_with_timing(
                                    socket,
                                    ledger,
                                    MessageRenderer::embedded().unwrap(),
                                    1000,
                                    resume,
                                    timing,
                                )
                                .await;
                                let _ = completed_tx.try_send(
                                    outcome
                                        .err()
                                        .map(|error| error.to_string())
                                        .unwrap_or_default(),
                                );
                            },
                        )
                    }
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("ws://{}/ws", listener.local_addr().unwrap());
        let task = tokio::spawn(async move { crate::server::serve(listener, app).await.unwrap() });
        Self {
            _dir: dir,
            ledger,
            url,
            task,
            completed,
        }
    }
    async fn append(&self) -> u64 {
        self.ledger
            .append_event("interface_info", &json!({}))
            .await
            .unwrap()
    }
    async fn connect(&self, query: &str) -> Socket {
        connect_async(format!("{}{query}", self.url))
            .await
            .unwrap()
            .0
    }
}
async fn next_json(socket: &mut Socket) -> Value {
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            match socket.next().await.unwrap().unwrap() {
                ClientMessage::Text(text) => return serde_json::from_str(&text).unwrap(),
                ClientMessage::Ping(bytes) => {
                    socket.send(ClientMessage::Pong(bytes)).await.unwrap()
                }
                other => panic!("unexpected {other:?}"),
            }
        }
    })
    .await
    .unwrap()
}

#[tokio::test]
async fn reconnect_replays_ordered_gap_before_checkpoint_and_live_events() {
    let fixture = Fixture::new(Timing::default()).await;
    assert_eq!(fixture.append().await, 1);
    let mut socket = fixture.connect("").await;
    let first = next_json(&mut socket).await;
    assert_eq!(first["seq"], 1);
    let stream = first["stream_id"].as_str().unwrap();
    assert_eq!(
        fixture
            .ledger
            .overlay_stream_id()
            .await
            .unwrap()
            .to_string(),
        stream
    );
    socket.close(None).await.unwrap();
    assert_eq!(fixture.append().await, 2);
    assert_eq!(fixture.append().await, 3);
    let mut socket = fixture
        .connect(&format!("?version=1&stream={stream}&after=1"))
        .await;
    let resumed = next_json(&mut socket).await;
    assert_eq!(resumed["type"], "resume");
    assert_eq!(resumed["through"], 3);
    assert!(resumed.get("seq").is_none());
    assert_eq!(fixture.append().await, 4);
    for seq in [2, 3] {
        let event = next_json(&mut socket).await;
        assert_eq!(event["seq"], seq);
        assert_eq!(event["type"], "interface_info");
    }
    let checkpoint = next_json(&mut socket).await;
    assert_eq!(checkpoint["type"], "snapshot");
    assert_eq!(checkpoint["seq"], 3);
    assert_eq!(checkpoint["feed_credit_sats"], 0);
    let live = next_json(&mut socket).await;
    assert_eq!(live["seq"], 4);
    socket.close(None).await.unwrap();
}

#[tokio::test]
async fn unavailable_cursors_and_oversized_history_explicitly_reset() {
    let fixture = Fixture::new(Timing::default()).await;
    fixture.append().await;
    let stream = fixture.ledger.overlay_stream_id().await.unwrap();
    for query in [
        format!("?version=1&stream={}&after=0", Uuid::new_v4()),
        format!("?version=1&stream={stream}&after=2"),
    ] {
        let mut socket = fixture.connect(&query).await;
        let reset = next_json(&mut socket).await;
        assert_eq!(reset["type"], "snapshot");
        assert_eq!(reset["reset_reason"], "resume_unavailable");
        socket.close(None).await.unwrap();
    }
    for query in ["?after=0", "?version=2", "?version=1&after=0", "?unknown=1"] {
        let error = connect_async(format!("{}{query}", fixture.url))
            .await
            .unwrap_err();
        match error {
            tokio_tungstenite::tungstenite::Error::Http(response) => {
                assert_eq!(response.status(), 400)
            }
            _ => panic!("{error}"),
        }
    }
    fixture
        .ledger
        .append_event("interface_info", &json!({"padding":"a".repeat(600_000)}))
        .await
        .unwrap();
    assert!(
        fixture
            .ledger
            .overlay_event_window(0, 2)
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(
        fixture.ledger.events_after(1, 1).await.unwrap().len(),
        1,
        "publication history remains readable"
    );
    let mut socket = fixture
        .connect(&format!("?version=1&stream={stream}&after=1"))
        .await;
    let reset = next_json(&mut socket).await;
    assert_eq!(reset["reset_reason"], "resume_unavailable");
    assert_eq!(reset["seq"], 2);
}

#[tokio::test]
async fn missing_pong_and_application_input_close_the_socket() {
    let fixture = Fixture::new(Timing {
        heartbeat: Duration::from_millis(50),
        pong: Duration::from_millis(50),
        ..Timing::default()
    })
    .await;
    let mut socket = fixture.connect("").await;
    next_json(&mut socket).await;
    tokio::time::sleep(Duration::from_millis(250)).await; // deliberately never poll or flush automatic pong
    let closed = tokio::time::timeout(Duration::from_secs(1), async {
        loop {
            match socket.next().await {
                Some(Ok(ClientMessage::Close(_))) | Some(Err(_)) | None => break,
                _ => {}
            }
        }
    })
    .await;
    assert!(closed.is_ok());
    let mut socket = fixture.connect("").await;
    next_json(&mut socket).await;
    socket
        .send(ClientMessage::Text("unauthorized input".into()))
        .await
        .unwrap();
    assert!(matches!(
        tokio::time::timeout(Duration::from_secs(1), socket.next())
            .await
            .unwrap(),
        Some(Err(_)) | None | Some(Ok(ClientMessage::Close(_)))
    ));
}

#[tokio::test]
async fn quiet_connection_survives_default_heartbeat_beyond_edge_timeout() {
    let fixture = Fixture::new(Timing::default()).await;
    let mut socket = fixture.connect("").await;
    next_json(&mut socket).await;
    let started = Instant::now();
    let mut pings = 0;
    while started.elapsed() < Duration::from_secs(76) {
        tokio::select! {
            frame = socket.next() => {
                match frame.unwrap().unwrap() {
                    ClientMessage::Ping(bytes) => { pings += 1; socket.send(ClientMessage::Pong(bytes)).await.unwrap(); }
                    other => panic!("unexpected idle frame {other:?}"),
                }
            }
            _ = tokio::time::sleep_until(started + Duration::from_secs(76)) => break,
        }
    }
    assert!(pings >= 3);
    fixture.append().await;
    assert_eq!(next_json(&mut socket).await["seq"], 1);
}

#[test]
fn stale_weather_replay_advances_cursor_without_displaying_old_message() {
    let message = durable_event_message(
        DurableEvent {
            seq: 7,
            event_type: "weather_status".into(),
            payload_json:
                json!({"message":"Old weather","data":{"observed_at":"2000-01-01T00:00:00Z"}})
                    .to_string(),
        },
        &MessageRenderer::embedded().unwrap(),
        1000,
    )
    .unwrap();
    let value: Value = serde_json::from_str(&message).unwrap();
    assert_eq!(value["type"], "event_skipped");
    assert_eq!(value["seq"], 7);
    assert!(value.get("message").is_none());
}

#[tokio::test]
async fn unread_output_hits_send_deadline_without_unbounded_queue() {
    let mut fixture = Fixture::new(Timing {
        heartbeat: Duration::from_secs(60),
        poll: Duration::from_millis(5),
        ..Timing::default()
    })
    .await;
    let mut socket = fixture.connect("").await;
    next_json(&mut socket).await;
    let pool = sqlx::SqlitePool::connect(&format!(
        "sqlite://{}",
        fixture._dir.path().join("overlay.db").display()
    ))
    .await
    .unwrap();
    let mut tx = pool.begin().await.unwrap();
    let payload = json!({"padding":"x".repeat(3500)}).to_string();
    for _ in 0..4000 {
        sqlx::query("INSERT INTO event_log(event_type,payload_json) VALUES('interface_info',?)")
            .bind(&payload)
            .execute(&mut *tx)
            .await
            .unwrap();
    }
    tx.commit().await.unwrap();
    // Keep the client alive but never consume its output or acknowledge a ping.
    let error = tokio::time::timeout(Duration::from_secs(30), fixture.completed.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(error, "overlay send deadline exceeded");
    assert_eq!(fixture.ledger.feed_credit_sats().await.unwrap(), 0);
    drop(socket);
}

#[tokio::test]
async fn queued_weather_is_rechecked_after_waiting_to_send() {
    let time = chrono::DateTime::from_timestamp(crate::gateway::now_epoch().unwrap() - 299, 0)
        .unwrap()
        .to_rfc3339();
    let mut queue = render_window(
        vec![DurableEvent {
            seq: 8,
            event_type: "weather_status".into(),
            payload_json: json!({"message":"Near expiry","data":{"observed_at":time}}).to_string(),
        }],
        &MessageRenderer::embedded().unwrap(),
        1000,
    )
    .unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&queue[0].body).unwrap()["type"],
        "weather_status"
    );
    tokio::time::sleep(Duration::from_millis(2100)).await;
    let value: Value = serde_json::from_str(&queue.pop_front().unwrap().current_body()).unwrap();
    assert_eq!(value["type"], "event_skipped");
    assert_eq!(value["seq"], 8);
}

#[tokio::test]
async fn restore_identity_reset_rejects_old_cursor_even_after_sequence_reuse() {
    let fixture = Fixture::new(Timing::default()).await;
    for _ in 0..3 {
        fixture.append().await;
    }
    let old_stream = fixture.ledger.overlay_stream_id().await.unwrap();
    // Model restored history before new events reuse abandoned sequence numbers.
    let pool = sqlx::SqlitePool::connect(&format!(
        "sqlite://{}",
        fixture._dir.path().join("overlay.db").display()
    ))
    .await
    .unwrap();
    sqlx::query("DELETE FROM event_log WHERE seq>1")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("UPDATE sqlite_sequence SET seq=1 WHERE name='event_log'")
        .execute(&pool)
        .await
        .unwrap();
    let new_stream = fixture.ledger.reset_overlay_stream().await.unwrap();
    assert_ne!(new_stream, old_stream);
    for _ in 0..3 {
        fixture.append().await;
    }
    let mut socket = fixture
        .connect(&format!("?version=1&stream={old_stream}&after=3"))
        .await;
    let reset = next_json(&mut socket).await;
    assert_eq!(reset["reset_reason"], "resume_unavailable");
    assert_eq!(reset["stream_id"], new_stream.to_string());
    assert_eq!(reset["seq"], 4);
}
