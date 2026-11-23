//! One original swap submission claim. A timeout never authorizes another POST.
use super::*;

impl Journal {
    pub(crate) fn completed_treasury_split(
        &self,
        binding: &TreasuryBinding,
        id: Uuid,
    ) -> Result<bool> {
        self.treasury_instruction(binding, id)?;
        let row: Option<(Vec<u8>,Vec<u8>)> = self.connection.query_row(
            "SELECT s.frozen_digest,q.frozen FROM treasury_split_results s JOIN treasury_split_requests q ON q.intent_id=s.intent_id WHERE s.intent_id=? AND length(q.frozen)<=2097152",
            [id.to_string()], |r| Ok((r.get(0)?,r.get(1)?))).optional()?;
        let Some((digest, bytes)) = row else {
            return Ok(false);
        };
        ensure!(
            digest == Sha256::digest(&bytes).to_vec(),
            "committed split binding changed"
        );
        self.treasury_split_derivation(binding, id)?;
        Ok(true)
    }

    /// Validate any retained CDK saga against the original submitted request.
    /// Missing saga is allowed: CDK may already have persisted the owned outputs.
    pub(crate) fn treasury_split_recovery_context(
        &self,
        binding: &TreasuryBinding,
        id: Uuid,
        saga: Option<&cdk::wallet::types::WalletSaga>,
    ) -> Result<(crate::ownership::SwapDerivation, Vec<cdk::nuts::PublicKey>)> {
        let derivation = self.treasury_split_derivation(binding, id)?;
        let bytes: Vec<u8> = self.connection.query_row(
            "SELECT frozen FROM treasury_split_requests WHERE intent_id=? AND length(frozen)<=2097152",
            [id.to_string()], |r| r.get(0))?;
        let frozen: Frozen = serde_json::from_slice(&bytes)?;
        let request: cdk::nuts::SwapRequest = serde_json::from_slice(&frozen.request)?;
        if let Some(saga) = saga {
            let fee = u64::from(request.input_amount()?)
                .checked_sub(u64::from(request.output_amount()?))
                .ok_or_else(|| anyhow::anyhow!("split input/output imbalance"))?;
            ensure!(
                crate::ownership::SwapDerivation::capture_split(saga, &request, fee)? == derivation,
                "CDK split recovery differs from original operation"
            );
        }
        let ys = request
            .inputs()
            .iter()
            .map(|p| Ok(p.y()?))
            .collect::<Result<Vec<_>>>()?;
        Ok((derivation, ys))
    }

    /// Claim CDK preparation before it can allocate counters or create a saga.
    /// Cancellation, even before the first CDK write, never grants a fresh attempt.
    pub(crate) fn claim_treasury_split_execution(
        &mut self,
        binding: &TreasuryBinding,
        id: Uuid,
        policy: &AdmissionPolicy,
        now: u64,
    ) -> Result<()> {
        policy.validate()?;
        ensure!(policy.enabled, "split execution disabled");
        let issued = self.treasury_instruction(binding, id)?;
        binding.validate_new_invoice(&issued, now)?;
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let (bytes, digest): (Vec<u8>, Vec<u8>) = tx.query_row(
            "SELECT r.reservation,a.reservation_digest FROM treasury_split_reservations r JOIN treasury_split_admissions a ON a.intent_id=r.intent_id JOIN treasury_instructions i ON i.id=r.intent_id WHERE r.intent_id=? AND i.state='bound' AND length(r.reservation)<=1048576 AND NOT EXISTS(SELECT 1 FROM treasury_split_requests q WHERE q.intent_id=r.intent_id) AND NOT EXISTS(SELECT 1 FROM treasury_preparation_claims p WHERE p.intent_id=r.intent_id)",
            [id.to_string()], |r| Ok((r.get(0)?,r.get(1)?)))?;
        ensure!(
            digest == Sha256::digest(&bytes).to_vec(),
            "split admission changed"
        );
        let reservation: Reservation = serde_json::from_slice(&bytes)?;
        reservation
            .plan
            .validate_for(&issued.intent, policy.limits)?;
        let quote: Vec<u8> = tx.query_row(
            "SELECT quote_json FROM treasury_quotes WHERE intent_id=? AND length(quote_json)<=65536",
            [id.to_string()], |r| r.get(0))?;
        let quote: cdk::wallet::MeltQuote = serde_json::from_slice(&quote)?;
        let fee = reservation
            .plan
            .fee_sats()
            .checked_add(u64::from(quote.fee_reserve))
            .ok_or_else(|| anyhow::anyhow!("split quote fee overflow"))?;
        ensure!(
            quote.expiry > now
                && quote.request == issued.invoice
                && quote.unit == cdk::nuts::CurrencyUnit::Sat
                && quote.payment_method == cdk::nuts::PaymentMethod::BOLT11
                && quote.state == cdk::nuts::MeltQuoteState::Unpaid
                && quote.used_by_operation.is_none()
                && u64::from(quote.amount) == issued.intent.deposit_sats
                && fee <= issued.intent.maximum_fee_sats
                && issued
                    .intent
                    .deposit_sats
                    .checked_add(fee)
                    .is_some_and(|n| n <= issued.intent.input_sats),
            "original split quote is missing, expired or unaffordable"
        );
        tx.execute("INSERT INTO treasury_split_execution_claims(intent_id,claimed_at,reservation_digest) VALUES(?,?,?)",
            params![id.to_string(),i64::try_from(now)?,digest])?;
        tx.commit()?;
        Ok(())
    }

