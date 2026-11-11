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
    /// Check only a pre-CyberHerd core upgrade policy; no app config or database.
    CheckCoreUpgrade {
        #[arg(long)]
        policy: PathBuf,
    },
    /// Copy a stopped/masked pre-CyberHerd core main/WAL without opening originals.
    BackupLegacyCore {
        #[arg(long)]
        policy: PathBuf,
        #[arg(long)]
        destination: PathBuf,
        #[arg(long, required = true)]
        writers_stopped: bool,
    },
    /// Verify an old core bundle and optionally compare a separate migrated copy.
    /// Neither operation migrates, restores or starts a service.
    VerifyCoreUpgrade {
        #[arg(long)]
        policy: PathBuf,
        #[arg(long)]
        bundle: PathBuf,
        #[arg(long)]
        candidate_copy: Option<PathBuf>,
    },
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
    /// Print a non-authorizing draft from an original finalized failed hold.
    FailedCostTemplate {
        #[arg(long)]
        reservation: Uuid,
        #[arg(long)]
        member: String,
    },
    /// Preview or apply a protected operator-reviewed failed-payment cost.
    /// This never resends a payment or claims member delivery.
    ReconcileFailedCost {
        #[arg(long)]
        approval: PathBuf,
        #[arg(long)]
        evidence: PathBuf,
        /// Commit only after the protected approval/evidence and fresh provider read agree.
        #[arg(long)]
        apply: bool,
    },
    /// Preview/apply an exact, audited credit retirement during stopped-writer maintenance.
    RetireFeedCredit {
        #[arg(long)]
        id: String,
        #[arg(long)]
        credit_entry_id: i64,
        #[arg(long)]
        expected_sats: u64,
        #[arg(long, required = true)]
        writers_stopped: bool,
        #[arg(long)]
        apply: bool,
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
    #[cfg(not(unix))]
    if matches!(
        args.command,
        Command::CheckCoreUpgrade { .. }
            | Command::BackupLegacyCore { .. }
            | Command::VerifyCoreUpgrade { .. }
    ) {
        bail!("legacy core upgrade operations require Unix protected-file authority");
    }
    // These commands must branch BEFORE AppConfig/LedgerStore::connect, which
    // would migrate the selected live database even for the ordinary status path.
    #[cfg(unix)]
    match &args.command {
        Command::CheckCoreUpgrade { policy } => {
            lightning_goats::state_maintenance::legacy::Policy::load_for_check(policy)?;
            println!("Legacy core policy is valid; no state, credentials or services accessed");
            return Ok(());
        }
        Command::BackupLegacyCore {
            policy,
            destination,
            writers_stopped,
        } => {
            if !*writers_stopped {
                bail!("explicit stopped-writer attestation required");
            }
            let operator = lightning_goats::state_maintenance::legacy::Operator::load(policy)?;
            let manifest = operator.snapshot(destination).await?;
            println!(
                "{}",
                serde_json::json!({"operation":"legacy_core_snapshot","id":manifest.id,"accepted":true,"resume_authorized":false})
            );
            return Ok(());
        }
        Command::VerifyCoreUpgrade {
            policy,
            bundle,
            candidate_copy,
        } => {
            let operator = lightning_goats::state_maintenance::legacy::Operator::load(policy)?;
            let manifest = if let Some(candidate) = candidate_copy {
                operator.compare_upgrade(bundle, candidate).await?
            } else {
                operator.verify(bundle).await?
            };
            println!(
                "{}",
                serde_json::json!({"operation":"verify_core_upgrade","id":manifest.id,"accepted":true,"candidate_compared":candidate_copy.is_some(),"resume_authorized":false})
            );
            return Ok(());
        }
        _ => {}
    }
    #[cfg(unix)]
    if matches!(
        args.command,
        Command::ReconcileFailedCost { .. } | Command::FailedCostTemplate { .. }
    ) {
        lightning_goats::payout::cost_recovery::require_protected_configuration(&args.config)?;
    }
    if let Command::RetireFeedCredit {
        writers_stopped, ..
    } = &args.command
    {
        #[cfg(unix)]
        {
            if !*writers_stopped || lightning_goats::core_bridge::unix::self_credentials()?.0 != 0 {
                bail!("credit retirement requires root and explicit stopped-writer maintenance");
            }
            let state = std::process::Command::new("systemctl")
                .args([
                    "show",
                    "lightning-goats-receive.service",
                    "-p",
                    "MainPID",
                    "--value",
                ])
                .output()?;
            if !state.status.success() || state.stdout != b"0\n" {
                bail!("core must be stopped before credit retirement");
            }
        }
        #[cfg(not(unix))]
        {
            bail!("credit retirement requires Unix protected-file authority");
        }
    }
    let config = AppConfig::load(&args.config)?;
    if let Command::RetireFeedCredit {
        id,
        credit_entry_id,
        expected_sats,
        apply,
        ..
    } = &args.command
    {
        #[cfg(unix)]
        {
            let result = LedgerStore::retire_credit_offline(
                &config.database.url,
                id,
                *credit_entry_id,
                *expected_sats,
                !apply,
            )
            .await?;
            println!("{}", serde_json::to_string(&result)?);
            return Ok(());
        }
    }
    let ledger = if matches!(
        args.command,
        Command::ReconcileFailedCost { .. } | Command::FailedCostTemplate { .. }
    ) {
        #[cfg(unix)]
        {
            LedgerStore::connect_existing_recovery(&config.database.url).await?
        }
        #[cfg(not(unix))]
        {
            bail!("failed-cost recovery requires Unix protected-file authority");
        }
    } else {
        LedgerStore::connect(&config.database.url).await?
    };

    match args.command {
        Command::CheckCoreUpgrade { .. }
        | Command::BackupLegacyCore { .. }
        | Command::VerifyCoreUpgrade { .. } => {
            bail!("legacy core upgrade operations require Unix protected-file authority");
        }
        Command::FailedCostTemplate {
            reservation,
            member,
        } => {
            println!(
                "{}",
                serde_json::to_string_pretty(
                    &ledger.failed_cost_draft(reservation, &member).await?
                )?
            );
        }
        Command::ReconcileFailedCost {
            approval,
            evidence,
            apply,
        } => {
            #[cfg(unix)]
            {
                let approved = lightning_goats::payout::cost_recovery::ApprovedDecision::load(
                    &approval, &evidence,
                )?;
                let decision = approved.decision();
                let bridge = config
                    .core_bridge
                    .as_ref()
                    .ok_or_else(|| anyhow::anyhow!("core payout account binding required"))?;
                if bridge.payout_account_id != Some(decision.account_id)
                    || bridge.reward_policy.as_ref().map(|p| p.owner.as_str())
                        != Some(decision.owner.as_str())
                {
                    bail!("protected config differs from failed-cost decision");
                }
                let existing = ledger.preview_failed_cost(decision).await?;
                let receipt = if apply && existing.is_none() {
                    if config.strike.api_url.trim_end_matches('/') != "https://api.strike.me" {
                        bail!("cost recovery requires the fixed production Strike API");
                    }
                    let client =
                        lightning_goats::strike::StrikeClient::from_config(&config.strike).await?;
                    let reader =
                        lightning_goats::payout::outcome::Reader::new(client, decision.account_id)?;
                    Some(ledger.reconcile_failed_cost(&approved, &reader).await?)
                } else {
                    existing
                };
                println!(
                    "{}",
                    serde_json::json!({"decision_id":decision.id,"reservation_id":decision.reservation_id,"member":decision.member,"gross_held_sats":decision.gross_sats,"reviewed_actual_cost_sats":decision.actual_cost_sats,"release_sats":decision.gross_sats-decision.actual_cost_sats,"committed":receipt.is_some(),"apply_requested":apply,"payment_sent":false,"member_paid":false})
                );
            }
            #[cfg(not(unix))]
            {
                let _ = (approval, evidence, apply);
                bail!("failed-cost recovery requires Unix");
            }
        }
        Command::RetireFeedCredit {
            id,
            credit_entry_id,
            expected_sats,
            writers_stopped: _,
            apply,
        } => {
            let result = ledger
                .retire_existing_feed_credit(&id, credit_entry_id, expected_sats, !apply)
                .await?;
            println!("{}", serde_json::to_string(&result)?);
        }
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
            if !dry_run {
                ledger
                    .configure_feeder_credit_cap(config.feeder.threshold_sats)
                    .await?;
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
