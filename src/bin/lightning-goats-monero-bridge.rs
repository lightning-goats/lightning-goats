#![forbid(unsafe_code)]
use anyhow::Result;
use clap::Parser;
use lightning_goats::monero_bridge::{Bridge, Config};
use std::path::PathBuf;

#[derive(Parser)]
struct Args {
    #[arg(long)]
    config: PathBuf,
    /// Validate non-secret configuration only. Does not open DB or call providers.
    #[arg(long)]
    check_config: bool,
}
#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    let config =
        Config::load(&args.config).map_err(|_| anyhow::anyhow!("bridge config rejected"))?;
    if args.check_config {
        println!("configuration valid; no services contacted");
        return Ok(());
    }
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::WARN)
        .init();
    let bridge = Bridge::open(config.clone()).await.map_err(|_| {
        anyhow::anyhow!("bridge startup failed; inspect private configuration/state")
    })?;
    let api = tokio::net::TcpListener::bind(config.listen).await?;
    let callback = tokio::net::TcpListener::bind(config.callback_listen).await?;
    let polling = bridge.clone();
    let worker = tokio::spawn(async move {
        loop {
            if polling.poll_once().await.is_err() {
                tracing::warn!("Monero receive polling unavailable");
            }
            tokio::time::sleep(std::time::Duration::from_secs(config.poll_seconds)).await;
        }
    });
    let result = tokio::select! {
        r = lightning_goats::server::serve(api, bridge.router()) => r,
        r = lightning_goats::server::serve(callback, bridge.callback_router()) => r,
        _ = shutdown() => Ok(()),
    };
    worker.abort();
    result
}
async fn shutdown() {
    let mut terminate = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        .expect("signal handler");
    tokio::select! { _ = tokio::signal::ctrl_c() => {}, _ = terminate.recv() => {} }
}