    /// Commit immediately before forwarding the exact frozen swap. The transport
    /// adapter must derive melt_input_fee_sats from mint-scoped output-keyset
    /// metadata; this method bounds it but does not authenticate caller metadata.
    #[allow(clippy::too_many_arguments)]
    pub fn claim_treasury_split_submission(
        &mut self,
        binding: &TreasuryBinding,
        id: Uuid,
        destination: &url::Url,
        request: &[u8],
        melt_input_fee_sats: u64,
        policy: &AdmissionPolicy,
        now: i64,
    ) -> Result<()> {
        policy.validate()?;
        ensure!(
            policy.enabled && now >= 0,
            "split execution disabled or invalid time"
        );
        ensure!(request.len() <= 1_048_576, "split request exceeds bound");
        let issued = self.treasury_instruction(binding, id)?;
        binding.validate_new_invoice(&issued, u64::try_from(now)?)?;
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let (frozen,reservation,admitted,input,fee,digest):(Vec<u8>,Vec<u8>,i64,u64,u64,Vec<u8>)=tx.query_row(
            "SELECT q.frozen,r.reservation,a.admitted_at,a.input_sats,a.fee_sats,a.reservation_digest FROM treasury_split_requests q JOIN treasury_split_reservations r ON r.intent_id=q.intent_id JOIN treasury_split_admissions a ON a.intent_id=q.intent_id WHERE q.intent_id=? AND length(q.frozen)<=2097152 AND length(r.reservation)<=1048576",
            [id.to_string()],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?)))?;
        let frozen: Frozen = serde_json::from_slice(&frozen)?;
        let retained: Reservation = serde_json::from_slice(&reservation)?;
        retained.plan.validate_for(&issued.intent, policy.limits)?;
        let whole_input = retained
            .inputs
            .iter()
            .try_fold(0u64, |sum, p| sum.checked_add(p.amount))
            .ok_or_else(|| anyhow::anyhow!("split input overflow"))?;
        ensure!(
            input == whole_input
                && input <= policy.limits.maximum_input_sats
                && fee == retained.request.fee_sats
                && digest == Sha256::digest(&reservation).to_vec()
                && admitted <= now,
            "split submission differs from charged exposure"
        );
        ensure!(
            request == frozen.request
                && *destination == frozen.derivation.mint_url.join("v1/swap")?
                && frozen.split_id == retained.request.split_id,
            "split submission changes original request or destination"
        );
        let latest: Option<i64> = tx.query_row(
            "SELECT MAX(admitted_at) FROM treasury_split_admissions",
            [],
            |r| r.get(0),
        )?;
        ensure!(
            latest.is_none_or(|latest| latest <= now),
            "split submission clock rolled back"
        );
        let quote:Vec<u8>=tx.query_row("SELECT quote_json FROM treasury_quotes WHERE intent_id=? AND length(quote_json)<=65536",[id.to_string()],|r|r.get(0))?;
        let quote: cdk::wallet::MeltQuote = serde_json::from_slice(&quote)?;
        let total_fee = fee
            .checked_add(melt_input_fee_sats)
            .and_then(|n| n.checked_add(u64::from(quote.fee_reserve)))
            .ok_or_else(|| anyhow::anyhow!("split and melt fee overflow"))?;
        ensure!(
            quote.expiry > u64::try_from(now)?
                && quote.request == issued.invoice
                && quote.mint_url.as_ref() == Some(&frozen.derivation.mint_url)
                && quote.unit == cdk::nuts::CurrencyUnit::Sat
                && quote.payment_method == cdk::nuts::PaymentMethod::BOLT11
                && quote.state == cdk::nuts::MeltQuoteState::Unpaid
                && u64::from(quote.amount) == issued.intent.deposit_sats
                && total_fee <= issued.intent.maximum_fee_sats
                && issued
                    .intent
                    .deposit_sats
                    .checked_add(total_fee)
                    .is_some_and(|n| n <= issued.intent.input_sats),
            "split is not affordable under the original unexpired quote"
        );
        let ready:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM treasury_instructions WHERE id=?1 AND state='bound') AND NOT EXISTS(SELECT 1 FROM treasury_split_results WHERE intent_id=?1) AND NOT EXISTS(SELECT 1 FROM treasury_prepared WHERE intent_id=?1)",
            [id.to_string()],|r|r.get(0))?;
        ensure!(
            ready,
            "split is already complete or melt preparation has started"
        );
        for input in &retained.inputs {
            let owner: String = tx.query_row(
                "SELECT intent_id FROM treasury_proof_holds WHERE mint_id=? AND y=?",
                params![binding.mint_id, input.y],
                |r| r.get(0),
            )?;
            ensure!(
                owner == id.to_string(),
                "split input hold changed before submission"
            );
        }
        // Primary-key conflict is intentional: even identical bytes cannot
        // acquire a second network attempt after timeout or process restart.
        tx.execute("INSERT INTO treasury_split_submissions(intent_id,submitted_at,destination,request_digest,melt_input_fee_sats) VALUES(?,?,?,?,?)",
            params![id.to_string(),now,destination.as_str(),Sha256::digest(request).to_vec(),melt_input_fee_sats])?;
        tx.commit()?;
        Ok(())
    }

    /// Preserve the first response before CDK consumes it. This is transport
    /// evidence only; proof verification is still required for any result commit.
    pub fn record_treasury_split_reply(&mut self, id: Uuid, reply: &[u8]) -> Result<()> {
        ensure!(
            !reply.is_empty() && reply.len() <= 1_048_576,
            "invalid split response size"
        );
        let _: serde_json::Value = serde_json::from_slice(reply)?;
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let claimed: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM treasury_split_submissions WHERE intent_id=?)",
            [id.to_string()],
            |r| r.get(0),
        )?;
        ensure!(claimed, "split has no submission claim");
        let previous: Option<Vec<u8>> = tx
            .query_row(
                "SELECT reply FROM treasury_split_replies WHERE intent_id=?",
                [id.to_string()],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(previous) = previous {
            ensure!(previous == reply, "split reply conflicts with original");
        } else {
            tx.execute(
                "INSERT INTO treasury_split_replies(intent_id,reply) VALUES(?,?)",
                params![id.to_string(), reply],
            )?;
        }
        tx.commit()?;
        Ok(())
    }
}

