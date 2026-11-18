//! Explicit treasury spending workflow; callers must enable it separately from receiving.
//! Failures/cancellation retain the original claims for reconciliation.
use crate::{
    intake::PinnedKeys,
    journal::Journal,
    operation_gate::{OperationGate, TreasuryPolicy},
    treasury::ClaimedPreparation,
    treasury_scheduler::Execution,
    writer_lock::WriterLock,
};
use anyhow::{Context as _, Result, ensure};
use cdk::{
    cdk_database::{Error as DatabaseError, WalletDatabase},
    nuts::{CurrencyUnit, PaymentMethod, State},
    wallet::{CustomHttpClient, MeltConfirmOptions, MeltQuote, Wallet, WalletBuilder},
};
use cdk_http_client::Transport;
use std::sync::{Arc, Mutex};
use uuid::Uuid;
mod split;

enum PreparationMode<'a> {
    Fresh,
    Recover(Option<&'a Execution>),
}

pub struct Executor<T: Transport> {
    wallet: Wallet,
    gate: OperationGate<T>,
    journal: Arc<Mutex<Journal>>,
    database: Arc<dyn WalletDatabase<DatabaseError> + Send + Sync>,
    policy: Arc<TreasuryPolicy>,
    keys: Arc<PinnedKeys>,
    split_policy: Option<Arc<crate::journal::treasury_split::AdmissionPolicy>>,
}

