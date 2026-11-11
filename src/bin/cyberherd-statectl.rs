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
    /// Compare a known schema-23 backup with an already migrated schema-24 copy.
    /// Never migrates, restores, clears markers or starts a service.
    CompareContactUpgrade {
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
