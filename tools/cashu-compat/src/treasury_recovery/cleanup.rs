//! Retryable original-operation cleanup after a separately committed report.
use super::*;
use cdk::{
    nuts::{CurrencyUnit, MeltQuoteState},
    wallet::types::{MeltSagaState, WalletSagaState},
};
use std::collections::BTreeSet;

impl<T: Transport> Recovery<'_, T> {
    pub async fn cleanup(&self, id: Uuid) -> Result<bool> {
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
        let context = self
            .journal
            .lock()
            .map_err(|_| anyhow::anyhow!("wallet journal unavailable"))?
            .treasury_cleanup_context(self.binding, id)?;
        let Some(context) = context else {
            return Ok(true);
        };
        let original = &context.recovery;
        let operation = original.derivation.cdk_operation;
        let saga = self.database.get_saga(&operation).await?;
        if let Some(saga) = &saga {
            ensure!(
                matches!(
                    saga.state,
                    WalletSagaState::Melt(
                        MeltSagaState::MeltRequested | MeltSagaState::PaymentPending
                    )
                ),
                "cleanup saga is outside original submitted melt states"
            );
            let mut normalized = saga.clone();
            normalized.state = WalletSagaState::Melt(MeltSagaState::MeltRequested);
            let input = context
                .request
                .inputs()
                .iter()
                .try_fold(0u64, |sum, p| sum.checked_add(u64::from(p.amount)))
                .ok_or_else(|| anyhow::anyhow!("cleanup input overflow"))?;
            let fee = input
                .checked_sub(u64::from(original.quote.amount))
                .and_then(|n| n.checked_sub(original.derivation.maximum_change_sats))
                .ok_or_else(|| anyhow::anyhow!("cleanup input fee mismatch"))?;
            ensure!(
                crate::melt_derivation::MeltDerivation::capture(
                    &normalized,
                    &context.request,
                    fee,
                    self.seed
                )? == original.derivation,
                "cleanup saga differs from original submitted request"
            );
        }
        let ys = context
            .request
            .inputs()
            .iter()
            .map(|p| Ok(p.y()?))
            .collect::<Result<Vec<_>>>()?;
        let expected = ys.iter().copied().collect::<BTreeSet<_>>();
        ensure!(
            expected.len() == ys.len(),
            "duplicate original cleanup input"
        );
        let stored = self.database.get_proofs_by_ys(ys.clone()).await?;
        let reserved = self.database.get_reserved_proofs(&operation).await?;
        ensure!(
            reserved.iter().all(|p| expected.contains(&p.y)),
            "cleanup operation owns unrelated proofs"
        );
        let mut seen = BTreeSet::new();
        for info in &stored {
            let proof = context
                .request
                .inputs()
                .iter()
                .find(|p| p.y().ok() == Some(info.y))
                .ok_or_else(|| anyhow::anyhow!("cleanup returned an unrelated proof"))?;
            ensure!(
                seen.insert(info.y)
                    && info.mint_url == original.derivation.mint_url
                    && info.unit == CurrencyUnit::Sat
                    && info.proof.y()? == info.y
                    && info.proof.secret == proof.secret
                    && info.proof.c == proof.c
                    && info.proof.amount == proof.amount
                    && info.proof.keyset_id == proof.keyset_id
                    && info
                        .used_by_operation
                        .is_none_or(|owner| owner == operation),
                "cleanup proof identity or ownership conflict"
            );
        }
        let quote = self.database.get_melt_quote(&original.quote.id).await?;
        if let Some(quote) = &quote {
            ensure!(
                quote.id == original.quote.id
                    && quote.request == original.quote.request
                    && quote.mint_url == original.quote.mint_url
                    && quote.unit == CurrencyUnit::Sat
                    && quote.payment_method == original.quote.payment_method
                    && quote.amount == original.quote.amount
                    && quote.fee_reserve == original.quote.fee_reserve
                    && quote.expiry == original.quote.expiry
                    && quote
                        .used_by_operation
                        .as_deref()
                        .is_none_or(|owner| owner == operation.to_string())
                    && quote
                        .payment_proof
                        .as_ref()
                        .is_none_or(|proof| Some(proof) == context.paid.payment_preimage.as_ref()),
                "cleanup quote differs from original paid operation"
            );
        }
        let fresh: CheckStateResponse = self
            .transport
            .http_post(
                original.derivation.mint_url.join("v1/checkstate")?,
                None,
                &CheckStateRequest { ys: ys.clone() },
            )
            .await
            .map_err(|_| anyhow::anyhow!("cleanup spent-state read unavailable"))?;
        let mut states = BTreeSet::new();
        ensure!(
            fresh.states.len() == ys.len()
                && fresh.states.iter().all(|s| s.state == State::Spent
                    && expected.contains(&s.y)
                    && states.insert(s.y)),
            "cleanup inputs are not definitively spent"
        );
        // All validations precede mutation. These local steps can be interrupted;
        // retry rechecks the same original operation and paid report.
        if !stored.is_empty() {
            self.database.update_proofs(vec![], ys).await?;
        }
        let mut paid_quote = quote.unwrap_or_else(|| original.quote.clone());
        if paid_quote.state != MeltQuoteState::Paid
            || paid_quote.used_by_operation.is_some()
            || paid_quote.payment_proof != context.paid.payment_preimage
        {
            paid_quote.state = MeltQuoteState::Paid;
            paid_quote.payment_proof = context.paid.payment_preimage.clone();
            paid_quote.used_by_operation = None;
            self.database.add_melt_quote(paid_quote).await?;
        }
        if saga.is_some() {
            self.database.delete_saga(&operation).await?;
        }
        self.journal
            .lock()
            .map_err(|_| anyhow::anyhow!("wallet journal unavailable"))?
            .record_treasury_cleanup(self.binding, id, operation, context.report_digest)?;
        Ok(true)
    }
}
