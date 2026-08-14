#![forbid(unsafe_code)]

use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, Subcommand};
use lightning_goats::{config::AppConfig, ledger::LedgerStore};

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
    /// Initialize the durable CLN pay-index cursor exactly once.
    InitCursor {
        /// Last CLN pay_index that is already accounted for. The next observed
        /// settlement must have a greater pay_index.
        #[arg(long)]
        pay_index: u64,
    },
    /// Print the current durable feed-credit accounting state.
    Status,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    let config = AppConfig::load(&args.config)?;
    let ledger = LedgerStore::connect(&config.database.url).await?;

    match args.command {
        Command::InitCursor { pay_index } => {
            ledger.initialize_cursor(pay_index).await?;
            println!("initialized CLN pay-index cursor at {pay_index}");
        }
        Command::Status => {
            let credit = ledger.feed_credit_sats().await?;
            let threshold = config.feeder.threshold_sats;
            let cursor = ledger.last_pay_index().await?;
            println!("mode={}", config.service.mode.as_str());
            println!("herd_user={}", config.lightning.herd_user);
            println!("last_pay_index={}", cursor.map_or_else(|| "uninitialized".to_owned(), |value| value.to_string()));
            println!("feed_credit_sats={credit}");
            println!("threshold_sats={threshold}");
            println!("feeds_due={}", credit / threshold);
            println!("remainder_sats={}", credit % threshold);
        }
    }

    Ok(())
}
