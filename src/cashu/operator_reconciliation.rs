//! Recover the two independently authenticated halves of one original settlement.
use super::{
    financial_release::require_stopped_core,
    operator_settlement::{load_approval, load_config},
    receipt_authority::ReceiptPolicy,
    treasury::PeerMelt,
    treasury_client::Client,
    treasury_worker::Wallet,
};
use crate::{
    ledger::{LedgerStore, TreasurySettlementState},
    strike::{ReconciledStrikeReceive, StrikeClient},
};
use anyhow::{Context, Result, ensure};
use lightning_goats_cashu_receipt::{
    treasury::{Destination, Issued, hex_digest},
    treasury_rpc::{Command, FEE_TIME_VERSION, Request},
};
use serde::{Deserialize, Serialize};
use std::{future::Future, path::Path, sync::Arc, time::Duration};
use uuid::Uuid;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Approval {
    version: u8,
    purpose: String,
    core_epoch: Uuid,
    wallet_instance: Uuid,
    wallet_epoch: Uuid,
    intent_id: Uuid,
    intent_digest: String,
    receive_request_id: Uuid,
    receive_id: Uuid,
    config_sha256: String,
    approved_at: i64,
    expires_at: i64,
}

#[derive(Serialize)]
pub struct Outcome {
    intent_id: Uuid,
    receive_request_id: Uuid,
    receive_id: Uuid,
    deposit_sats: u64,
    strike_verified_this_run: bool,
    wallet_report_observed: bool,
    wallet_status_unavailable: bool,
    apply_requested: bool,
    #[serde(flatten)]
    state: TreasurySettlementState,
}

pub(crate) struct Prepared {
    issued: Issued,
    original_state: TreasurySettlementState,
    receive: Option<ReconciledStrikeReceive>,
    report: Option<PeerMelt>,
    wallet_status_unavailable: bool,
}

impl Approval {
    fn require_current(&self) -> Result<()> {
        let now = crate::gateway::now_epoch()?;
        ensure!(
            self.version == 1
                && self.purpose == "reconcile_original_cashu_settlement"
                && [
                    self.core_epoch,
                    self.wallet_instance,
                    self.wallet_epoch,
                    self.intent_id,
                    self.receive_request_id,
                    self.receive_id
                ]
                .into_iter()
                .all(|id| !id.is_nil())
                && self.approved_at >= 0
                && self
                    .expires_at
                    .checked_sub(self.approved_at)
                    .is_some_and(|n| (1..=86400).contains(&n))
                && (self.approved_at..=self.expires_at).contains(&now),
            "original settlement approval is invalid or expired"
        );
        crate::nostr_event::validate_hex(&self.config_sha256, 32)?;
        crate::nostr_event::validate_hex(&self.intent_digest, 32)?;
        Ok(())
    }

