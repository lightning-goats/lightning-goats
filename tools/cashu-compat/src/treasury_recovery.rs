//! Serialized recovery only: no mint quote creation or melt submission methods.
use crate::{
    intake::PinnedKeys,
    journal::Journal,
    melt_reconciliation::{normalize_restored_change, reconstruct_change, verify_owned_change},
    treasury::TreasuryBinding,
    writer_lock::{Authority, Operation, WriterLock},
};
use anyhow::{Result, ensure};
use cdk::{
    cdk_database::{Error as DatabaseError, WalletDatabase},
    nuts::{CheckStateRequest, CheckStateResponse, State},
};
use cdk_http_client::Transport;
use lightning_goats_cashu_receipt::treasury::MeltData;
use std::sync::{Arc, Mutex};
use uuid::Uuid;
mod cleanup;

pub struct Recovery<'a, T: Transport> {
    pub transport: &'a T,
    pub journal: Arc<Mutex<Journal>>,
    pub database: Arc<dyn WalletDatabase<DatabaseError> + Send + Sync>,
    pub writer: Arc<WriterLock>,
    pub binding: &'a TreasuryBinding,
    pub keys: &'a PinnedKeys,
    pub seed: &'a [u8; 64],
}
struct Lease {
    active: crate::writer_lock::ActiveOperation,
    id: Uuid,
}
impl Drop for Lease {
    fn drop(&mut self) {
        if let Ok(mut active) = self.active.lock()
            && active
                .as_ref()
                .is_some_and(|operation| operation.id == self.id)
        {
            *active = None;
        }
    }
}
impl<T: Transport> Recovery<'_, T> {
    /// Recover the original retained quote into CDK without mint contact.
    /// Never overwrite an advanced or conflicting CDK operation.
    pub async fn recover_quote(
        &self,
        id: Uuid,
        mint: &crate::mint_policy::MintPolicy,
        now: u64,
    ) -> Result<Option<cdk::wallet::MeltQuote>> {
        let active = self.writer.coordinator();
        {
            let mut current = active
                .lock()
                .map_err(|_| anyhow::anyhow!("wallet coordinator unavailable"))?;
            ensure!(current.is_none(), "another wallet operation is active");
            *current = Some(Operation {
                id,
                authority: Authority::TreasuryRecovery,
            });
        }
        let _lease = Lease { active, id };
        let quote = self
            .journal
            .lock()
            .map_err(|_| anyhow::anyhow!("wallet journal unavailable"))?
            .adopt_retained_treasury_quote(self.binding, mint, id, now)?;
        let Some(quote) = quote else {
            return Ok(None);
        };
        if let Some(mut existing) = self.database.get_melt_quote(&quote.id).await? {
            // CDK's optimistic row version is storage metadata, not quote terms.
            existing.version = quote.version;
            ensure!(
                serde_json::to_vec(&existing)? == serde_json::to_vec(&quote)?,
                "CDK quote conflicts with retained original"
            );
        } else {
            self.database.add_melt_quote(quote.clone()).await?;
        }
        Ok(Some(quote))
    }

    /// None preserves an unresolved original operation. Errors also retain its
    /// claim/holds. Exact completed reports replay without mint contact.
    pub async fn reconcile(&self, id: Uuid) -> Result<Option<MeltData>> {
        let active = self.writer.coordinator();
        {
            let mut current = active
                .lock()
                .map_err(|_| anyhow::anyhow!("wallet coordinator unavailable"))?;
            ensure!(current.is_none(), "another wallet operation is active");
            *current = Some(Operation {
                id,
                authority: Authority::TreasuryRecovery,
            });
        }
        let _lease = Lease { active, id };
        let (context, retained) = {
            let journal = self
                .journal
                .lock()
                .map_err(|_| anyhow::anyhow!("wallet journal unavailable"))?;
            if let Some(report) = journal.completed_treasury_report(self.binding, id)? {
                return Ok(Some(report));
            }
            (
                journal.treasury_recovery_context(self.binding, id)?,
                journal.retained_treasury_restore(self.binding, id)?,
            )
        };
        let Some(mut paid) = context.poll_paid(self.transport).await? else {
            return Ok(None);
        };
        let signatures = if let Some(signatures) = &paid.change {
            signatures.clone()
        } else if context.derivation.maximum_change_sats == 0 {
            vec![]
        } else {
            let restored = if let Some((original_paid, response)) = retained {
                paid = original_paid;
                response
            } else {
                let response = context
                    .restore_missing_change(self.transport, &paid)
                    .await?;
                self.journal
                    .lock()
                    .map_err(|_| anyhow::anyhow!("wallet journal unavailable"))?
                    .record_treasury_restore(self.binding, id, &paid, &response)?;
                response
            };
            normalize_restored_change(&context.derivation, &restored)?
        };
        let mut candidates = reconstruct_change(
            &context.derivation,
            self.seed,
            &self.binding.mint_id,
            self.keys,
            &signatures,
        )?;
        let ys = candidates.iter().map(|info| info.y).collect::<Vec<_>>();
        let fresh: CheckStateResponse = if ys.is_empty() {
            CheckStateResponse { states: vec![] }
        } else {
            self.transport
                .http_post(
                    context.derivation.mint_url.join("v1/checkstate")?,
                    None,
                    &CheckStateRequest { ys: ys.clone() },
                )
                .await
                .map_err(|_| anyhow::anyhow!("restored change state query failed"))?
        };
        verify_owned_change(
            &context.derivation,
            self.seed,
            &self.binding.mint_id,
            self.keys,
            &signatures,
            &candidates,
            &fresh,
        )?;
        let existing = if ys.is_empty() {
            vec![]
        } else {
            self.database.get_proofs_by_ys(ys).await?
        };
        for candidate in &mut candidates {
            let matches = existing
                .iter()
                .filter(|info| info.y == candidate.y)
                .collect::<Vec<_>>();
            ensure!(matches.len() <= 1, "ambiguous stored change");
            if let Some(previous) = matches.first() {
                ensure!(
                    previous.mint_url == candidate.mint_url
                        && previous.unit == candidate.unit
                        && previous.state == State::Unspent
                        && previous.proof == candidate.proof,
                    "existing change proof conflicts or is no longer available"
                );
                *candidate = (*previous).clone();
            }
        }
        // A crash after this write leaves the original claim unresolved. The
        // next recovery rechecks the same proofs and cannot submit another melt.
        if !candidates.is_empty() {
            self.database
                .update_proofs(candidates.clone(), vec![])
                .await?;
        }
        let report = self
            .journal
            .lock()
            .map_err(|_| anyhow::anyhow!("wallet journal unavailable"))?
            .reconcile_treasury_melt(
                self.binding,
                id,
                &paid,
                self.seed,
                self.keys,
                &candidates,
                &fresh,
            )?;
        Ok(Some(report))
    }
}
