#![forbid(unsafe_code)]

use std::{future::pending, path::PathBuf, sync::Arc, time::Duration};

use anyhow::{Result, anyhow};
use axum::{
    Json, Router,
    body::Bytes,
    extract::{DefaultBodyLimit, Path, Query, Request, State, ws::WebSocketUpgrade},
    http::{HeaderMap, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use clap::Parser;
use lightning_goats::{
    config::AppConfig,
    feeder::run_feed_worker,
    gateway::GatewayClient,
    informational::run_informational_worker,
    ledger::LedgerStore,
    lnurl::{LnurlErrorResponse, LnurlService, LnurlServiceError},
    messaging::{run_message_processor, run_outbox_publisher},
    nostr::NakClient,
    overlay::{OverlayResume, serve_overlay_socket},
    presentation::MessageRenderer,
    strike::StrikeRuntime,
};
use serde::{Deserialize, Serialize};
use tracing::info;
use tracing_subscriber::EnvFilter;

const MAX_STRIKE_WEBHOOK_BODY: usize = 32 * 1024;

#[derive(Debug, Parser)]
#[command(name = "lightning-goatsd")]
#[command(about = "Lightning Goats Strike payment accounting and feeder automation service")]
struct Args {
    #[arg(long, default_value = "/etc/lightning-goats/config.toml")]
    config: PathBuf,
}

#[derive(Clone)]
struct AppState {
    config: Arc<AppConfig>,
    ledger: LedgerStore,
    gateway: GatewayClient,
    strike: StrikeRuntime,
    lnurl: LnurlService,
    renderer: MessageRenderer,
    status_slots: Arc<tokio::sync::Semaphore>,
    overlay_slots: Arc<tokio::sync::Semaphore>,
}

#[derive(Debug, Serialize)]
struct HealthResponse {
    status: &'static str,
}

#[derive(Debug, Serialize)]
struct StatusResponse {
    mode: &'static str,
    payment_backend: &'static str,
    lightning_address_users: Vec<String>,
    gateway_reachable: bool,
    feed_credit_sats: u64,
    threshold_sats: u64,
    feeds_due: u64,
    remainder_sats: u64,
    unresolved_feed_attempt: Option<String>,
    feeder_override_active: Option<bool>,
    remote_feeding_enabled: Option<bool>,
    temperature_f: Option<f64>,
}

#[derive(Debug, Deserialize)]
struct LnurlCallbackQuery {
    amount: Option<String>,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let args = Args::parse();
    let config = Arc::new(AppConfig::load(&args.config)?);
    let ledger = LedgerStore::connect(&config.database.url).await?;
    let renderer = MessageRenderer::embedded()?;

    let interrupted = ledger.mark_interrupted_feed_intents_unknown().await?;
    if interrupted > 0 {
        tracing::error!(
            interrupted,
            "converted interrupted feed intent(s) to unknown; operator reconciliation is required"
        );
    }

    let gateway = GatewayClient::new(&config.gateway.url)?;
    let strike = StrikeRuntime::from_config(&config.strike).await?;
    let lnurl = LnurlService::new(
        &config.lnurl,
        &config.lightning_address,
        strike.clone(),
        ledger.clone(),
    )?;
    let nostr = if config.service.mode.nostr_enabled() {
        Some(NakClient::from_config(&config.nostr).await?)
    } else {
        None
    };

    let state = AppState {
        config: Arc::clone(&config),
        ledger: ledger.clone(),
        gateway: gateway.clone(),
        strike: strike.clone(),
        lnurl,
        renderer: renderer.clone(),
        status_slots: Arc::new(tokio::sync::Semaphore::new(4)),
        overlay_slots: Arc::new(tokio::sync::Semaphore::new(32)),
    };
    let app = app(state);

    let listener = tokio::net::TcpListener::bind(config.service.listen).await?;
    info!(
        listen = %config.service.listen,
        mode = config.service.mode.as_str(),
        payment_backend = "strike",
        gateway = %config.gateway.url,
        "lightning-goatsd listening"
    );

    let mut server =
        tokio::spawn(async move { lightning_goats::server::serve(listener, app).await });
    let mut recovery = tokio::spawn(strike.run_recovery_worker(ledger.clone()));
    let mut feeder = tokio::spawn(run_feed_worker(
        ledger.clone(),
        gateway.clone(),
        config.feeder.threshold_sats,
        Duration::from_secs(config.feeder.inter_feed_delay_seconds),
        config.service.mode,
    ));
    let mut informational = tokio::spawn(run_informational_worker(
        ledger.clone(),
        gateway,
        config.informational.clone(),
    ));
    let mut message_processor = tokio::spawn(run_message_processor(
        ledger.clone(),
        nostr.clone(),
        renderer,
        config.feeder.threshold_sats,
        config.service.mode,
    ));
    let mut publisher = tokio::spawn({
        let ledger = ledger.clone();
        async move {
            if let Some(nak) = nostr {
                run_outbox_publisher(ledger, nak).await
            } else {
                pending::<Result<()>>().await
            }
        }
    });

    let result = tokio::select! {
        _ = shutdown_signal() => {
            info!("shutdown signal received");
            Ok(())
        }
        result = &mut server => task_exit("HTTP server", result),
        result = &mut recovery => task_exit("settlement recovery", result),
        result = &mut feeder => task_exit("feed worker", result),
        result = &mut informational => task_exit("informational worker", result),
        result = &mut message_processor => task_exit("Nostr message processor", result),
        result = &mut publisher => task_exit("Nostr outbox publisher", result),
    };

    server.abort();
    recovery.abort();
    feeder.abort();
    informational.abort();
    message_processor.abort();
    publisher.abort();
    result
}

fn app(state: AppState) -> Router {
    Router::new()
        .route("/healthz", get(healthz))
        .route("/api/v1/status", get(status))
        .route(
            "/api/v1/strike/webhook",
            post(strike_webhook).layer(DefaultBodyLimit::max(MAX_STRIKE_WEBHOOK_BODY)),
        )
        .route("/.well-known/lnurlp/{user}", get(lnurl_discovery))
        .route("/lnurlp/{user}/callback", get(lnurl_callback))
        .route("/ws/overlay", get(overlay_ws))
        .with_state(state)
        .layer(middleware::from_fn_with_state(
            Arc::new(tokio::sync::Semaphore::new(64)),
            bounded_http,
        ))
}

async fn bounded_http(
    State(slots): State<Arc<tokio::sync::Semaphore>>,
    request: Request,
    next: Next,
) -> Response {
    let is_lnurl = request.uri().path().starts_with("/.well-known/lnurlp/")
        || request.uri().path().starts_with("/lnurlp/");
    let rejection = |status| {
        if is_lnurl {
            lnurl_error(status, "Invoice service is busy; retry later")
        } else {
            status.into_response()
        }
    };
    let Ok(_permit) = slots.try_acquire_owned() else {
        return rejection(StatusCode::SERVICE_UNAVAILABLE);
    };
    match tokio::time::timeout(Duration::from_secs(30), next.run(request)).await {
        Ok(response) => response,
        Err(_) => rejection(StatusCode::REQUEST_TIMEOUT),
    }
}

fn task_exit(name: &str, result: Result<Result<()>, tokio::task::JoinError>) -> Result<()> {
    match result {
        Ok(Ok(())) => Err(anyhow!("{name} exited unexpectedly")),
        Ok(Err(error)) => Err(anyhow!("{name} failed: {error:#}")),
        Err(error) => Err(anyhow!("{name} task failed: {error}")),
    }
}

async fn healthz() -> Json<HealthResponse> {
    Json(HealthResponse { status: "ok" })
}

async fn status(State(state): State<AppState>) -> Result<Json<StatusResponse>, StatusCode> {
    let _permit = state
        .status_slots
        .clone()
        .try_acquire_owned()
        .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
    let feed_credit_sats = state.ledger.feed_credit_sats().await.map_err(|error| {
        tracing::error!(%error, "failed reading feed credit for status endpoint");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;
    let unresolved_feed_attempt = state
        .ledger
        .unresolved_feed_attempt()
        .await
        .map_err(|error| {
            tracing::error!(%error, "failed reading feed attempt for status endpoint");
            StatusCode::INTERNAL_SERVER_ERROR
        })?
        .map(|attempt| attempt.id.to_string());
    let threshold_sats = state.config.feeder.threshold_sats;

    let (gateway_reachable, feeder_override_active, remote_feeding_enabled) =
        match state.gateway.feeder_safety().await {
            Ok(safety) => (
                true,
                Some(safety.override_enabled),
                Some(safety.remote_enabled),
            ),
            Err(error) => {
                tracing::warn!(%error, "trusted feeder gateway unavailable for status endpoint");
                (false, None, None)
            }
        };
    let temperature_f = match state.gateway.temperature_f().await {
        Ok(value) => value,
        Err(error) => {
            tracing::warn!(%error, "trusted gateway temperature unavailable for status endpoint");
            None
        }
    };

    Ok(Json(StatusResponse {
        mode: state.config.service.mode.as_str(),
        payment_backend: "strike",
        lightning_address_users: state.lnurl.configured_users(),
        gateway_reachable,
        feed_credit_sats,
        threshold_sats,
        feeds_due: feed_credit_sats / threshold_sats,
        remainder_sats: feed_credit_sats % threshold_sats,
        unresolved_feed_attempt,
        feeder_override_active,
        remote_feeding_enabled,
        temperature_f,
    }))
}

async fn lnurl_discovery(Path(user): Path<String>, State(state): State<AppState>) -> Response {
    match state.lnurl.discovery(&user) {
        Ok(response) => (StatusCode::OK, Json(response)).into_response(),
        Err(error) if error.is_unknown_user() => {
            lnurl_error(StatusCode::NOT_FOUND, error.public_reason())
        }
        Err(error) => {
            if let Some(internal) = error.internal_error() {
                tracing::error!(%internal, %user, "failed constructing LNURL discovery response");
            }
            lnurl_error(StatusCode::INTERNAL_SERVER_ERROR, error.public_reason())
        }
    }
}

async fn lnurl_callback(
    Path(user): Path<String>,
    State(state): State<AppState>,
    Query(query): Query<LnurlCallbackQuery>,
) -> Response {
    let Some(raw_amount) = query.amount.as_deref() else {
        return lnurl_error(StatusCode::OK, "Missing amount");
    };
    let amount_msat = match raw_amount.parse::<u64>() {
        Ok(amount) => amount,
        Err(_) => return lnurl_error(StatusCode::OK, "Invalid amount"),
    };

    match state.lnurl.callback(&user, amount_msat).await {
        Ok(response) => (StatusCode::OK, Json(response)).into_response(),
        Err(LnurlServiceError::UnknownUser) => {
            lnurl_error(StatusCode::NOT_FOUND, "Unknown Lightning Address")
        }
        Err(LnurlServiceError::InvalidAmount(reason)) => lnurl_error(StatusCode::OK, reason),
        Err(LnurlServiceError::Busy) => lnurl_error(
            StatusCode::TOO_MANY_REQUESTS,
            "Invoice service is busy; retry later",
        ),
        Err(error @ LnurlServiceError::Provider(_)) => {
            if let Some(internal) = error.internal_error() {
                tracing::error!(%internal, %user, amount_msat, "Strike-backed LNURL invoice creation failed");
            }
            lnurl_error(StatusCode::BAD_GATEWAY, error.public_reason())
        }
    }
}

fn lnurl_error(status: StatusCode, reason: &str) -> Response {
    (status, Json(LnurlErrorResponse::new(reason))).into_response()
}

async fn strike_webhook(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> StatusCode {
    if !headers
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| {
            v.split(';')
                .next()
                .unwrap_or("")
                .trim()
                .eq_ignore_ascii_case("application/json")
        })
    {
        return StatusCode::UNSUPPORTED_MEDIA_TYPE;
    }
    if body.len() > MAX_STRIKE_WEBHOOK_BODY {
        tracing::warn!(body_len = body.len(), "rejected oversized Strike webhook");
        return StatusCode::PAYLOAD_TOO_LARGE;
    }
    let Some(signature) = headers
        .get("x-webhook-signature")
        .and_then(|value| value.to_str().ok())
    else {
        tracing::warn!("rejected Strike webhook without a valid signature header");
        return StatusCode::UNAUTHORIZED;
    };

    if let Err(error) = state.strike.verify_webhook_signature(&body, signature) {
        tracing::warn!(%error, "rejected Strike webhook with invalid signature");
        return StatusCode::UNAUTHORIZED;
    }
    let event = match state.strike.parse_completed_event(&body) {
        Ok(event) => event,
        Err(error) => {
            tracing::warn!(%error, "rejected unsupported or malformed signed Strike webhook");
            return StatusCode::BAD_REQUEST;
        }
    };

    match state.ledger.enqueue_strike_event(&event).await {
        Ok(()) => StatusCode::NO_CONTENT,
        Err(error) => {
            tracing::error!(%error,"Strike inbox persistence failed; notification remains retryable");
            StatusCode::SERVICE_UNAVAILABLE
        }
    }
}

async fn overlay_ws(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
    Query(resume): Query<OverlayResume>,
) -> Response {
    if resume.validate().is_err() {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let Ok(permit) = state.overlay_slots.clone().try_acquire_owned() else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    let ledger = state.ledger.clone();
    let renderer = state.renderer.clone();
    let threshold_sats = state.config.feeder.threshold_sats;
    ws.max_message_size(1024)
        .max_frame_size(1024)
        .write_buffer_size(4096)
        .max_write_buffer_size(65536)
        .on_upgrade(move |socket| async move {
            let _permit = permit;
            if let Err(error) =
                serve_overlay_socket(socket, ledger, renderer, threshold_sats, resume).await
            {
                tracing::warn!(%error, "overlay websocket disconnected after server-side error");
            }
        })
}

async fn shutdown_signal() {
    if let Err(error) = tokio::signal::ctrl_c().await {
        tracing::error!(%error, "failed to install shutdown signal handler");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hmac::{Hmac, Mac};
    use lightning_goats::strike::{StrikeClient, StrikeWebhookVerifier};
    use sha2::Sha256;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    #[tokio::test]
    async fn webhook_acknowledges_only_durable_inbox_without_waiting_for_provider() {
        let directory = tempfile::TempDir::new().unwrap();
        let db = format!("sqlite://{}", directory.path().join("inbox.db").display());
        let ledger = LedgerStore::connect(&db).await.unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let provider = Router::new().fallback({
            let calls = calls.clone();
            move || {
                let calls = calls.clone();
                async move {
                    calls.fetch_add(1, Ordering::SeqCst);
                    tokio::time::sleep(Duration::from_secs(30)).await;
                    StatusCode::SERVICE_UNAVAILABLE
                }
            }
        });
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let provider_url = format!("http://{}/", listener.local_addr().unwrap());
        let provider_task =
            tokio::spawn(async move { axum::serve(listener, provider).await.unwrap() });
        let strike = StrikeRuntime::new(
            StrikeClient::new(&provider_url, "synthetic-receive-only".into()).unwrap(),
            StrikeWebhookVerifier::new("synthetic-webhook-secret".into()).unwrap(),
        );
        let config: AppConfig =
            toml::from_str(include_str!("../deploy/config.canary.toml.example")).unwrap();
        let lnurl = LnurlService::new(
            &config.lnurl,
            &config.lightning_address,
            strike.clone(),
            ledger.clone(),
        )
        .unwrap();
        let router = app(AppState {
            config: Arc::new(config),
            ledger: ledger.clone(),
            gateway: GatewayClient::new("http://127.0.0.1:9/").unwrap(),
            strike,
            lnurl,
            renderer: MessageRenderer::embedded().unwrap(),
            status_slots: Arc::new(tokio::sync::Semaphore::new(4)),
            overlay_slots: Arc::new(tokio::sync::Semaphore::new(32)),
        });
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            lightning_goats::server::serve(listener, router)
                .await
                .unwrap()
        });
        let url = format!("http://{address}/api/v1/strike/webhook");
        let client = reqwest::Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(2))
            .build()
            .unwrap();
        let body = serde_json::json!({"id":uuid::Uuid::new_v4(),"eventType":"receive-request.receive-completed","webhookVersion":"v1","data":{"entityId":uuid::Uuid::new_v4(),"receiveId":uuid::Uuid::new_v4()}}).to_string();
        let signature = |body: &str| {
            let mut mac = Hmac::<Sha256>::new_from_slice(b"synthetic-webhook-secret").unwrap();
            mac.update(body.as_bytes());
            hex::encode(mac.finalize().into_bytes())
        };
        let post = |body: String| {
            client
                .post(&url)
                .header("content-type", "application/json")
                .header("x-webhook-signature", signature(&body))
                .body(body)
                .send()
        };
        for _ in 0..2 {
            assert_eq!(
                post(body.clone()).await.unwrap().status(),
                StatusCode::NO_CONTENT
            );
        }
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        let pool = sqlx::SqlitePool::connect(&db).await.unwrap();
        sqlx::query("WITH RECURSIVE n(x) AS (SELECT 1 UNION ALL SELECT x+1 FROM n WHERE x<30) INSERT INTO invoice_admissions(id,finished) SELECT 'synthetic:'||x,1 FROM n").execute(&pool).await.unwrap();
        let limited = client
            .get(format!(
                "http://{address}/lnurlp/herd/callback?amount=1000000"
            ))
            .send()
            .await
            .unwrap();
        assert_eq!(limited.status(), StatusCode::TOO_MANY_REQUESTS);
        let limited: serde_json::Value = limited.json().await.unwrap();
        assert_eq!(limited["status"], "ERROR");
        // Saturating public issuance must not disable webhook persistence.
        assert_eq!(
            post(body.clone()).await.unwrap().status(),
            StatusCode::NO_CONTENT
        );
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM strike_inbox")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(count, 1);
        let restored = LedgerStore::connect(&db).await.unwrap();
        assert!(restored.due_strike_work().await.unwrap().is_some());
        assert_eq!(restored.feed_credit_sats().await.unwrap(), 0);

        sqlx::query("CREATE TRIGGER fail_inbox BEFORE INSERT ON strike_inbox BEGIN SELECT RAISE(FAIL,'injected storage failure'); END").execute(&pool).await.unwrap();
        let mut changed: serde_json::Value = serde_json::from_str(&body).unwrap();
        changed["id"] = uuid::Uuid::new_v4().to_string().into();
        assert_eq!(
            post(changed.to_string()).await.unwrap().status(),
            StatusCode::SERVICE_UNAVAILABLE
        );
        assert_eq!(
            client
                .post(&url)
                .header("content-type", "application/json")
                .header("x-webhook-signature", "00".repeat(32))
                .body(body.clone())
                .send()
                .await
                .unwrap()
                .status(),
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            client.post(&url).body(body).send().await.unwrap().status(),
            StatusCode::UNSUPPORTED_MEDIA_TYPE
        );
        assert_eq!(
            client.get(&url).send().await.unwrap().status(),
            StatusCode::METHOD_NOT_ALLOWED
        );

        // No Content-Length: the extractor must stop a chunked body at the same bound.
        let mut socket = tokio::net::TcpStream::connect(address).await.unwrap();
        let oversized = "x".repeat(MAX_STRIKE_WEBHOOK_BODY + 1);
        let wire = format!(
            "POST /api/v1/strike/webhook HTTP/1.1\r\nHost: {address}\r\nContent-Type: application/json\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n{:x}\r\n{oversized}\r\n0\r\n\r\n",
            oversized.len()
        );
        socket.write_all(wire.as_bytes()).await.unwrap();
        let mut response = String::new();
        tokio::time::timeout(Duration::from_secs(2), socket.read_to_string(&mut response))
            .await
            .unwrap()
            .unwrap();
        assert!(response.starts_with("HTTP/1.1 413"), "{response}");
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        // Actual application upgrade route: permits last for the whole socket.
        use futures_util::{SinkExt, StreamExt};
        use tokio_tungstenite::{connect_async, tungstenite::Message};
        let websocket_url = format!("ws://{address}/ws/overlay");
        let mut sockets = Vec::new();
        for _ in 0..32 {
            let (mut socket, _) = connect_async(&websocket_url).await.unwrap();
            assert!(matches!(
                socket.next().await.unwrap().unwrap(),
                Message::Text(_)
            ));
            sockets.push(socket);
        }
        let error = connect_async(&websocket_url).await.unwrap_err();
        match error {
            tokio_tungstenite::tungstenite::Error::Http(response) => {
                assert_eq!(response.status(), 503)
            }
            _ => panic!("{error}"),
        }
        let mut oversized_socket = sockets.pop().unwrap();
        oversized_socket
            .send(Message::Binary(vec![0; 1025].into()))
            .await
            .unwrap();
        assert!(matches!(
            tokio::time::timeout(Duration::from_secs(2), oversized_socket.next())
                .await
                .unwrap(),
            Some(Err(_)) | None | Some(Ok(Message::Close(_)))
        ));
        let (mut replacement, _) = connect_async(&websocket_url).await.unwrap();
        assert!(matches!(
            replacement.next().await.unwrap().unwrap(),
            Message::Text(_)
        ));
        replacement.close(None).await.unwrap();
        for mut socket in sockets {
            socket.close(None).await.unwrap();
        }
        assert_eq!(ledger.feed_credit_sats().await.unwrap(), 0);
        assert_eq!(calls.load(Ordering::SeqCst), 0);

        let mut slow = tokio::net::TcpStream::connect(address).await.unwrap();
        slow.write_all(format!("POST /api/v1/strike/webhook HTTP/1.1\r\nHost: {address}\r\nContent-Type: application/json\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n100\r\nx").as_bytes()).await.unwrap();
        let mut timed_out = String::new();
        tokio::time::timeout(Duration::from_secs(45), slow.read_to_string(&mut timed_out))
            .await
            .unwrap()
            .unwrap();
        assert!(timed_out.starts_with("HTTP/1.1 408"), "{timed_out}");
        server.abort();
        provider_task.abort();
    }

    #[tokio::test]
    async fn http_concurrency_rejects_without_an_unbounded_wait_queue() {
        let gate = Arc::new(tokio::sync::Semaphore::new(0));
        let calls = Arc::new(AtomicUsize::new(0));
        let router = Router::new()
            .fallback({
                let gate = gate.clone();
                let calls = calls.clone();
                move || {
                    let gate = gate.clone();
                    let calls = calls.clone();
                    async move {
                        calls.fetch_add(1, Ordering::SeqCst);
                        gate.acquire_owned().await.unwrap().forget();
                        StatusCode::OK
                    }
                }
            })
            .layer(middleware::from_fn_with_state(
                Arc::new(tokio::sync::Semaphore::new(2)),
                bounded_http,
            ));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            lightning_goats::server::serve(listener, router)
                .await
                .unwrap()
        });
        let client = reqwest::Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(5))
            .build()
            .unwrap();
        let mut active = Vec::new();
        for _ in 0..2 {
            let client = client.clone();
            let url = url.clone();
            active.push(tokio::spawn(async move {
                client.get(url).send().await.unwrap()
            }));
        }
        tokio::time::timeout(Duration::from_secs(2), async {
            while calls.load(Ordering::SeqCst) < 2 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        assert_eq!(
            client.get(&url).send().await.unwrap().status(),
            StatusCode::SERVICE_UNAVAILABLE
        );
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        let lnurl = client
            .get(format!("{url}lnurlp/herd/callback?amount=1000"))
            .send()
            .await
            .unwrap();
        assert_eq!(lnurl.status(), StatusCode::SERVICE_UNAVAILABLE);
        let body: serde_json::Value = lnurl.json().await.unwrap();
        assert_eq!(body["status"], "ERROR");
        gate.add_permits(3);
        for response in active {
            assert_eq!(response.await.unwrap().status(), StatusCode::OK);
        }
        assert_eq!(
            client.get(url).send().await.unwrap().status(),
            StatusCode::OK
        );
        server.abort();
    }
}