    pub(crate) async fn prepare(
        &self,
        ledger: &LedgerStore,
        registration: &ReceiptPolicy,
        destination: &Destination,
        strike: impl Future<Output = Result<StrikeClient>>,
        wallet: &impl Wallet,
    ) -> Result<Prepared> {
        self.require_current()?;
        ensure!(
            self.core_epoch == registration.core_epoch
                && self.wallet_instance == registration.wallet_instance
                && self.wallet_epoch == registration.wallet_epoch,
            "original settlement approval differs from protected registration"
        );
        let issued = ledger
            .cashu_treasury_invoice(self.intent_id)
            .await?
            .context("original settlement requires a bound invoice")?;
        let intent = &issued.intent;
        ensure!(
            self.core_epoch == intent.core_epoch
                && self.wallet_instance == intent.wallet_instance
                && self.wallet_epoch == intent.wallet_epoch
                && self.intent_digest == hex_digest(intent.digest()?)
                && self.receive_request_id == issued.receive_request_id
                && *destination == intent.destination,
            "original settlement approval differs from the frozen intent, invoice or destination"
        );
        let original_state = ledger
            .treasury_settlement_state(&issued, self.receive_id)
            .await?;
        let receive = if original_state.strike_deposit_retained {
            None
        } else {
            let stored = ledger
                .strike_receive_request(issued.receive_request_id)
                .await?
                .context("original settlement receive request missing")?;
            Some(
                strike
                    .await?
                    .verify_cashu_treasury_receive(intent, &stored, self.receive_id)
                    .await?,
            )
        };
        let (report, wallet_status_unavailable) = if original_state.wallet_report_retained {
            (None, false)
        } else {
            let request = Request {
                version: FEE_TIME_VERSION,
                request_id: Uuid::new_v4(),
                command: Command::Status(Box::new(intent.clone())),
            };
            match wallet.exchange(&request).await {
                Ok(response) => {
                    response.reply.validate_for(&request)?;
                    ensure!(
                        response.reply.report.is_some() == response.report.is_some(),
                        "original wallet report was not authenticated"
                    );
                    if let Some(report) = &response.report {
                        ensure!(
                            response.reply.report.as_ref()
                                == Some(report.validate(registration, intent)?),
                            "wallet response differs from its authenticated report"
                        );
                    }
                    (response.report, false)
                }
                Err(_) => (None, true),
            }
        };
        Ok(Prepared {
            issued,
            original_state,
            receive,
            report,
            wallet_status_unavailable,
        })
    }

    pub(crate) async fn finish(
        &self,
        ledger: &LedgerStore,
        registration: &ReceiptPolicy,
        prepared: Prepared,
        apply: bool,
    ) -> Result<Outcome> {
        self.require_current()?;
        let state = if apply {
            // Both inputs are independently authoritative and retained for exact
            // retry. The existing ledger transaction completes only when both agree.
            if let Some(receive) = &prepared.receive {
                ledger
                    .record_cashu_treasury_deposit(&prepared.issued.intent, receive)
                    .await?;
            }
            if let Some(report) = &prepared.report {
                ledger
                    .record_cashu_melt_result(registration, report)
                    .await?;
            }
            ledger
                .treasury_settlement_state(&prepared.issued, self.receive_id)
                .await?
        } else {
            prepared.original_state
        };
        Ok(Outcome {
            intent_id: self.intent_id,
            receive_request_id: self.receive_request_id,
            receive_id: self.receive_id,
            deposit_sats: prepared.issued.intent.deposit_sats,
            strike_verified_this_run: prepared.receive.is_some(),
            wallet_report_observed: prepared.report.is_some(),
            wallet_status_unavailable: prepared.wallet_status_unavailable,
            apply_requested: apply,
            state,
        })
    }
}

pub async fn run(config_path: &Path, approval_path: &Path, apply: bool) -> Result<Outcome> {
    let approval: Approval = load_approval(approval_path)?;
    approval.require_current()?;
    let config = load_config(config_path, &approval.config_sha256)?;
    let registration = Arc::new(
        config
            .cashu_receipts
            .as_ref()
            .context("protected wallet registration is absent")?
            .policy()?,
    );
    let uid = crate::private_ipc::uid()?;
    ensure!(
        uid > 0 && uid != registration.wallet_uid,
        "core recovery requires the separate core runtime UID"
    );
    let treasury = config
        .cashu_treasury
        .as_ref()
        .context("protected treasury policy is absent")?;
    let client = Client::new(
        treasury.socket.clone(),
        registration.clone(),
        Duration::from_secs(treasury.deadline_seconds),
        1,
    )?;
    require_stopped_core().await?;
    let ledger = LedgerStore::connect_existing_recovery(&config.database.url).await?;
    let prepared = approval
        .prepare(
            &ledger,
            &registration,
            &treasury.policy.destination,
            StrikeClient::from_config(&config.strike),
            &client,
        )
        .await?;
    if apply {
        require_stopped_core().await?;
        crate::sqlite::check_recovery_marker(
            crate::sqlite::durable_options(&config.database.url)?.get_filename(),
        )?;
    }
    approval
        .finish(&ledger, &registration, prepared, apply)
        .await
}
