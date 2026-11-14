#![forbid(unsafe_code)]
use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    about = "Offline four-journal backup, verification and blocked restore staging; no payment or service activation"
)]
struct Args {
    #[arg(long, default_value = "/etc/cyberherd/state-maintenance.toml")]
    config: PathBuf,
    #[command(subcommand)]
    command: Command,
}
#[derive(Subcommand)]
enum Command {
    /// Check policy fields only, without state, credentials or service access.
    Check,
    /// Copy all four stopped/masked journal writers into a new private bundle.
    Snapshot {
        #[arg(long)]
        destination: PathBuf,
        #[arg(long, required = true)]
        writers_stopped: bool,
    },
    /// Verify a private bundle. Its manifest contains the detailed retention report.
    Verify {
        #[arg(long)]
        bundle: PathBuf,
    },
    /// Review retained reward backlog in an accepted backup; never live journals.
    ReviewRewards {
        #[arg(long)]
        bundle: PathBuf,
        #[arg(long)]
        destination: PathBuf,
    },
    /// Export all known reconciliation identities from an accepted backup.
    /// Never contacts a provider, clears markers or authorizes resumption.
    InventoryRecovery {
        #[arg(long)]
        bundle: PathBuf,
        #[arg(long)]
        destination: PathBuf,
    },
    /// Derive a private initial reward cutoff preview from an accepted backup.
    /// Never applies a cutoff, opens live journals or authorizes payouts.
    PreviewInitialRewardCutover {
        #[arg(long)]
        bundle: PathBuf,
        #[arg(long)]
        destination: PathBuf,
        #[arg(long)]
        decision_id: uuid::Uuid,
    },
    /// Compare a reviewed preview against a separate already-disposed copy.
    /// Never writes live state, grants activation or starts services.
    CompareInitialRewardCutover {
        #[arg(long)]
        bundle: PathBuf,
        #[arg(long)]
        preview: PathBuf,
        #[arg(long)]
        preview_sha256: String,
        #[arg(long)]
        candidate_copy: PathBuf,
    },
    /// Copy only the exact compiled schema-23 source for the contact upgrade.
    /// Same four stopped/masked writers and file guards; never restore or resume.
    SnapshotBeforeContact {
        #[arg(long)]
        destination: PathBuf,
        #[arg(long, required = true)]
        writers_stopped: bool,
    },
    /// Verify an exact schema-23 contact-upgrade source; never ordinary restore.
    VerifyContactUpgradeSource {
        #[arg(long)]
        bundle: PathBuf,
    },
    /// Compare a known schema-23 backup with an already migrated schema-25 copy.
    /// Never migrates, restores, clears markers or starts a service.
    CompareContactUpgrade {
        #[arg(long)]
        bundle: PathBuf,
        #[arg(long)]
        candidate_copy: PathBuf,
    },
    /// Snapshot only the exact schema-24 source with all four writers stopped.
    SnapshotBeforeFeederCap {
        #[arg(long)]
        destination: PathBuf,
        #[arg(long, required = true)]
        writers_stopped: bool,
    },
    /// Verify the exact schema-24 source; never restore or resume it.
    VerifyFeederCapUpgradeSource {
        #[arg(long)]
        bundle: PathBuf,
    },
    /// Compare retained schema-24 state with its additive schema-25 migration.
    CompareFeederCapUpgrade {
        #[arg(long)]
        bundle: PathBuf,
        #[arg(long)]
        candidate_copy: PathBuf,
    },
    /// Snapshot the exact payout-schema-8 source with all four writers masked.
    SnapshotBeforeInclusiveFees {
        #[arg(long)]
        destination: PathBuf,
        #[arg(long, required = true)]
        writers_stopped: bool,
    },
    /// Verify only the exact payout-schema-8 source of an inclusive-fee upgrade.
    VerifyInclusiveFeeUpgradeSource {
        #[arg(long)]
        bundle: PathBuf,
    },
    /// Compare an exact old payout backup with a separately migrated copy.
    CompareInclusiveFeeUpgrade {
        #[arg(long)]
        bundle: PathBuf,
        #[arg(long)]
        candidate_copy: PathBuf,
    },
    /// Stage all four journals in new directories with mandatory startup markers.
    StageRestore {
        #[arg(long)]
        bundle: PathBuf,
        #[arg(long)]
        destination: PathBuf,
        #[arg(long, required = true)]
        writers_stopped: bool,
    },
}
#[cfg(unix)]
#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    if matches!(args.command, Command::Check) {
        lightning_goats::state_maintenance::Policy::load_for_check(&args.config)?;
        println!("State maintenance policy is valid; no state, credentials or services accessed");
        return Ok(());
    }
    let operator = lightning_goats::state_maintenance::Operator::load(&args.config)?;
    let (operation, manifest) = match args.command {
        Command::Snapshot {
            destination,
            writers_stopped: true,
        } => ("snapshot", operator.snapshot(&destination).await?),
        Command::Verify { bundle } => ("verify", operator.verify(&bundle).await?),
        Command::ReviewRewards {
            bundle,
            destination,
        } => {
            let review = operator.review_rewards(&bundle, &destination).await?;
            // Counts, UUIDs and financial scope stay in the private report.
            println!(
                "{}",
                serde_json::json!({"operation":"review_rewards","id":review.id,
                "accepted":true,"activation_authorized":false})
            );
            return Ok(());
        }
        Command::InventoryRecovery {
            bundle,
            destination,
        } => {
            let inventory = operator.inventory_recovery(&bundle, &destination).await?;
            println!(
                "{}",
                serde_json::json!({"operation":"inventory_recovery","id":inventory.id,"accepted":true,"adoption_authorized":false,"resume_authorized":false})
            );
            return Ok(());
        }
        Command::PreviewInitialRewardCutover {
            bundle,
            destination,
            decision_id,
        } => {
            let preview = operator
                .preview_initial_reward_cutover(&bundle, &destination, decision_id)
                .await?;
            println!(
                "{}",
                serde_json::json!({"operation":"preview_initial_reward_cutover",
                "id":preview.id,"accepted":true,"cutover_applied":false,"activation_authorized":false})
            );
            return Ok(());
        }
        Command::CompareInitialRewardCutover {
            bundle,
            preview,
            preview_sha256,
            candidate_copy,
        } => {
            let (manifest, summary) = operator
                .compare_initial_reward_cutover(&bundle, &preview, &preview_sha256, &candidate_copy)
                .await?;
            println!(
                "{}",
                serde_json::json!({"operation":"initial_reward_cutover_compare",
                "id":manifest.id,"accepted":true,"resume_authorized":false,
                "decision_sha256":summary.decision_sha256})
            );
            return Ok(());
        }
        Command::SnapshotBeforeContact {
            destination,
            writers_stopped: true,
        } => (
            "contact_upgrade_snapshot",
            operator.snapshot_before_contact(&destination).await?,
        ),
        Command::VerifyContactUpgradeSource { bundle } => (
            "contact_upgrade_source_verify",
            operator.verify_contact_upgrade_source(&bundle).await?,
        ),
        Command::CompareContactUpgrade {
            bundle,
            candidate_copy,
        } => (
            "contact_upgrade_compare",
            operator
                .compare_contact_upgrade(&bundle, &candidate_copy)
                .await?,
        ),
        Command::SnapshotBeforeFeederCap {
            destination,
            writers_stopped: true,
        } => (
            "feeder_cap_upgrade_snapshot",
            operator.snapshot_before_feeder_cap(&destination).await?,
        ),
        Command::VerifyFeederCapUpgradeSource { bundle } => (
            "feeder_cap_upgrade_source_verify",
            operator.verify_feeder_cap_upgrade_source(&bundle).await?,
        ),
        Command::CompareFeederCapUpgrade {
            bundle,
            candidate_copy,
        } => (
            "feeder_cap_upgrade_compare",
            operator
                .compare_feeder_cap_upgrade(&bundle, &candidate_copy)
                .await?,
        ),
        Command::SnapshotBeforeInclusiveFees {
            destination,
            writers_stopped: true,
        } => (
            "inclusive_fee_upgrade_snapshot",
            operator
                .snapshot_before_inclusive_fees(&destination)
                .await?,
        ),
        Command::VerifyInclusiveFeeUpgradeSource { bundle } => (
            "inclusive_fee_upgrade_source_verify",
            operator
                .verify_inclusive_fee_upgrade_source(&bundle)
                .await?,
        ),
        Command::CompareInclusiveFeeUpgrade {
            bundle,
            candidate_copy,
        } => (
            "inclusive_fee_upgrade_compare",
            operator
                .compare_inclusive_fee_upgrade(&bundle, &candidate_copy)
                .await?,
        ),
        Command::StageRestore {
            bundle,
            destination,
            writers_stopped: true,
        } => (
            "stage_restore",
            operator.stage_restore(&bundle, &destination).await?,
        ),
        _ => anyhow::bail!("explicit stopped-writer attestation is required"),
    };
    // Detailed scope, private paths and counts belong only in the private manifest.
    println!(
        "{}",
        serde_json::json!({"operation":operation,"id":manifest.id,"accepted":true,"resume_authorized":false})
    );
    Ok(())
}
#[cfg(not(unix))]
fn main() -> Result<()> {
    anyhow::bail!("state maintenance requires Unix protected file authority")
}