impl<T: Transport + Clone + 'static> Executor<T> {
    /// Construct only under explicit settlement-spending configuration. This
    /// does not admit invoices, reserve sources or make a network request.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        transport: T,
        policy: Arc<TreasuryPolicy>,
        database: Arc<dyn WalletDatabase<DatabaseError> + Send + Sync>,
        journal: Arc<Mutex<Journal>>,
        writer: Arc<WriterLock>,
        keys: Arc<PinnedKeys>,
        message_limit: usize,
    ) -> Result<Self> {
        ensure!(
            policy.mint.id == policy.binding.mint_id && policy.mint.bolt11_exit_qualified,
            "treasury mint scope is not qualified"
        );
        let mint_url = policy.mint.advertised_url.parse()?;
        let gate = OperationGate::new(transport, journal.clone(), writer, message_limit)?
            .with_wallet_database(database.clone())
            .with_key_policy(policy.mint.id.clone(), keys.clone())
            .with_treasury_policy(policy.clone());
        let wallet = WalletBuilder::new()
            .mint_url(mint_url)
            .unit(CurrencyUnit::Sat)
            .localstore(database.clone())
            .seed(policy.seed)
            .client(CustomHttpClient::with_transport(
                policy.mint.advertised_url.parse()?,
                gate.clone(),
                None,
            ))
            .use_http_subscription()
            .build()?;
        Ok(Self {
            wallet,
            gate,
            journal,
            database,
            policy,
            keys,
            split_policy: None,
        })
    }

    /// Only fully reserved original invoices may reach the quote transport.
    /// Retry/restart uses Recovery::recover_quote, never this method again.
    pub async fn quote_reserved(&self, id: Uuid) -> Result<MeltQuote> {
        let _lease = self.gate.arm_treasury(id)?;
        let issued = self
            .journal
            .lock()
            .map_err(|_| anyhow::anyhow!("wallet journal unavailable"))?
            .treasury_instruction(&self.policy.binding, id)?;
        self.wallet
            .melt_quote(PaymentMethod::BOLT11, issued.invoice, None, None)
            .await
            .map_err(|_| anyhow::anyhow!("treasury quote requires original-operation recovery"))
    }

    /// Resume the original durable preparation, never create another CDK saga.
    /// Frozen/submitted requests and ambiguous CDK states remain recovery-only.
    pub async fn resume_prepared(&self, id: Uuid) -> Result<()> {
        let _lease = self.gate.arm_treasury(id)?;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs();
        let context = self
            .journal
            .lock()
            .map_err(|_| anyhow::anyhow!("wallet journal unavailable"))?
            .unsubmitted_treasury_preparation(&self.policy.binding, id, now)?;
        ensure!(
            context.quote.mint_url.as_ref() == Some(&self.policy.mint.advertised_url.parse()?),
            "prepared quote belongs to a different configured mint"
        );
        let saga = self
            .database
            .get_saga(&context.operation)
            .await?
            .ok_or_else(|| anyhow::anyhow!("original CDK preparation is missing"))?;
        validate_unsubmitted_saga(&saga, context.operation, &context.quote)?;
        let quote = self
            .database
            .get_melt_quote(&context.quote.id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("original CDK quote is missing"))?;
        let mut terms = quote.clone();
        ensure!(
            terms.used_by_operation.as_deref() == Some(context.operation.to_string().as_str()),
            "CDK quote is owned by another operation"
        );
        terms.used_by_operation = None;
        terms.version = context.quote.version;
        ensure!(
            serde_json::to_vec(&terms)? == serde_json::to_vec(&context.quote)?,
            "CDK quote differs from original preparation"
        );
        let stored = self.database.get_proofs_by_ys(context.ys.clone()).await?;
        validate_held_proofs(
            &stored,
            &context.ys,
            &quote,
            State::Reserved,
            Some(context.operation),
        )?;
        // CDK's pinned public resume entry point checks the same saga state and
        // optimistic version. All proofs go directly to melt; no swap is allowed.
        self.wallet
            .confirm_prepared_melt_with_options(
                context.operation,
                quote,
                stored.into_iter().map(|p| p.proof).collect(),
                vec![],
                context.input_fee.into(),
                context.input_fee.into(),
                Default::default(),
                MeltConfirmOptions::skip_swap(),
            )
            .await
            .map(|_| ())
            .map_err(|_| {
                anyhow::anyhow!("treasury resume requires original-operation reconciliation")
            })
    }

    /// Recover an already reserved preparation without submitting.
    pub async fn recover_preparation(&self, id: Uuid) -> Result<Uuid> {
        self.recover_preparation_with_policy(id, None).await
    }

    /// With execution enabled, an exact retained claim can complete its local
    /// quote and proof reservations. Melt submission is a separate stage.
    pub(crate) async fn recover_preparation_with_policy(
        &self,
        id: Uuid,
        execution: Option<&Execution>,
    ) -> Result<Uuid> {
        let _lease = self.gate.arm_treasury(id)?;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs();
        let claim = self
            .journal
            .lock()
            .map_err(|_| anyhow::anyhow!("wallet journal unavailable"))?
            .claimed_treasury_preparation(&self.policy.binding, id, now)?;
        self.finish_preparation(id, claim, PreparationMode::Recover(execution), now)
            .await
    }

    async fn finish_preparation(
        &self,
        id: Uuid,
        claim: ClaimedPreparation,
        mode: PreparationMode<'_>,
        now: u64,
    ) -> Result<Uuid> {
        let ClaimedPreparation {
            operation: claimed_operation,
            quote: original,
            ys,
        } = claim;
        ensure!(
            original.mint_url.as_ref() == Some(&self.policy.mint.advertised_url.parse()?),
            "claimed quote belongs to another configured mint"
        );
        let quote = self
            .database
            .get_melt_quote(&original.id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("original CDK quote missing"))?;
        let quote_owner = quote
            .used_by_operation
            .as_deref()
            .map(Uuid::parse_str)
            .transpose()?;
        let operation = claimed_operation
            .or(quote_owner)
            .context("original CDK operation not retained")?;
        let operation_id = operation.to_string();
        ensure!(
            !operation.is_nil()
                && quote
                    .used_by_operation
                    .as_deref()
                    .is_none_or(|owner| owner == operation_id),
            "CDK quote is owned by a different operation"
        );
        let mut terms = quote.clone();
        terms.used_by_operation = None;
        terms.version = original.version;
        ensure!(
            serde_json::to_vec(&terms)? == serde_json::to_vec(&original)?,
            "CDK quote differs from original claimed quote"
        );
        let saga = self.database.get_saga(&operation).await?;
        if let Some(saga) = &saga {
            validate_unsubmitted_saga(saga, operation, &original)?;
        }
        let stored = self.database.get_proofs_by_ys(ys.clone()).await?;
        let reserved = self.database.get_reserved_proofs(&operation).await?;
        let needs_reservation = saga.is_none() && reserved.is_empty();
        ensure!(
            quote_owner.is_some() || needs_reservation,
            "unowned CDK quote has evidence of prior preparation"
        );
        let reservation_policy = if needs_reservation {
            let execution = match mode {
                PreparationMode::Fresh => None,
                PreparationMode::Recover(execution) => Some(
                    execution
                        .filter(|policy| policy.execute)
                        .context("original CDK proof reservation requires treasury execution")?,
                ),
            };
            validate_held_proofs(&stored, &ys, &original, State::Unspent, None)?;
            execution
        } else {
            validate_held_proofs(&stored, &ys, &original, State::Reserved, Some(operation))?;
            validate_held_proofs(&reserved, &ys, &original, State::Reserved, Some(operation))?;
            None
        };
        if matches!(mode, PreparationMode::Recover(_)) {
            let quotes = self.database.get_melt_quotes().await?;
            ensure!(
                quotes
                    .iter()
                    .all(|q| q.used_by_operation.as_deref() != Some(&operation_id)
                        || q.id == original.id),
                "original operation owns an unrelated quote"
            );
        }
        // Use CDK's fee arithmetic on mint-scoped, already stored metadata.
        // Wallet::get_proofs_fee may refresh metadata over the network.
        let keysets = self
            .database
            .get_mint_keysets(
                original
                    .mint_url
                    .clone()
                    .ok_or_else(|| anyhow::anyhow!("missing original mint"))?,
            )
            .await?
            .ok_or_else(|| anyhow::anyhow!("retained mint fee metadata missing"))?;
        let mut counts = std::collections::HashMap::new();
        let mut fees = std::collections::HashMap::new();
        for proof in &stored {
            let keyset = proof.proof.keyset_id;
            let matches = keysets
                .iter()
                .filter(|k| k.id == keyset)
                .collect::<Vec<_>>();
            ensure!(
                matches.len() == 1 && matches[0].unit == CurrencyUnit::Sat,
                "retained keyset fee metadata missing or ambiguous"
            );
            *counts.entry(keyset).or_insert(0u64) += 1;
            fees.insert(keyset, matches[0].input_fee_ppk);
        }
        let input_fee = u64::from(cdk::fees::calculate_fee(&counts, &fees)?.total);
        let proofs = stored.into_iter().map(|p| p.proof).collect::<Vec<_>>();
        if saga.is_none() {
            {
                let mut journal = self
                    .journal
                    .lock()
                    .map_err(|_| anyhow::anyhow!("wallet journal unavailable"))?;
                journal.check_treasury_preparation_evidence(
                    &self.policy.binding,
                    id,
                    operation,
                    &quote,
                    input_fee,
                    &proofs,
                    now,
                    false,
                )?;
                if let Some(policy) = reservation_policy {
                    journal.authorize_treasury_execution(
                        &self.policy.binding,
                        id,
                        policy,
                        i64::try_from(now)?,
                    )?;
                }
            }
            if quote_owner.is_none() {
                self.database
                    .reserve_melt_quote(&original.id, &operation)
                    .await?;
            }
            if needs_reservation {
                self.database.reserve_proofs(ys.clone(), &operation).await?;
                let reserved = self.database.get_reserved_proofs(&operation).await?;
                validate_held_proofs(&reserved, &ys, &original, State::Reserved, Some(operation))?;
            }
            use cdk::wallet::types::{
                MeltOperationData, MeltSagaState, OperationData, WalletSaga, WalletSagaState,
            };
            let restored = WalletSaga::new(
                operation,
                WalletSagaState::Melt(MeltSagaState::ProofsReserved),
                original.amount,
                original
                    .mint_url
                    .clone()
                    .ok_or_else(|| anyhow::anyhow!("missing original mint"))?,
                CurrencyUnit::Sat,
                OperationData::Melt(MeltOperationData {
                    quote_id: original.id.clone(),
                    amount: original.amount,
                    fee_reserve: original.fee_reserve,
                    counter_start: None,
                    counter_end: None,
                    change_amount: None,
                    metadata: Default::default(),
                    final_proof_ys: None,
                    change_blinded_messages: None,
                }),
            );
            self.database.add_saga(restored).await?;
        }
        self.journal
            .lock()
            .map_err(|_| anyhow::anyhow!("wallet journal unavailable"))?
            .bind_treasury_preparation_evidence(
                &self.policy.binding,
                id,
                operation,
                &quote,
                input_fee,
                &proofs,
                now,
            )?;
        Ok(operation)
    }

    /// Persist preparation without submitting; resume uses this same identity.
    pub async fn prepare_reserved(&self, id: Uuid) -> Result<Uuid> {
        let _lease = self.gate.arm_treasury(id)?;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs();
        let claim = self
            .journal
            .lock()
            .map_err(|_| anyhow::anyhow!("wallet journal unavailable"))?
            .claim_treasury_preparation(&self.policy.binding, id, now)?;
        self.finish_preparation(id, claim, PreparationMode::Fresh, now)
            .await
    }

    /// Prepare the exact held proofs and submit once, without an implicit swap.
    /// Success is only transport completion, not verified treasury settlement.
    /// Every error/cancellation requires reconciliation of this same intent.
    pub async fn submit_reserved(&self, id: Uuid) -> Result<()> {
        self.prepare_reserved(id).await?;
        self.resume_prepared(id).await
    }
}

