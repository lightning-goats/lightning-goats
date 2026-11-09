#![forbid(unsafe_code)]

use std::path::PathBuf;

use anyhow::{Result, bail};
use clap::{Parser, Subcommand, ValueEnum};
use lightning_goats::{
    config::AppConfig,
    ledger::{LedgerStore, OperatorCreditGrant},
    presentation::PAYMENT_MESSAGE_MIN_SATS,
};
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
    /// Invalidate overlay cursors after offline database restore, before restarting the daemon.
    ResetOverlayStream,
    /// Add local feed credit without a Lightning payment. May cause real feeding.
    AddCredit {
        /// Stable unique grant reference. Reuse it for a safe retry.
        #[arg(long)]
        id: String,
        #[arg(long)]
        sats: u64,
        #[arg(long, default_value = "herd")]
        address: String,
        /// Private audit explanation; not included in public messages.
        #[arg(long)]
        reason: String,
        /// Validate and show prospective credit without granting it.
        #[arg(long)]
        dry_run: bool,
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
        Command::AddCredit {
            id,
            sats,
            address,
            reason,
            dry_run,
        } => {
            if !config
                .lightning_address
                .iter()
                .any(|entry| entry.user == address && entry.credit_pool == "herd")
            {
                bail!("address must be a configured herd-pool Lightning Address username");
            }
            let result = ledger
                .grant_operator_credit(
                    &OperatorCreditGrant {
                        id: id.clone(),
                        amount_sats: sats,
                        address_user: address,
                        reason,
                    },
                    dry_run,
                )
                .await?;
            println!(
                "{}",
                serde_json::json!({
                    "status":result.status.as_str(), "grant_id":id, "amount_sats":sats,
                    "feed_credit_at_grant_sats":result.feed_credit_sats,
                    "feeds_due_at_grant":result.feed_credit_sats / config.feeder.threshold_sats,
                    "payment_message_eligible":sats >= PAYMENT_MESSAGE_MIN_SATS,
                    "mode":config.service.mode.as_str(), "nostr_enabled":config.service.mode.nostr_enabled(),
                    "source":"operator_credit"
                })
            );
        }
        Command::ResetOverlayStream => {
            let id = ledger.reset_overlay_stream().await?;
            println!("overlay_stream_id={id}");
        }
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
