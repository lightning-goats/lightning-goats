//! NUT-09 recovery scoped to one write-ahead receive.
use super::*;
use crate::ownership::SwapDerivation;
use cdk::{
    dhke::construct_proofs,
    nuts::{CurrencyUnit, PreMintSecrets, RestoreRequest, RestoreResponse, State, SwapRequest},
    wallet::types::{ProofInfo, TransactionDirection, TransactionId, TransactionStatus},
};
use std::collections::{BTreeMap, BTreeSet};

impl<T: Transport + 'static> Coordinator<'_, T> {
    pub(crate) async fn reconcile_original_outputs(&self, id: Uuid) -> Result<Progress> {
        let _lease = self.gate.arm_acquisition_recovery(id)?;
        if let Some(progress) = self.ready(id)? {
            return Ok(progress);
        }
        let (derivation, request, fee) = {
            let journal = self
                .journal
                .lock()
                .map_err(|_| anyhow::anyhow!("wallet journal unavailable"))?;
            if journal.state(id)? != "acquiring" {
                return Ok(Progress::Pending(id));
            }
            let Ok(derivation) = journal.derivation(id) else {
                return Ok(Progress::Pending(id));
            };
            let original = journal
                .prepared_request(id)?
                .ok_or_else(|| anyhow::anyhow!("original swap missing"))?;
            let request: SwapRequest = serde_json::from_slice(&original)
                .map_err(|_| anyhow::anyhow!("invalid original swap"))?;
            ensure!(
                derivation.mint_url == self.mint_url && request.outputs() == &derivation.outputs,
                "original acquisition binding changed"
            );
            let fee = journal.verify_swap_inputs(id, &request)?;
            (derivation, request, fee)
        };
        let ys = derivation.output_ys(self.seed)?;
        let mut owned = self.database.get_proofs_by_ys(ys.clone()).await?;
        let operation = derivation.cdk_operation;
        let saga = self.database.get_saga(&operation).await?;
        let transaction = self
            .database
            .get_transaction(TransactionId::from_saga_id(operation))
            .await?;
        if owned.len() == ys.len()
            && saga.is_none()
            && transaction
                .as_ref()
                .is_none_or(|entry| entry.status == TransactionStatus::Completed)
        {
            return self.reconcile_owned(id).await;
        }
        if let Some(saga) = &saga {
            ensure!(
                SwapDerivation::capture(saga, &request)? == derivation,
                "receive saga differs from frozen original"
            );
        }
        let input_proofs = request
            .inputs()
            .iter()
            .map(|p| Ok((p.y()?, p)))
            .collect::<Result<BTreeMap<_, _>>>()?;
        let inputs: Vec<_> = input_proofs.keys().copied().collect();
        let Some(mut transaction) = transaction else {
            return Ok(Progress::Pending(id));
        };
        ensure!(
            transaction.saga_id == Some(operation)
                && transaction.mint_url == self.mint_url
                && transaction.unit == CurrencyUnit::Sat
                && transaction.direction == TransactionDirection::Incoming
                && transaction.amount == request.output_amount()?
                && u64::from(transaction.fee) == fee
                && transaction.ys.len() == inputs.len()
                && transaction.ys.iter().copied().collect::<BTreeSet<_>>()
                    == inputs.iter().copied().collect(),
            "receive transaction differs from frozen original"
        );
        let stored = self.database.get_proofs_by_ys(inputs.clone()).await?;
        let reserved = self.database.get_reserved_proofs(&operation).await?;
        ensure!(
            reserved.iter().all(|p| input_proofs.contains_key(&p.y))
                && stored
                    .iter()
                    .all(
                        |row| input_proofs.get(&row.y).is_some_and(|proof| row.mint_url
                            == self.mint_url
                            && row.unit == CurrencyUnit::Sat
                            && row.state == State::Pending
                            && row.used_by_operation == Some(operation)
                            && row.proof.secret == proof.secret
                            && row.proof.c == proof.c
                            && row.proof.amount == proof.amount
                            && row.proof.keyset_id == proof.keyset_id)
                    )
                && owned.iter().all(|p| p.used_by_operation.is_none()),
            "original receive proof ownership conflict"
        );
        let mint = self.wallet.mint_connector();
        let added = if owned.len() == ys.len() {
            Vec::new()
        } else {
            let Ok(reply) = mint
                .post_restore(RestoreRequest {
                    outputs: derivation.outputs.clone(),
                })
                .await
            else {
                return Ok(Progress::Pending(id));
            };
            if reply.outputs.len() != ys.len() || reply.signatures.len() != ys.len() {
                return Ok(Progress::Pending(id));
            }
            let restored = self.unblind_original_outputs(&derivation, reply).await?;
            let existing: BTreeSet<_> = owned.iter().map(|p| p.y).collect();
            restored
                .into_iter()
                .filter(|p| !existing.contains(&p.y))
                .collect::<Vec<_>>()
        };
        owned.extend(added.iter().cloned());
        let Ok(input_states) = mint
            .post_check_state(CheckStateRequest { ys: inputs.clone() })
            .await
        else {
            return Ok(Progress::Pending(id));
        };
        if input_states.states.len() != inputs.len()
            || input_states
                .states
                .iter()
                .any(|state| state.state != State::Spent)
            || input_states
                .states
                .into_iter()
                .map(|state| state.y)
                .collect::<BTreeSet<_>>()
                != inputs.iter().copied().collect()
        {
            return Ok(Progress::Pending(id));
        }
        let Ok(output_states) = mint.post_check_state(CheckStateRequest { ys }).await else {
            return Ok(Progress::Pending(id));
        };
        let verified = {
            let journal = self
                .journal
                .lock()
                .map_err(|_| anyhow::anyhow!("wallet journal unavailable"))?;
            verify_owned_outputs(
                id,
                &journal,
                &derivation,
                self.seed,
                &self.mint_id,
                &self.keys,
                &owned,
                &output_states,
            )?
        };
        ensure!(
            self.database
                .increment_keyset_counter(&derivation.keyset_id, 0)
                .await?
                >= derivation.counter_end,
            "original output counter range is not reserved"
        );
        // Persist proofs before completion. A restart validates the same output
        // Ys and resumes any remaining local steps before attributing custody.
        if !added.is_empty() || !stored.is_empty() {
            self.database.update_proofs(added, inputs).await?;
        }
        if transaction.status != TransactionStatus::Completed {
            transaction.status = TransactionStatus::Completed;
            self.database.add_transaction(transaction).await?;
        }
        if saga.is_some() {
            self.database.delete_saga(&operation).await?;
        }
        {
            let mut journal = self
                .journal
                .lock()
                .map_err(|_| anyhow::anyhow!("wallet journal unavailable"))?;
            verified.commit(&mut journal, None)?;
        }
        Ok(self.ready(id)?.unwrap_or(Progress::Acquired(id)))
    }

