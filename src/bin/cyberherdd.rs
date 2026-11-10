#![forbid(unsafe_code)]
use anyhow::Result;
use clap::Parser;
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    about = "Separate CyberHerd membership, optional presentation and operator service (no payouts)"
)]
struct Args {
    #[arg(long, default_value = "/etc/cyberherd/config.toml")]
    config: PathBuf,
    /// Validate configuration without opening databases or contacting services.
    #[arg(long)]
    check: bool,
}

#[cfg(unix)]
#[tokio::main]
async fn main() -> Result<()> {
    use lightning_goats::cyberherd::runtime::{Config, Runtime};
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();
    let args = Args::parse();
    let config = Config::load(&args.config)?;
    if args.check {
        println!("CyberHerd configuration is valid; no runtime started");
        return Ok(());
    }
    Runtime::open(config, &args.config).await?.run().await
}

#[cfg(not(unix))]
fn main() -> Result<()> {
    anyhow::bail!("CyberHerd requires Unix peer authentication")
}
