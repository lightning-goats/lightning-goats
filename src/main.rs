#![forbid(unsafe_code)]

use std::{path::PathBuf, sync::Arc};

use anyhow::Result;
use axum::{Json, Router, extract::State, routing::get};
use clap::Parser;
use lightning_goats::config::AppConfig;
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

#[derive(Debug, Serialize)]
struct HealthResponse {
    status: &'static str,
}

#[derive(Debug, Serialize)]
struct StatusResponse {
    mode: &'static str,
    herd_user: String,
    threshold_sats: u64,
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

    let app = Router::new()
        .route("/healthz", get(healthz))
        .route("/api/v1/status", get(status))
        .with_state(Arc::clone(&config));

    let listener = tokio::net::TcpListener::bind(config.service.listen).await?;
    info!(
        listen = %config.service.listen,
        mode = config.service.mode.as_str(),
        "lightning-goatsd listening"
    );

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    Ok(())
}

async fn healthz() -> Json<HealthResponse> {
    Json(HealthResponse { status: "ok" })
}

async fn status(State(config): State<Arc<AppConfig>>) -> Json<StatusResponse> {
    Json(StatusResponse {
        mode: config.service.mode.as_str(),
        herd_user: config.lightning.herd_user.clone(),
        threshold_sats: config.feeder.threshold_sats,
    })
}

async fn shutdown_signal() {
    if let Err(error) = tokio::signal::ctrl_c().await {
        tracing::error!(%error, "failed to install shutdown signal handler");
    }
}
