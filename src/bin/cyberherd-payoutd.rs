#![forbid(unsafe_code)]
use anyhow::Result;
use clap::Parser;
use std::path::PathBuf;
#[derive(Parser)]
#[command(about = "Isolated CyberHerd payout worker; no public ingress or feeder capability")]
struct Args {
    #[arg(long, default_value = "/etc/cyberherd-payout/config.toml")]
    config: PathBuf,
    #[arg(long)]
    check: bool,
}
#[cfg(unix)]
#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();
    let args = Args::parse();
    let config = lightning_goats::payout::runtime::Config::load(&args.config)?;
    if args.check {
        println!("Payout configuration is valid; no runtime started");
        return Ok(());
    }
    config.open(&args.config).await?.run().await
}
#[cfg(not(unix))]
fn main() -> Result<()> {
    anyhow::bail!("payout worker requires Unix peer authentication")
}