    async fn unblind_original_outputs(
        &self,
        derivation: &SwapDerivation,
        reply: RestoreResponse,
    ) -> Result<Vec<ProofInfo>> {
        let derived = PreMintSecrets::restore_batch(
            derivation.keyset_id,
            self.seed,
            derivation.counter_start,
            derivation.counter_end,
        )?;
        let mut secrets: BTreeMap<_, _> = derived
            .secrets
            .into_iter()
            .map(|p| (p.blinded_message.blinded_secret, p))
            .collect();
        let expected: BTreeMap<_, _> = derivation
            .outputs
            .iter()
            .map(|output| (output.blinded_secret, output))
            .collect();
        let keys = self
            .database
            .get_keys(&derivation.keyset_id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("original output keyset missing"))?;
        let mut ordered = Vec::with_capacity(reply.outputs.len());
        for (output, signature) in reply.outputs.iter().zip(&reply.signatures) {
            let original = expected
                .get(&output.blinded_secret)
                .ok_or_else(|| anyhow::anyhow!("unrequested restored output"))?;
            let premint = secrets
                .remove(&output.blinded_secret)
                .ok_or_else(|| anyhow::anyhow!("duplicate restored output"))?;
            let pinned = self.keys.key(
                &self.mint_id,
                original.keyset_id,
                u64::from(original.amount),
            )?;
            ensure!(
                output.amount == original.amount
                    && output.keyset_id == original.keyset_id
                    && signature.amount == original.amount
                    && signature.keyset_id == original.keyset_id
                    && keys.amount_key(original.amount) == Some(pinned),
                "restored output amount, keyset or mint key changed"
            );
            signature
                .verify_dleq(pinned, original.blinded_secret)
                .map_err(|_| anyhow::anyhow!("invalid restored output DLEQ"))?;
            ordered.push(premint);
        }
        ensure!(secrets.is_empty(), "restored output set incomplete");
        let proofs = construct_proofs(
            reply.signatures,
            ordered.iter().map(|p| p.r.clone()).collect(),
            ordered.iter().map(|p| p.secret.clone()).collect(),
            &keys,
        )?;
        proofs
            .into_iter()
            .zip(ordered)
            .map(|(proof, premint)| {
                let info = ProofInfo::new(
                    proof,
                    self.mint_url.clone(),
                    State::Unspent,
                    CurrencyUnit::Sat,
                )?;
                Ok(match premint.derivation_index {
                    Some(index) => info.with_derivation_index(index),
                    None => info,
                })
            })
            .collect()
    }
}
