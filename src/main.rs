#![forbid(unsafe_code)]

use std::{path::PathBuf, sync::Arc, time::Duration};

use anyhow::{Result, anyhow, bail};
use axum::{Json, Router, extract::State, http::StatusCode, routing::get};
use clap::Parser;
use lightning_goats::{
    cln::ClnRestClient, config::AppConfig, feeder::run_feed_worker,
    invoice_watcher::run_invoice_watcher, ledger::LedgerStore, openhab::OpenHabClient,
};
use serde::Serialize;
use tracing::info;
use tracing_subscriber::EnvFilter;

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
}

#[derive(Debug, Serialize)]
struct HealthResponse {
    status: &'static str,
}

#[derive(Debug, Serialize)]
struct StatusResponse {
    mode: &'static str,
    herd_user: String,
    last_pay_index: u64,
    feed_credit_sats: u64,
    threshold_sats: u64,
    feeds_due: u64,
    remainder_sats: u64,
    unresolved_feed_attempt: Option<String>,
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

    let last_pay_index = ledger.last_pay_index().await?;
    if last_pay_index.is_none() {
        bail!(
            "CLN pay-index cursor is uninitialized; run lightning-goatsctl init-cursor before starting lightning-goatsd"
        );
    }

    let interrupted = ledger.mark_interrupted_feed_intents_unknown().await?;
    if interrupted > 0 {
        tracing::error!(
            interrupted,
            "converted interrupted feed intent(s) to unknown; operator reconciliation is required"
        );
    }

    let cln = ClnRestClient::from_config(&config.lightning).await?;
    let openhab = OpenHabClient::from_config(&config.openhab).await?;

    let state = AppState {
        config: Arc::clone(&config),
        ledger: ledger.clone(),
    };
    let app = Router::new()
        .route("/healthz", get(healthz))
        .route("/api/v1/status", get(status))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(config.service.listen).await?;
    info!(
        listen = %config.service.listen,
        mode = config.service.mode.as_str(),
        "lightning-goatsd listening"
    );

    let mut server = tokio::spawn(async move {
        axum::serve(listener, app)
            .await
            .map_err(anyhow::Error::from)
    });
    let mut watcher = tokio::spawn(run_invoice_watcher(
        cln,
        ledger.clone(),
        config.lightning.herd_user.clone(),
    ));
    let mut feeder = tokio::spawn(run_feed_worker(
        ledger,
        openhab,
        config.feeder.threshold_sats,
        Duration::from_secs(config.feeder.inter_feed_delay_seconds),
        config.service.mode,
    ));

    let result = tokio::select! {
        _ = shutdown_signal() => {
            info!("shutdown signal received");
            Ok(())
        }
        result = &mut server => task_exit("HTTP server", result),
        result = &mut watcher => task_exit("paid-invoice watcher", result),
        result = &mut feeder => task_exit("feed worker", result),
    };

    server.abort();
    watcher.abort();
    feeder.abort();
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
        .last_pay_index()
        .await
        .map_err(|error| {
            tracing::error!(%error, "failed reading CLN cursor for status endpoint");
            StatusCode::INTERNAL_SERVER_ERROR
        })?
        .ok_or(StatusCode::SERVICE_UNAVAILABLE)?;
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

    Ok(Json(StatusResponse {
        mode: state.config.service.mode.as_str(),
        herd_user: state.config.lightning.herd_user.clone(),
        last_pay_index,
        feed_credit_sats,
        threshold_sats,
        feeds_due: feed_credit_sats / threshold_sats,
        remainder_sats: feed_credit_sats % threshold_sats,
        unresolved_feed_attempt,
    }))
}

async fn shutdown_signal() {
    if let Err(error) = tokio::signal::ctrl_c().await {
        tracing::error!(%error, "failed to install shutdown signal handler");
    }
}
