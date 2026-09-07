#![forbid(unsafe_code)]

use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, Subcommand, ValueEnum};
use lightning_goats::{config::AppConfig, ledger::LedgerStore};
use uuid::Uuid;

#[derive(Debug, Parser)]
#[command(name = "lightning-goatsctl")]
#[command(about = "Local operator controls for Lightning Goats")]
struct Args {
    #[arg(long, default_value = "/etc/lightning-goats/config.toml")]
    config: PathBuf,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Resolve an ambiguous feed attempt without directly actuating the feeder.
    ReconcileFeed {
        #[arg(long)]
        id: Uuid,
        #[arg(long, value_enum)]
        outcome: ReconcileOutcome,
    },
    /// Print the current durable feed-credit accounting state.
    Status,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum ReconcileOutcome {
    Fed,
    NotFed,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    let config = AppConfig::load(&args.config)?;
    let ledger = LedgerStore::connect(&config.database.url).await?;

    match args.command {
        Command::ReconcileFeed { id, outcome } => match outcome {
            ReconcileOutcome::Fed => {
                ledger.reconcile_unknown_as_fed(id).await?;
                println!("reconciled feed attempt {id} as physically fed");
            }
            ReconcileOutcome::NotFed => {
                ledger.reconcile_unknown_as_not_fed(id).await?;
                println!("reconciled feed attempt {id} as not physically fed");
            }
        },
        Command::Status => {
            let credit = ledger.feed_credit_sats().await?;
            let threshold = config.feeder.threshold_sats;
            let unresolved = ledger.unresolved_feed_attempt().await?;
            println!("mode={}", config.service.mode.as_str());
            println!("payment_backend=strike");
            println!("feed_credit_sats={credit}");
            println!("threshold_sats={threshold}");
            println!("feeds_due={}", credit / threshold);
            println!("remainder_sats={}", credit % threshold);
            if let Some(attempt) = unresolved {
                println!("unresolved_feed_attempt_id={}", attempt.id);
                println!("unresolved_feed_attempt_status={:?}", attempt.status);
            } else {
                println!("unresolved_feed_attempt_id=none");
            }
        }
    }

    Ok(())
}
