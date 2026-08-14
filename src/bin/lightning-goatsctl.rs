#![forbid(unsafe_code)]

use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand, ValueEnum};
use lightning_goats::{
    config::AppConfig,
    ledger::{LedgerStore, LegacyCutoverManifest, LegacyPendingInvoice, LegacySettledInvoice},
};
use serde::Deserialize;
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
    /// Initialize the durable CLN pay-index cursor exactly once.
    InitCursor {
        /// Last CLN pay_index that is already accounted for. The next observed
        /// settlement must have a greater pay_index.
        #[arg(long)]
        pay_index: u64,
    },
    /// Resolve an ambiguous feed attempt without directly actuating the feeder.
    ReconcileFeed {
        #[arg(long)]
        id: Uuid,
        #[arg(long, value_enum)]
        outcome: ReconcileOutcome,
    },
    /// Atomically install the stable pre-cutover LNbits wallet/pending-invoice snapshot.
    LegacyInstallManifest {
        #[arg(long)]
        manifest: PathBuf,
    },
    /// Verify that a saved cutover manifest exactly matches the one installed in SQLite.
    LegacyVerifyManifest {
        #[arg(long)]
        manifest: PathBuf,
    },
    /// Import one paid legacy invoice that was allowlisted in the cutover manifest.
    LegacyImportSettlement {
        #[arg(long)]
        settlement: PathBuf,
    },
    /// Print the current durable feed-credit accounting state.
    Status,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum ReconcileOutcome {
    Fed,
    NotFed,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct LegacyManifestFile {
    wallet_id: String,
    opening_credit_sats: u64,
    cutover_at: i64,
    snapshot_at: i64,
    pending_invoices: Vec<LegacyPendingFile>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct LegacyPendingFile {
    payment_hash: String,
    checking_id: Option<String>,
    wallet_id: String,
    amount_sats: u64,
    created_at: Option<i64>,
    expiry_at: Option<i64>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct LegacySettlementFile {
    payment_hash: String,
    checking_id: Option<String>,
    wallet_id: String,
    amount_sats: u64,
    settled_at: Option<i64>,
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
        Command::LegacyInstallManifest { manifest } => {
            let manifest = read_legacy_manifest(&manifest).await?;
            let outcome = ledger.install_legacy_cutover_manifest(&manifest).await?;
            println!("legacy_cutover_manifest={outcome:?}");
            println!("feed_credit_sats={}", ledger.feed_credit_sats().await?);
        }
        Command::LegacyVerifyManifest { manifest } => {
            let manifest = read_legacy_manifest(&manifest).await?;
            ledger.verify_legacy_cutover_manifest(&manifest).await?;
            println!("legacy_cutover_manifest=verified");
        }
        Command::LegacyImportSettlement { settlement } => {
            let settlement: LegacySettlementFile = read_json_file(&settlement).await?;
            let settlement = LegacySettledInvoice {
                payment_hash: settlement.payment_hash,
                checking_id: settlement.checking_id,
                wallet_id: settlement.wallet_id,
                amount_sats: settlement.amount_sats,
                settled_at: settlement.settled_at,
            };
            let outcome = ledger.import_legacy_settlement(&settlement).await?;
            println!("legacy_settlement={outcome:?}");
            println!("feed_credit_sats={}", ledger.feed_credit_sats().await?);
        }
        Command::Status => {
            let credit = ledger.feed_credit_sats().await?;
            let threshold = config.feeder.threshold_sats;
            let cursor = ledger.last_pay_index().await?;
            let unresolved = ledger.unresolved_feed_attempt().await?;
            println!("mode={}", config.service.mode.as_str());
            println!("herd_user={}", config.lightning.herd_user);
            println!(
                "last_pay_index={}",
                cursor.map_or_else(|| "uninitialized".to_owned(), |value| value.to_string())
            );
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

async fn read_legacy_manifest(path: &PathBuf) -> Result<LegacyCutoverManifest> {
    let manifest: LegacyManifestFile = read_json_file(path).await?;
    Ok(LegacyCutoverManifest {
        wallet_id: manifest.wallet_id,
        opening_credit_sats: manifest.opening_credit_sats,
        cutover_at: manifest.cutover_at,
        snapshot_at: manifest.snapshot_at,
        pending_invoices: manifest
            .pending_invoices
            .into_iter()
            .map(|pending| LegacyPendingInvoice {
                payment_hash: pending.payment_hash,
                checking_id: pending.checking_id,
                wallet_id: pending.wallet_id,
                amount_sats: pending.amount_sats,
                created_at: pending.created_at,
                expiry_at: pending.expiry_at,
            })
            .collect(),
    })
}

async fn read_json_file<T>(path: &PathBuf) -> Result<T>
where
    T: for<'de> Deserialize<'de>,
{
    let raw = tokio::fs::read_to_string(path)
        .await
        .with_context(|| format!("failed reading {}", path.display()))?;
    serde_json::from_str(&raw)
        .with_context(|| format!("failed parsing JSON from {}", path.display()))
}
