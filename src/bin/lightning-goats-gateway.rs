#![forbid(unsafe_code)]

use std::path::PathBuf;

use anyhow::Result;
use clap::Parser;
use lightning_goats::gateway::{GatewayServerConfig, TrustedGateway};
use tracing::info;
use tracing_subscriber::EnvFilter;

#[derive(Debug, Parser)]
#[command(name = "lightning-goats-gateway")]
#[command(about = "Trusted in-house OpenHAB and weather gateway for Lightning Goats")]
struct Args {
    #[arg(long, default_value = "/etc/lightning-goats-gateway/config.toml")]
    config: PathBuf,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let args = Args::parse();
    let config = GatewayServerConfig::load(&args.config)?;
    let gateway = TrustedGateway::from_config(&config).await?;
    let listen = gateway.listen();
    let listener = tokio::net::TcpListener::bind(listen).await?;
    info!(%listen, "trusted Lightning Goats integration gateway listening");

    axum::serve(listener, gateway.router())
        .with_graceful_shutdown(shutdown_signal())
        .await
        .map_err(anyhow::Error::from)
}

async fn shutdown_signal() {
    if let Err(error) = tokio::signal::ctrl_c().await {
        tracing::error!(%error, "failed to install shutdown signal handler");
    }
}