impl Journal {
    pub(crate) fn unsubmitted_treasury_split(
        &self,
        binding: &TreasuryBinding,
        id: Uuid,
        limits: Limits,
    ) -> Result<Reservation> {
        let issued = self.treasury_instruction(binding, id)?;
        let bytes:Vec<u8>=self.connection.query_row(
            "SELECT r.reservation FROM treasury_split_reservations r JOIN treasury_instructions i ON i.id=r.intent_id WHERE i.id=? AND i.state='bound' AND length(r.reservation)<=1048576 AND NOT EXISTS(SELECT 1 FROM treasury_split_submissions s WHERE s.intent_id=i.id) AND NOT EXISTS(SELECT 1 FROM treasury_split_results s WHERE s.intent_id=i.id)",
            [id.to_string()],|r|r.get(0))?;
        let retained: Reservation = serde_json::from_slice(&bytes)?;
        retained.plan.validate_for(&issued.intent, limits)?;
        Ok(retained)
    }
    /// Read original recovery derivation without new execution authority/expiry.
    pub(crate) fn treasury_split_derivation(
        &self,
        binding: &TreasuryBinding,
        id: Uuid,
    ) -> Result<crate::ownership::SwapDerivation> {
        let issued = self.treasury_instruction(binding, id)?;
        let (bytes,digest):(Vec<u8>,Vec<u8>)=self.connection.query_row(
            "SELECT q.frozen,s.request_digest FROM treasury_split_requests q JOIN treasury_split_submissions s ON s.intent_id=q.intent_id WHERE q.intent_id=? AND length(q.frozen)<=2097152",
            [id.to_string()],|r|Ok((r.get(0)?,r.get(1)?)))?;
        let frozen: Frozen = serde_json::from_slice(&bytes)?;
        ensure!(
            frozen.intent_digest
                == lightning_goats_cashu_receipt::treasury::hex_digest(issued.intent.digest()?)
                && digest == Sha256::digest(&frozen.request).to_vec(),
            "split recovery binding changed"
        );
        Ok(frozen.derivation)
    }
}