fn validate_unsubmitted_saga(
    saga: &cdk::wallet::types::WalletSaga,
    operation: Uuid,
    quote: &MeltQuote,
) -> Result<()> {
    use cdk::wallet::types::{MeltSagaState, OperationData, WalletSagaState};
    let OperationData::Melt(data) = &saga.data else {
        anyhow::bail!("original operation is not a melt");
    };
    ensure!(
        saga.id == operation
            && saga.state == WalletSagaState::Melt(MeltSagaState::ProofsReserved)
            && saga.unit == CurrencyUnit::Sat
            && Some(&saga.mint_url) == quote.mint_url.as_ref()
            && saga.amount == quote.amount
            && saga.quote_id.as_ref() == Some(&quote.id)
            && data.quote_id == quote.id
            && data.amount == quote.amount
            && data.fee_reserve == quote.fee_reserve
            && data.counter_start.is_none()
            && data.counter_end.is_none()
            && data.final_proof_ys.is_none()
            && data.change_blinded_messages.is_none()
            && data.change_amount.is_none(),
        "CDK preparation is advanced or conflicting"
    );
    Ok(())
}

/// Validate the complete database response before CDK can reserve or submit it.
/// A row's cached Y is insufficient: it must match the actual proof secret too.
fn validate_held_proofs(
    stored: &[cdk::wallet::types::ProofInfo],
    expected: &[cdk::nuts::PublicKey],
    quote: &MeltQuote,
    state: State,
    operation: Option<Uuid>,
) -> Result<()> {
    let expected_set: std::collections::BTreeSet<_> = expected.iter().copied().collect();
    let actual: std::collections::BTreeSet<_> = stored.iter().map(|p| p.y).collect();
    ensure!(
        !expected.is_empty()
            && expected_set.len() == expected.len()
            && actual.len() == stored.len()
            && actual == expected_set,
        "held treasury proofs are missing, duplicated or substituted"
    );
    for proof in stored {
        ensure!(
            proof.proof.y()? == proof.y
                && proof.state == state
                && proof.used_by_operation == operation
                && proof.unit == CurrencyUnit::Sat
                && Some(&proof.mint_url) == quote.mint_url.as_ref(),
            "held treasury proof identity, ownership or scope changed"
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use cdk::{
        nuts::{Id, Proof, SecretKey},
        secret::Secret,
        wallet::types::ProofInfo,
    };

    #[test]
    fn held_proofs_require_exact_unique_secrets_and_operation_ownership() -> Result<()> {
        let mint: cdk::mint_url::MintUrl = "https://mint.example/".parse()?;
        let key = SecretKey::from_slice(&[7; 32])?;
        let make = |secret: &str| -> Result<ProofInfo> {
            let secret = Secret::new(secret);
            let signature =
                cdk::dhke::sign_message(&key, &cdk::dhke::hash_to_curve(secret.as_bytes())?)?;
            let proof = Proof::new(
                8.into(),
                "00deadbeef123456".parse::<Id>()?,
                secret,
                signature,
            );
            Ok(ProofInfo::new(
                proof,
                mint.clone(),
                State::Unspent,
                CurrencyUnit::Sat,
            )?)
        };
        let a = make("held-a")?;
        let b = make("held-b")?;
        let mut quote: MeltQuote = serde_json::from_value(serde_json::json!({
            "id":"fixture", "mint_url":"https://mint.example/", "unit":"sat",
            "amount":7, "request":"fixture", "fee_reserve":1, "state":"UNPAID",
            "expiry":2000, "payment_method":"bolt11", "version":0
        }))?;
        let ys = [a.y, b.y];
        validate_held_proofs(&[b.clone(), a.clone()], &ys, &quote, State::Unspent, None)?;
        for rows in [
            vec![],
            vec![a.clone()],
            vec![a.clone(), a.clone()],
            vec![a.clone(), make("other")?],
        ] {
            assert!(validate_held_proofs(&rows, &ys, &quote, State::Unspent, None).is_err());
        }
        assert!(
            validate_held_proofs(
                &[a.clone(), b.clone()],
                &[a.y, a.y],
                &quote,
                State::Unspent,
                None
            )
            .is_err()
        );
        let operation = Uuid::from_u128(42);
        let mut reserved = [a.clone(), b.clone()];
        for proof in &mut reserved {
            proof.state = State::Reserved;
            proof.used_by_operation = Some(operation);
        }
        validate_held_proofs(&reserved, &ys, &quote, State::Reserved, Some(operation))?;
        assert!(validate_held_proofs(&reserved, &ys, &quote, State::Unspent, None).is_err());
        assert!(
            validate_held_proofs(
                &reserved,
                &ys,
                &quote,
                State::Reserved,
                Some(Uuid::from_u128(43))
            )
            .is_err()
        );
        let mut stale = a.clone();
        stale.used_by_operation = Some(operation);
        assert!(
            validate_held_proofs(&[stale, b.clone()], &ys, &quote, State::Unspent, None).is_err()
        );
        let mut forged = a.clone();
        forged.proof = b.proof.clone();
        assert!(
            validate_held_proofs(&[forged, b.clone()], &ys, &quote, State::Unspent, None).is_err()
        );
        quote.mint_url = None;
        assert!(validate_held_proofs(&[a, b], &ys, &quote, State::Unspent, None).is_err());
        Ok(())
    }
}
