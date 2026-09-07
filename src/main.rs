#![forbid(unsafe_code)]

use std::{future::pending, path::PathBuf, sync::Arc, time::Duration};

use anyhow::{Result, anyhow};
use axum::{
    Json, Router,
    body::Bytes,
    extract::{Path, Query, State, ws::WebSocketUpgrade},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use clap::Parser;
use lightning_goats::{
    config::AppConfig,
    feeder::run_feed_worker,
    gateway::GatewayClient,
    informational::run_informational_worker,
    ledger::{LedgerStore, SettlementOutcome},
    lnurl::{LnurlErrorResponse, LnurlService, LnurlServiceError},
    messaging::{run_message_processor, run_outbox_publisher},
    nostr::NakClient,
    overlay::serve_overlay_socket,
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
        strike,
        lnurl,
        renderer: renderer.clone(),
    };
    let app = Router::new()
        .route("/healthz", get(healthz))
        .route("/api/v1/status", get(status))
        .route("/api/v1/strike/webhook", post(strike_webhook))
        .route("/.well-known/lnurlp/{user}", get(lnurl_discovery))
        .route("/lnurlp/{user}/callback", get(lnurl_callback))
        .route("/ws/overlay", get(overlay_ws))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(config.service.listen).await?;
    info!(
        listen = %config.service.listen,
        mode = config.service.mode.as_str(),
        payment_backend = "strike",
        gateway = %config.gateway.url,
        "lightning-goatsd listening"
    );

    let mut server = tokio::spawn(async move {
        axum::serve(listener, app)
            .await
            .map_err(anyhow::Error::from)
    });
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
        result = &mut feeder => task_exit("feed worker", result),
        result = &mut informational => task_exit("informational worker", result),
        result = &mut message_processor => task_exit("Nostr message processor", result),
        result = &mut publisher => task_exit("Nostr outbox publisher", result),
    };

    server.abort();
    feeder.abort();
    informational.abort();
    message_processor.abort();
    publisher.abort();
    result
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

    match state
        .strike
        .reconcile_and_credit(&state.ledger, &event)
        .await
    {
        Ok(SettlementOutcome::Credited {
            sats,
            address_user,
            credit_pool,
        }) => {
            tracing::info!(
                sats,
                %address_user,
                %credit_pool,
                receive_request_id = %event.receive_request_id,
                receive_id = %event.receive_id,
                "credited authoritative Strike receive"
            );
            StatusCode::NO_CONTENT
        }
        Ok(SettlementOutcome::Duplicate) => {
            tracing::debug!(
                receive_id = %event.receive_id,
                "duplicate Strike completed-receive webhook was idempotent"
            );
            StatusCode::NO_CONTENT
        }
        Err(error) => {
            tracing::error!(
                %error,
                receive_request_id = %event.receive_request_id,
                receive_id = %event.receive_id,
                "Strike webhook could not be reconciled; returning retryable failure"
            );
            StatusCode::BAD_GATEWAY
        }
    }
}

async fn overlay_ws(ws: WebSocketUpgrade, State(state): State<AppState>) -> Response {
    let ledger = state.ledger.clone();
    let renderer = state.renderer.clone();
    let threshold_sats = state.config.feeder.threshold_sats;
    ws.on_upgrade(move |socket| async move {
        if let Err(error) = serve_overlay_socket(socket, ledger, renderer, threshold_sats).await {
            tracing::warn!(%error, "overlay websocket disconnected after server-side error");
        }
    })
}

async fn shutdown_signal() {
    if let Err(error) = tokio::signal::ctrl_c().await {
        tracing::error!(%error, "failed to install shutdown signal handler");
    }
}
