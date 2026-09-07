#![forbid(unsafe_code)]

use std::{future::pending, path::PathBuf, sync::Arc, time::Duration};

use anyhow::{Result, anyhow};
use axum::{
    Json, Router,
    body::Bytes,
    extract::{State, ws::WebSocketUpgrade},
    http::{HeaderMap, StatusCode},
    response::Response,
    routing::{get, post},
};
use clap::Parser;
use lightning_goats::{
    cln::ClnRestClient,
    config::AppConfig,
    feeder::run_feed_worker,
    invoice_watcher::run_invoice_watcher,
    ledger::{LedgerStore, SettlementOutcome},
    messaging::{run_message_processor, run_outbox_publisher},
    nostr::NakClient,
    openhab::OpenHabClient,
    overlay::serve_overlay_socket,
    strike::StrikeRuntime,
};
use serde::Serialize;
use tracing::info;
use tracing_subscriber::EnvFilter;

const MAX_STRIKE_WEBHOOK_BODY: usize = 32 * 1024;

#[derive(Debug, Parser)]
#[command(name = "lightning-goatsd")]
#[command(about = "Lightning Goats payment accounting and feeder automation service")]
struct Args {
    #[arg(long, default_value = "/etc/lightning-goats/config.toml")]
    config: PathBuf,
}

#[derive(Clone)]
struct AppState {
    config: Arc<AppConfig>,
    ledger: LedgerStore,
    openhab: OpenHabClient,
    strike: Option<StrikeRuntime>,
}

#[derive(Debug, Serialize)]
struct HealthResponse {
    status: &'static str,
}

#[derive(Debug, Serialize)]
struct StatusResponse {
    mode: &'static str,
    herd_user: String,
    strike_enabled: bool,
    /// Transitional compatibility state; `None` means the legacy CLN watcher is disabled.
    last_pay_index: Option<u64>,
    feed_credit_sats: u64,
    threshold_sats: u64,
    feeds_due: u64,
    remainder_sats: u64,
    unresolved_feed_attempt: Option<String>,
    feeder_override_active: Option<bool>,
    temperature_f: Option<f64>,
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

    let interrupted = ledger.mark_interrupted_feed_intents_unknown().await?;
    if interrupted > 0 {
        tracing::error!(
            interrupted,
            "converted interrupted feed intent(s) to unknown; operator reconciliation is required"
        );
    }

    let legacy_cln_cursor = ledger.last_legacy_cln_pay_index().await?;
    let openhab = OpenHabClient::from_config(&config.openhab).await?;
    let strike = match &config.strike {
        Some(strike_config) => Some(StrikeRuntime::from_config(strike_config).await?),
        None => None,
    };
    let nostr = if config.service.mode.nostr_enabled() {
        Some(NakClient::from_config(&config.nostr).await?)
    } else {
        None
    };

    let state = AppState {
        config: Arc::clone(&config),
        ledger: ledger.clone(),
        openhab: openhab.clone(),
        strike,
    };
    let app = Router::new()
        .route("/healthz", get(healthz))
        .route("/api/v1/status", get(status))
        .route("/api/v1/strike/webhook", post(strike_webhook))
        .route("/ws/overlay", get(overlay_ws))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(config.service.listen).await?;
    info!(
        listen = %config.service.listen,
        mode = config.service.mode.as_str(),
        strike_enabled = config.strike.is_some(),
        "lightning-goatsd listening"
    );

    let mut server = tokio::spawn(async move {
        axum::serve(listener, app)
            .await
            .map_err(anyhow::Error::from)
    });

    let watcher_config = Arc::clone(&config);
    let watcher_ledger = ledger.clone();
    let mut watcher = tokio::spawn(async move {
        if legacy_cln_cursor.is_some() {
            let cln = ClnRestClient::from_config(&watcher_config.lightning).await?;
            run_invoice_watcher(
                cln,
                watcher_ledger,
                watcher_config.lightning.herd_user.clone(),
            )
            .await
        } else {
            tracing::info!(
                "legacy CLN cursor is uninitialized; compatibility watcher disabled while provider-neutral service remains available"
            );
            pending::<Result<()>>().await
        }
    });
    let mut feeder = tokio::spawn(run_feed_worker(
        ledger.clone(),
        openhab,
        config.feeder.threshold_sats,
        Duration::from_secs(config.feeder.inter_feed_delay_seconds),
        config.service.mode,
    ));
    let mut message_processor = tokio::spawn(run_message_processor(
        ledger.clone(),
        nostr.clone(),
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
        result = &mut watcher => task_exit("legacy paid-invoice watcher", result),
        result = &mut feeder => task_exit("feed worker", result),
        result = &mut message_processor => task_exit("Nostr message processor", result),
        result = &mut publisher => task_exit("Nostr outbox publisher", result),
    };

    server.abort();
    watcher.abort();
    feeder.abort();
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
    let last_pay_index = state
        .ledger
        .last_legacy_cln_pay_index()
        .await
        .map_err(|error| {
            tracing::error!(%error, "failed reading legacy CLN cursor for status endpoint");
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

    let feeder_override_active = match state.openhab.feeder_override_enabled().await {
        Ok(value) => Some(value),
        Err(error) => {
            tracing::warn!(%error, "OpenHAB FeederOverride unavailable for status endpoint");
            None
        }
    };
    let temperature_f = match state.openhab.temperature_f().await {
        Ok(value) => value,
        Err(error) => {
            tracing::warn!(%error, "OpenHAB temperature unavailable for status endpoint");
            None
        }
    };

    Ok(Json(StatusResponse {
        mode: state.config.service.mode.as_str(),
        herd_user: state.config.lightning.herd_user.clone(),
        strike_enabled: state.strike.is_some(),
        last_pay_index,
        feed_credit_sats,
        threshold_sats,
        feeds_due: feed_credit_sats / threshold_sats,
        remainder_sats: feed_credit_sats % threshold_sats,
        unresolved_feed_attempt,
        feeder_override_active,
        temperature_f,
    }))
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
    let Some(strike) = state.strike.as_ref() else {
        return StatusCode::SERVICE_UNAVAILABLE;
    };
    let Some(signature) = headers
        .get("x-webhook-signature")
        .and_then(|value| value.to_str().ok())
    else {
        tracing::warn!("rejected Strike webhook without a valid signature header");
        return StatusCode::UNAUTHORIZED;
    };

    if let Err(error) = strike.verify_webhook_signature(&body, signature) {
        tracing::warn!(%error, "rejected Strike webhook with invalid signature");
        return StatusCode::UNAUTHORIZED;
    }
    let event = match strike.parse_completed_event(&body) {
        Ok(event) => event,
        Err(error) => {
            tracing::warn!(%error, "rejected unsupported or malformed signed Strike webhook");
            return StatusCode::BAD_REQUEST;
        }
    };

    match strike.reconcile_and_credit(&state.ledger, &event).await {
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
    let threshold_sats = state.config.feeder.threshold_sats;
    ws.on_upgrade(move |socket| async move {
        if let Err(error) = serve_overlay_socket(socket, ledger, threshold_sats).await {
            tracing::warn!(%error, "overlay websocket disconnected after server-side error");
        }
    })
}

async fn shutdown_signal() {
    if let Err(error) = tokio::signal::ctrl_c().await {
        tracing::error!(%error, "failed to install shutdown signal handler");
    }
}
