#![forbid(unsafe_code)]
use anyhow::Result;
use clap::Parser;
use std::path::PathBuf;
#[derive(Parser)]
#[command(about = "Isolated CyberHerd payout worker; no public ingress or feeder capability")]
struct Args {
    #[arg(long, default_value = "/etc/cyberherd-payout/config.toml")]
    config: PathBuf,
    #[arg(long, conflicts_with = "initialize_state")]
    check: bool,
    /// Initialize only a fresh disabled zero-budget journal; no sender credential.
    #[arg(long)]
    initialize_state: bool,
    /// Migrate only an existing disabled payout journal; never start a worker.
    #[arg(long, requires="writers_stopped", conflicts_with_all=["check","initialize_state","sync_failed_cost"])]
    migrate_inclusive_fees: bool,
    #[arg(long, requires = "migrate_inclusive_fees")]
    writers_stopped: bool,
    /// Read one original failed-cost receipt from core and acknowledge it locally.
    /// Requires the payout UID; does not load a spending credential or send money.
    #[arg(long,requires_all=["grant","member"],conflicts_with_all=["check","initialize_state"])]
    sync_failed_cost: bool,
    #[arg(long, requires = "sync_failed_cost")]
    grant: Option<uuid::Uuid>,
    #[arg(long, requires = "sync_failed_cost")]
    member: Option<String>,
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
    if args.initialize_state {
        config.initialize_state(&args.config).await?;
        println!("New payout journal initialized; no runtime started");
        return Ok(());
    }
    if args.migrate_inclusive_fees {
        config
            .migrate_inclusive_fees_offline(&args.config, args.writers_stopped)
            .await?;
        println!("Existing payout journal migrated; no runtime, credential or transport started");
        return Ok(());
    }
    if args.sync_failed_cost {
        let member = args
            .member
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("member required"))?;
        lightning_goats::nostr_event::validate_public_key(member)?;
        let grant = args
            .grant
            .ok_or_else(|| anyhow::anyhow!("grant required"))?;
        if grant.is_nil() {
            anyhow::bail!("grant must be non-nil");
        }
        let (store, core) = config.recovery_store(&args.config).await?;
        let receipt = store.sync_failed_cost(&core, grant, member).await?;
        println!(
            "{}",
            serde_json::json!({"grant":grant,"member":member,"reconciled":receipt.is_some(),"held_sats":store.held_sats().await?,"payment_sent":false})
        );
        return Ok(());
    }
    config.open(&args.config).await?.run().await
}
#[cfg(not(unix))]
fn main() -> Result<()> {
    anyhow::bail!("payout worker requires Unix peer authentication")
}
