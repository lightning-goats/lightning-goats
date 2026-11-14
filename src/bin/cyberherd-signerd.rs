#![forbid(unsafe_code)]
use anyhow::Result;
use clap::Parser;
use std::path::PathBuf;
#[derive(Parser)]
#[command(about = "Purpose-limited outgoing CyberHerd NIP-46 signing broker")]
struct Args {
    #[arg(long, default_value = "/etc/cyberherd-signer/config.toml")]
    config: PathBuf,
    #[arg(long, conflicts_with = "initialize_state")]
    check: bool,
    /// Initialize only a fresh disabled zero-admission journal; no key or signer.
    #[arg(long)]
    initialize_state: bool,
}
#[cfg(unix)]
#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    let config = lightning_goats::payout::signer::Config::load(&args.config)?;
    if args.check {
        println!("Outgoing signer configuration is valid; no runtime started");
        return Ok(());
    }
    if args.initialize_state {
        config.initialize_state(&args.config).await?;
        println!("New outgoing signer journal initialized; no runtime started");
        return Ok(());
    }
    config.run(&args.config).await
}
#[cfg(not(unix))]
fn main() -> Result<()> {
    anyhow::bail!("signer requires Unix peer authentication")
}
