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
    /// Validate protected daemon settings offline, without opening its database or credentials.
    CheckConfig,
    /// Apply one authenticated late receive to its original historical contribution or contact.
    ReconcileHistoricalReceive {
        #[arg(long)]
        request_id: Uuid,
        #[arg(long)]
        receive_id: Uuid,
    },
    /// Read the gateway status of one original feeder UUID from a fenced Cashu checkpoint.
    ObserveRestoredCashuFeeder {
        #[arg(long)]
        selection: PathBuf,
        #[arg(long)]
        selection_sha256: String,
        #[arg(long)]
        gateway_origin: String,
        #[arg(long)]
        output: PathBuf,
    },
    /// Read one original Strike treasury receive selected from a fenced Cashu checkpoint.
    VerifyRestoredCashuStrike {
        #[arg(long)]
        selection: PathBuf,
        #[arg(long)]
        selection_sha256: String,
        #[arg(long)]
        credential: PathBuf,
        #[arg(long, default_value = "https://api.strike.me/")]
        strike_origin: String,
        #[arg(long)]
        output: PathBuf,
    },
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
    /// Preview/apply one reviewed pending financial plan release; never release custody.
    ReleaseFinancialPlan {
        #[arg(long)]
        approval: PathBuf,
        #[arg(long, required = true)]
        writers_stopped: bool,
        #[arg(long)]
        apply: bool,
    },
    /// Preview/apply a bounded core-owned Cashu settlement reservation; no payment is sent.
    PlanCashuSettlement {
        #[arg(long)]
        approval: PathBuf,
        #[arg(long, required = true)]
        writers_stopped: bool,
        #[arg(long)]
        apply: bool,
    },
    /// Verify and optionally record one original Cashu treasury settlement.
    ReconcileCashuSettlement {
        #[arg(long)]
        approval: PathBuf,
        #[arg(long, required = true)]
        writers_stopped: bool,
        #[arg(long)]
        apply: bool,
    },
    /// Inspect retained evidence and the linked feeding plan for an original Cashu settlement.
    InspectCashuSettlement {
        #[arg(long)]
        intent: Uuid,
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
    if matches!(args.command, Command::CheckConfig) {
        #[cfg(unix)]
        {
            lightning_goats::payout::cost_recovery::require_protected_configuration(&args.config)?;
            AppConfig::load(&args.config)
                .map_err(|_| anyhow::anyhow!("core configuration rejected"))?;
            println!("Core configuration valid; database and credentials not opened.");
            return Ok(());
        }
        #[cfg(not(unix))]
        bail!("offline core configuration validation requires Unix protected-file authority");
    }
    if let Command::ObserveRestoredCashuFeeder {
        selection,
        selection_sha256,
        gateway_origin,
        output,
    } = &args.command
    {
        #[cfg(unix)]
        {
            lightning_goats::state_maintenance::cashu_feeder_review::observe(
                selection,
                selection_sha256,
                gateway_origin,
                output,
            )
            .await
            .map_err(|_| anyhow::anyhow!("restored Cashu feeder observation refused"))?;
            println!(
                "{{\"result\":\"CASHU_RESTORE_FEEDER_REQUEST_OBSERVED\",\"resume_authorized\":false}}"
            );
            return Ok(());
        }
        #[cfg(not(unix))]
        {
            let _ = (selection, selection_sha256, gateway_origin, output);
            bail!("restored Cashu feeder observation requires Unix protected-file authority");
        }
    }
    if let Command::VerifyRestoredCashuStrike {
        selection,
        selection_sha256,
        credential,
        strike_origin,
        output,
    } = &args.command
    {
        #[cfg(unix)]
        {
            lightning_goats::state_maintenance::cashu_strike_review::verify(
                selection,
                selection_sha256,
                credential,
                strike_origin,
                output,
            )
            .await
            .map_err(|_| anyhow::anyhow!("restored Cashu Strike verification refused"))?;
            println!(
                "{{\"result\":\"CASHU_RESTORE_STRIKE_RECEIVE_VERIFIED\",\"resume_authorized\":false}}"
            );
            return Ok(());
        }
        #[cfg(not(unix))]
        {
            let _ = (
                selection,
                selection_sha256,
                credential,
                strike_origin,
                output,
            );
            bail!("restored Cashu Strike verification requires Unix protected-file authority");
        }
    }
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
        Command::ReconcileFailedCost { .. }
            | Command::FailedCostTemplate { .. }
            | Command::ReconcileHistoricalReceive { .. }
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
    if let Command::ReconcileCashuSettlement {
        approval,
        writers_stopped,
        apply,
    } = &args.command
    {
        #[cfg(unix)]
        {
            if !*writers_stopped {
                bail!("explicit stopped-writer attestation required");
            }
            let result = lightning_goats::cashu::operator_reconciliation::run(
                &args.config,
                approval,
                *apply,
            )
            .await?;
            println!("{}", serde_json::to_string(&result)?);
            return Ok(());
        }
        #[cfg(not(unix))]
        {
            let _ = (approval, writers_stopped, apply);
            bail!("original settlement recovery requires Unix protected-file authority");
        }
    }
    if let Command::InspectCashuSettlement { intent } = &args.command {
        #[cfg(unix)]
        {
            lightning_goats::payout::cost_recovery::require_protected_configuration(&args.config)?;
            let config = AppConfig::load(&args.config)?;
            lightning_goats::payout::cost_recovery::existing_private_database(
                &config.database.url,
            )?;
            let ledger = LedgerStore::connect_recovery_read_only(&config.database.url).await?;
            let status = ledger.inspect_cashu_settlement(*intent).await?;
            println!("{}", serde_json::to_string(&status)?);
            return Ok(());
        }
        #[cfg(not(unix))]
        {
            let _ = intent;
            bail!("Cashu settlement inspection requires Unix protected-file authority");
        }
    }
    if let Command::PlanCashuSettlement {
        approval,
        writers_stopped,
        apply,
    } = &args.command
    {
        #[cfg(unix)]
        {
            if !*writers_stopped {
                bail!("explicit stopped-writer attestation required");
            }
            let decision = lightning_goats::cashu::operator_settlement::Approval::load(approval)?;
            let config = decision.load_config(&args.config)?;
            let receipts = config
                .cashu_receipts
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("protected wallet registration is absent"))?;
            let registration = receipts.policy()?;
            decision.require_registration(&registration)?;
            let treasury = config
                .cashu_treasury
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("protected treasury policy is absent"))?;
            lightning_goats::cashu::financial_release::require_stopped_core().await?;
            let ledger = LedgerStore::connect_existing_recovery(&config.database.url).await?;
            decision.require_current()?;
            let planned = ledger
                .plan_operator_cashu_settlement(
                    &registration,
                    &treasury.policy,
                    &decision.request,
                    *apply,
                )
                .await?;
            println!(
                "{}",
                serde_json::json!({"request_id":decision.request.id,"mint_id":decision.request.mint_id,
                "maximum_input_sats":decision.request.maximum_input_sats,"apply_requested":apply,
                "would_plan":planned.is_some(),"intent_id":if *apply {planned.as_ref().map(|i|i.id)} else {None},
                "input_sats":planned.as_ref().map(|i|i.input_sats),"deposit_sats":planned.as_ref().map(|i|i.deposit_sats),
                "maximum_fee_sats":planned.as_ref().map(|i|i.maximum_fee_sats),
                "provider_request":false,"physical_actuation":false})
            );
            return Ok(());
        }
        #[cfg(not(unix))]
        {
            let _ = (approval, writers_stopped, apply);
            bail!("settlement planning requires Unix protected-file authority");
        }
    }
    if let Command::ReleaseFinancialPlan {
        approval,
        writers_stopped,
        apply,
    } = &args.command
    {
        #[cfg(unix)]
        {
            if !*writers_stopped {
                bail!("explicit stopped-writer attestation required");
            }
            lightning_goats::payout::cost_recovery::require_protected_configuration(&args.config)?;
            let decision = lightning_goats::cashu::financial_release::Approval::load(approval)?;
            let config = AppConfig::load(&args.config)?;
            if config.feeder.threshold_sats != decision.threshold_sats {
                bail!("release threshold differs from protected configuration");
            }
            lightning_goats::cashu::financial_release::require_stopped_core().await?;
            let ledger = LedgerStore::connect_existing_recovery(&config.database.url).await?;
            if ledger.overlay_stream_id().await? != decision.epoch {
                bail!("release approval belongs to another ledger epoch");
            }
            decision.require_current()?;
            let would_release = ledger
                .preview_financial_feed_plan_release(decision.plan_id, decision.threshold_sats)
                .await?;
            let changed = if *apply {
                decision.require_current()?;
                ledger
                    .release_financial_feed_plan(decision.plan_id, decision.threshold_sats)
                    .await?
            } else {
                false
            };
            println!(
                "{}",
                serde_json::json!({"plan_id":decision.plan_id,"would_release":would_release,
                "apply_requested":apply,"changed":changed,"custody_released":false,
                "provider_request":false,"physical_actuation":false})
            );
            return Ok(());
        }
        #[cfg(not(unix))]
        {
            let _ = (approval, writers_stopped, apply);
            bail!("financial release requires Unix protected-file authority");
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
        Command::ReconcileFailedCost { .. }
            | Command::FailedCostTemplate { .. }
            | Command::ReconcileHistoricalReceive { .. }
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
        Command::VerifyRestoredCashuStrike { .. } | Command::ObserveRestoredCashuFeeder { .. } => {
            unreachable!("handled before database initialization")
        }
        Command::ReleaseFinancialPlan { .. } => {
            bail!("financial release must use the protected offline path")
        }
        Command::CheckCoreUpgrade { .. }
        | Command::BackupLegacyCore { .. }
        | Command::VerifyCoreUpgrade { .. } => {
            bail!("legacy core upgrade operations require Unix protected-file authority");
        }
        Command::ReconcileHistoricalReceive {
            request_id,
            receive_id,
        } => {
            let original = ledger
                .strike_receive_request(request_id)
                .await?
                .ok_or_else(|| anyhow::anyhow!("historical invoice missing"))?;
            if original.address_user == "cashu_treasury" || original.credit_pool == "cashu_treasury"
            {
                bail!(
                    "treasury receipts require reconcile-cashu-settlement with the original approval and wallet evidence"
                );
            }
            let client = lightning_goats::strike::StrikeClient::from_config(
                &lightning_goats::config::StrikeConfig {
                    api_url: "https://api.strike.me/".to_owned(),
                },
            )
            .await?;
            let outcome = client
                .reconcile_original_receive(&ledger, request_id, receive_id)
                .await?;
            use lightning_goats::ledger::SettlementOutcome;
            println!(
                "{}",
                match outcome {
                    SettlementOutcome::Credited { .. } => "contribution recorded",
                    SettlementOutcome::ContactFeeAccepted => "contact fee accepted",
                    SettlementOutcome::Duplicate => "already recorded",
                    SettlementOutcome::TreasurySettlementAccepted =>
                        bail!("unexpected treasury receipt"),
                }
            );
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
                    let strike = config
                        .strike
                        .as_ref()
                        .ok_or_else(|| anyhow::anyhow!("Strike settlement backend missing"))?;
                    if strike.api_url.trim_end_matches('/') != "https://api.strike.me" {
                        bail!("cost recovery requires the fixed production Strike API");
                    }
                    let client = lightning_goats::strike::StrikeClient::from_config(strike).await?;
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
        Command::PlanCashuSettlement { .. }
        | Command::ReconcileCashuSettlement { .. }
        | Command::InspectCashuSettlement { .. }
        | Command::CheckConfig => {
            unreachable!("handled before database initialization")
        }
        Command::Status => {
            let credit = ledger.feed_credit_sats().await?;
            let threshold = config.feeder.threshold_sats;
            let unresolved = ledger.unresolved_feed_attempt().await?;
            println!("mode={}", config.service.mode.as_str());
            println!("payment_backend=cashu_mint");
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
