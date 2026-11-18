//! Durable treasury invoice, source, proof, quote and prepared-operation bindings.
//! These records alone grant no network submission authority.
use super::*;
use crate::treasury::TreasuryBinding;
use lightning_goats_cashu_receipt::treasury::{Issued, MAX_TREASURY_BYTES};

impl Journal {
    /// Only the authenticated core IPC handler may supply this instruction.
    /// Admission records an invoice, never permission to send a melt request.
    /// Exact replay remains readable after expiry without authorizing new work.
    pub fn bind_treasury_instruction(
        &mut self,
        binding: &TreasuryBinding,
        issued: &Issued,
        now: u64,
    ) -> Result<bool> {
        binding.validate_scope(issued)?;
        let bytes = serde_json::to_vec(issued)?;
        ensure!(
            bytes.len() <= MAX_TREASURY_BYTES,
            "treasury instruction exceeds bound"
        );
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let epoch: String = tx.query_row(
            "SELECT epoch FROM wallet_epoch WHERE singleton=1",
            [],
            |r| r.get(0),
        )?;
        ensure!(
            epoch == issued.intent.wallet_epoch.to_string(),
            "treasury wallet epoch mismatch"
        );
        let previous: Option<Vec<u8>> = tx
            .query_row(
                "SELECT issued_json FROM treasury_instructions WHERE id=?",
                [issued.intent.id.to_string()],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(previous) = previous {
            ensure!(
                previous == bytes,
                "treasury instruction conflicts with original invoice"
            );
            tx.commit()?;
            return Ok(false);
        }
        binding.validate_new_invoice(issued, now)?;
        let pending: i64 = tx.query_row(
            "SELECT COUNT(*) FROM treasury_instructions WHERE state!='reconciled'",
            [],
            |r| r.get(0),
        )?;
        ensure!(
            pending < 1024,
            "treasury admission full; retain original core instruction"
        );
        tx.execute("INSERT INTO treasury_instructions(id,issued_json,receive_id,payment_hash) VALUES(?,?,?,?)",
            params![issued.intent.id.to_string(), bytes, issued.receive_request_id.to_string(), issued.payment_hash])?;
        tx.commit()?;
        Ok(true)
    }
}

impl Journal {
    /// Reload the original instruction for reconciliation, including after expiry.
    /// This read does not allow a new quote, submission or replacement invoice.
    pub fn treasury_instruction(&self, binding: &TreasuryBinding, id: Uuid) -> Result<Issued> {
        let bytes: Vec<u8> = self.connection.query_row(
            "SELECT issued_json FROM treasury_instructions WHERE id=? AND length(issued_json)<=?",
            params![id.to_string(), MAX_TREASURY_BYTES],
            |r| r.get(0),
        )?;
        let issued: Issued = serde_json::from_slice(&bytes)?;
        binding.validate_scope(&issued)?;
        ensure!(issued.intent.id == id, "stored treasury identity mismatch");
        let epoch: String = self.connection.query_row(
            "SELECT epoch FROM wallet_epoch WHERE singleton=1",
            [],
            |r| r.get(0),
        )?;
        ensure!(
            epoch == binding.wallet_epoch.to_string(),
            "stored treasury epoch mismatch"
        );
        Ok(issued)
    }

    /// Reserve acquired source value atomically across feed settlements and
    /// sweeps. Exact CDK proof reservation is additionally required before melt.
    /// Holds are never released merely because an invoice expires or a reply is lost.
    pub fn reserve_treasury_sources(
        &mut self,
        binding: &TreasuryBinding,
        id: Uuid,
        now: u64,
    ) -> Result<()> {
        let issued = self.treasury_instruction(binding, id)?;
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let existing: i64 = tx.query_row(
            "SELECT COUNT(*) FROM treasury_source_holds WHERE intent_id=?",
            [id.to_string()],
            |r| r.get(0),
        )?;
        if existing != 0 {
            ensure!(
                existing as usize == issued.intent.sources.len(),
                "partial treasury reservation"
            );
            for source in &issued.intent.sources {
                let amount: u64 = tx.query_row("SELECT input_sats FROM treasury_source_holds WHERE intent_id=? AND acquisition_id=?",
                    params![id.to_string(),source.acquisition_id.to_string()], |r| r.get(0))?;
                ensure!(amount == source.input_sats, "treasury reservation conflict");
            }
            tx.commit()?;
            return Ok(());
        }
        binding.validate_new_invoice(&issued, now)?;
        for source in &issued.intent.sources {
            let receipt = tx.query_row(
                "SELECT a.mint_id,a.policy,a.transport,a.source_ref,a.gross,a.receive_fee,a.net,a.acquired_evidence,c.source_author FROM acquisitions a JOIN acquisition_context c ON c.acquisition_id=a.id WHERE a.id=? AND a.state IN ('credit_export_pending','credited')",
                [source.acquisition_id.to_string()], |r| Ok(Receipt {
                    schema_version: 1, wallet_epoch: binding.wallet_epoch, acquisition_id: source.acquisition_id,
                    mint_id: r.get(0)?, policy_version: r.get(1)?, transport: r.get(2)?, source_ref: r.get(3)?,
                    gross_sats: r.get(4)?, receive_fee_sats: r.get(5)?, net_sats: r.get(6)?, evidence_digest: r.get(7)?,
                    source_author: r.get(8)?, credit_pool: "herd",
                }))?;
            let wire = crate::receipt_wire::from_private(binding.wallet_instance, &receipt)?;
            ensure!(
                receipt.mint_id == issued.intent.mint_id
                    && lightning_goats_cashu_receipt::treasury::hex_digest(wire.digest()?)
                        == source.receipt_digest,
                "treasury source differs from original acquisition receipt"
            );
            let held: u64 = tx.query_row("SELECT COALESCE(SUM(h.input_sats-COALESCE(r.change_sats,0)),0) FROM treasury_source_holds h LEFT JOIN treasury_source_results r ON r.intent_id=h.intent_id AND r.acquisition_id=h.acquisition_id WHERE h.acquisition_id=?",
                [source.acquisition_id.to_string()], |r| r.get(0))?;
            ensure!(
                held.checked_add(source.input_sats)
                    .is_some_and(|sum| sum <= receipt.net_sats),
                "treasury source is already reserved or exceeds owned value"
            );
            tx.execute("INSERT INTO treasury_source_holds(intent_id,acquisition_id,input_sats) VALUES(?,?,?)",
                params![id.to_string(),source.acquisition_id.to_string(),source.input_sats])?;
        }
        tx.commit()?;
        Ok(())
    }
}

impl Journal {
    /// Freeze exact output Ys after source-value reservation. Caller must still
    /// validate current CDK ownership, DLEQ and mint state before submission.
    /// Selections are canonical and immutable; cancellation needs a separate
    /// verified nonpayment/recovery transition, never a timeout-based release.
    pub fn reserve_treasury_proof_ids(
        &mut self,
        binding: &TreasuryBinding,
        id: Uuid,
        selected: &BTreeMap<Uuid, Vec<String>>,
        now: u64,
    ) -> Result<()> {
        let issued = self.treasury_instruction(binding, id)?;
        ensure!(
            selected.len() == issued.intent.sources.len(),
            "proof selection source count mismatch"
        );
        let mut unique = std::collections::BTreeSet::new();
        for ys in selected.values() {
            ensure!(ys.len() <= self.max_proofs, "invalid selected proof count");
            unique.extend(ys.iter());
            ensure!(
                unique.len() <= self.max_proofs,
                "proof selection exceeds bound"
            );
            ensure!(
                ys.windows(2).all(|pair| pair[0] < pair[1]),
                "proof Ys must be unique and sorted"
            );
            for y in ys {
                ensure!(
                    cdk::nuts::PublicKey::from_hex(y)?.to_hex() == *y,
                    "noncanonical proof Y"
                );
            }
        }
        let bytes = serde_json::to_vec(selected)?;
        ensure!(bytes.len() <= 1_048_576, "proof selection too large");
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let budget = super::treasury_budget::verified(&tx, &issued.intent)?;
        let previous: Option<Vec<u8>> = tx
            .query_row(
                "SELECT selection FROM treasury_proof_selections WHERE intent_id=?",
                [id.to_string()],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(previous) = previous {
            ensure!(
                previous == bytes,
                "treasury proof selection conflicts with original"
            );
            tx.commit()?;
            return Ok(());
        }
        binding.validate_new_invoice(&issued, now)?;
        for source in &issued.intent.sources {
            let ys = selected
                .get(&source.acquisition_id)
                .ok_or_else(|| anyhow::anyhow!("source proof selection missing"))?;
            let held: u64 = tx.query_row("SELECT input_sats FROM treasury_source_holds WHERE intent_id=? AND acquisition_id=?",
                params![id.to_string(),source.acquisition_id.to_string()], |r|r.get(0))?;
            ensure!(held == source.input_sats, "source value was not reserved");
            let mut amount = 0u64;
            for y in ys {
                let value: u64 = tx.query_row("SELECT amount FROM treasury_output_parts WHERE mint_id=? AND y=? AND acquisition_id=?",
                    params![binding.mint_id,y,source.acquisition_id.to_string()], |r|r.get(0))?;
                amount = amount
                    .checked_add(value)
                    .ok_or_else(|| anyhow::anyhow!("proof value overflow"))?;
                tx.execute(
                    "INSERT INTO treasury_proof_holds(mint_id,y,intent_id) VALUES(?,?,?) ON CONFLICT(mint_id,y) DO NOTHING",
                    params![binding.mint_id, y, id.to_string()],
                )?;
                let owner: String = tx.query_row(
                    "SELECT intent_id FROM treasury_proof_holds WHERE mint_id=? AND y=?",
                    params![binding.mint_id, y],
                    |r| r.get(0),
                )?;
                ensure!(
                    owner == id.to_string(),
                    "proof is reserved by another operation"
                );
            }
            ensure!(
                amount == budget.sources[&source.acquisition_id],
                "selected proof denominations do not match reserved source value"
            );
        }
        let complete:u64 = tx.query_row("SELECT COALESCE(SUM(o.amount),0) FROM treasury_owned_outputs o JOIN treasury_proof_holds h ON h.mint_id=o.mint_id AND h.y=o.y WHERE h.intent_id=?",[id.to_string()],|r|r.get(0))?;
        ensure!(
            complete == budget.input_sats,
            "shared proof requires all original source parts"
        );
        tx.execute(
            "INSERT INTO treasury_proof_selections(intent_id,selection) VALUES(?,?)",
            params![id.to_string(), bytes],
        )?;
        tx.commit()?;
        Ok(())
    }
}

impl Journal {
    /// Freeze the mint's original unpaid BOLT11 quote. This does not submit it.
    /// Quote state observations after submission belong to reconciliation, not
    /// replacement of these immutable terms.
    pub fn bind_treasury_quote(
        &mut self,
        binding: &TreasuryBinding,
        mint: &crate::mint_policy::MintPolicy,
        id: Uuid,
        quote: &cdk::wallet::MeltQuote,
        now: u64,
    ) -> Result<()> {
        let issued = self.treasury_instruction(binding, id)?;
        let budget = super::treasury_budget::for_quote(&self.connection, &issued.intent)?;
        ensure!(
            mint.id == binding.mint_id && mint.bolt11_exit_qualified,
            "treasury mint is not qualified"
        );
        let expected_url: cdk::mint_url::MintUrl = mint.advertised_url.parse()?;
        ensure!(
            quote.mint_url.as_ref() == Some(&expected_url)
                && quote.request == issued.invoice
                && quote.unit == cdk::nuts::CurrencyUnit::Sat
                && quote.payment_method == cdk::nuts::PaymentMethod::BOLT11
                && u64::from(quote.amount) == issued.intent.deposit_sats
                && u64::from(quote.fee_reserve) <= budget.maximum_fee_sats
                && issued
                    .intent
                    .deposit_sats
                    .checked_add(u64::from(quote.fee_reserve))
                    .is_some_and(|n| n <= budget.input_sats),
            "melt quote changes frozen invoice, mint, unit or fee authority"
        );
        ensure!(
            !quote.id.is_empty()
                && quote.id.len() <= 256
                && !quote.id.chars().any(char::is_control)
                && quote.state == cdk::nuts::MeltQuoteState::Unpaid
                && quote.payment_proof.is_none()
                && quote.used_by_operation.is_none()
                && quote.estimated_blocks.is_none()
                && quote.fee_index.is_none(),
            "new treasury quote is not an unused Lightning quote"
        );
        let bytes = serde_json::to_vec(quote)?;
        ensure!(
            bytes.len() <= MAX_TREASURY_BYTES,
            "melt quote exceeds bound"
        );
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let previous: Option<Vec<u8>> = tx
            .query_row(
                "SELECT quote_json FROM treasury_quotes WHERE intent_id=?",
                [id.to_string()],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(previous) = previous {
            ensure!(
                previous == bytes,
                "melt quote conflicts with frozen original"
            );
            tx.commit()?;
            return Ok(());
        }
        binding.validate_new_invoice(&issued, now)?;
        ensure!(quote.expiry > now, "melt quote expired before binding");
        let reserved: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM treasury_proof_selections WHERE intent_id=?1) OR EXISTS(SELECT 1 FROM treasury_split_reservations WHERE intent_id=?1)",
            [id.to_string()],
            |r| r.get(0),
        )?;
        ensure!(reserved, "melt quote requires reserved source proofs");
        tx.execute(
            "INSERT INTO treasury_quotes(intent_id,mint_id,quote_id,quote_json) VALUES(?,?,?,?)",
            params![id.to_string(), binding.mint_id, quote.id, bytes],
        )?;
        tx.commit()?;
        Ok(())
    }
}

impl Journal {
    /// Claim one CDK preparation attempt before it can reserve proofs/create a
    /// saga. A failed/canceled preparation requires recovery, never a new saga.
    pub fn claim_treasury_preparation(
        &mut self,
        binding: &TreasuryBinding,
        id: Uuid,
        now: u64,
    ) -> Result<(cdk::wallet::MeltQuote, Vec<cdk::nuts::PublicKey>)> {
        let issued = self.treasury_instruction(binding, id)?;
        binding.validate_new_invoice(&issued, now)?;
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        super::treasury_budget::verified(&tx, &issued.intent)?;
        let selected: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM treasury_proof_selections WHERE intent_id=?)",
            [id.to_string()],
            |r| r.get(0),
        )?;
        ensure!(selected, "melt preparation requires final selected proofs");
        let (state, bytes): (String, Vec<u8>) = tx.query_row(
            "SELECT i.state,q.quote_json FROM treasury_instructions i JOIN treasury_quotes q ON q.intent_id=i.id WHERE i.id=? AND length(q.quote_json)<=?",
            params![id.to_string(), MAX_TREASURY_BYTES], |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        ensure!(state == "bound", "treasury already prepared or submitted");
        let quote: cdk::wallet::MeltQuote = serde_json::from_slice(&bytes)?;
        ensure!(
            quote.expiry > now && quote.request == issued.invoice,
            "treasury quote expired or changed"
        );
        let ys = {
            let mut stmt = tx.prepare(
                "SELECT h.y FROM treasury_proof_holds h JOIN treasury_owned_outputs o ON o.mint_id=h.mint_id AND o.y=h.y WHERE h.intent_id=? ORDER BY h.y LIMIT ?",
            )?;
            stmt.query_map(
                params![id.to_string(), self.max_proofs.saturating_add(1)],
                |r| r.get::<_, String>(0),
            )?
            .collect::<Result<Vec<_>, _>>()?
        };
        ensure!(
            !ys.is_empty() && ys.len() <= self.max_proofs,
            "invalid reserved proof count"
        );
        let ys = ys
            .into_iter()
            .map(|y| y.parse())
            .collect::<Result<Vec<_>, _>>()?;
        tx.execute(
            "INSERT INTO treasury_preparation_claims(intent_id) VALUES(?)",
            [id.to_string()],
        )?;
        tx.commit()?;
        Ok((quote, ys))
    }

    /// Read the retained pre-CDK claim without allocating another preparation.
    pub(crate) fn claimed_treasury_preparation(
        &self,
        binding: &TreasuryBinding,
        id: Uuid,
        now: u64,
    ) -> Result<(cdk::wallet::MeltQuote, Vec<cdk::nuts::PublicKey>)> {
        let issued = self.treasury_instruction(binding, id)?;
        binding.validate_new_invoice(&issued, now)?;
        let bytes: Vec<u8> = self.connection.query_row(
            "SELECT q.quote_json FROM treasury_instructions i JOIN treasury_quotes q ON q.intent_id=i.id JOIN treasury_preparation_claims c ON c.intent_id=i.id WHERE i.id=? AND i.state='bound' AND NOT EXISTS(SELECT 1 FROM treasury_prepared p WHERE p.intent_id=i.id) AND NOT EXISTS(SELECT 1 FROM treasury_melt_requests r WHERE r.intent_id=i.id) AND length(q.quote_json)<=?",
            params![id.to_string(), MAX_TREASURY_BYTES], |r| r.get(0),
        )?;
        let quote: cdk::wallet::MeltQuote = serde_json::from_slice(&bytes)?;
        ensure!(
            quote.expiry > now && quote.request == issued.invoice,
            "claimed quote expired or changed"
        );
        let mut stmt = self
            .connection
            .prepare("SELECT h.y FROM treasury_proof_holds h JOIN treasury_owned_outputs o ON o.mint_id=h.mint_id AND o.y=h.y WHERE h.intent_id=? ORDER BY h.y LIMIT ?")?;
        let ys = stmt
            .query_map(
                params![id.to_string(), self.max_proofs.saturating_add(1)],
                |r| r.get::<_, String>(0),
            )?
            .collect::<Result<Vec<_>, _>>()?;
        ensure!(
            !ys.is_empty() && ys.len() <= self.max_proofs,
            "invalid original proof holds"
        );
        Ok((
            quote,
            ys.into_iter()
                .map(|y| y.parse())
                .collect::<Result<Vec<_>, _>>()?,
        ))
    }

    /// Load the original prepared identity only before a final request exists.
    /// A frozen/submitted request must use reconciliation, not preparation replay.
    pub(crate) fn unsubmitted_treasury_preparation(
        &self,
        binding: &TreasuryBinding,
        id: Uuid,
        now: u64,
    ) -> Result<crate::treasury::PreparedContext> {
        let issued = self.treasury_instruction(binding, id)?;
        let budget = super::treasury_budget::verified(&self.connection, &issued.intent)?;
        binding.validate_new_invoice(&issued, now)?;
        let (quote, prepared): (Vec<u8>, Vec<u8>) = self.connection.query_row(
            "SELECT q.quote_json,p.prepared_json FROM treasury_instructions i JOIN treasury_quotes q ON q.intent_id=i.id JOIN treasury_prepared p ON p.intent_id=i.id WHERE i.id=? AND i.state='prepared' AND NOT EXISTS(SELECT 1 FROM treasury_melt_requests r WHERE r.intent_id=i.id) AND length(q.quote_json)<=? AND length(p.prepared_json)<=?",
            params![id.to_string(), MAX_TREASURY_BYTES, MAX_TREASURY_BYTES], |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        let quote: cdk::wallet::MeltQuote = serde_json::from_slice(&quote)?;
        let (version, intent, operation, quote_id, fee, ys): (
            u8,
            Uuid,
            Uuid,
            String,
            u64,
            std::collections::BTreeSet<String>,
        ) = serde_json::from_slice(&prepared)?;
        ensure!(
            version == 1
                && intent == id
                && !operation.is_nil()
                && quote.id == quote_id
                && quote.request == issued.invoice
                && quote.expiry > now
                && fee
                    .checked_add(u64::from(quote.fee_reserve))
                    .is_some_and(|f| f <= budget.maximum_fee_sats)
                && !ys.is_empty()
                && ys.len() <= self.max_proofs,
            "invalid original treasury preparation"
        );
        let held = {
            let mut stmt = self
                .connection
                .prepare("SELECT h.y FROM treasury_proof_holds h JOIN treasury_owned_outputs o ON o.mint_id=h.mint_id AND o.y=h.y WHERE h.intent_id=?")?;
            stmt.query_map([id.to_string()], |r| r.get::<_, String>(0))?
                .collect::<Result<std::collections::BTreeSet<_>, _>>()?
        };
        ensure!(held == ys, "original treasury proof holds changed");
        Ok(crate::treasury::PreparedContext {
            operation,
            quote,
            input_fee: fee,
            ys: ys
                .into_iter()
                .map(|y| y.parse())
                .collect::<Result<Vec<_>, _>>()?,
        })
    }

    /// Bind CDK's actual prepared operation to our immutable quote/proof holds.
    /// This boundary selects direct submission with skip_swap: an implicit CDK
    /// swap would replace proof provenance and needs its own journaled workflow.
    /// It grants no network authority; the transport must freeze the final melt
    /// request and change derivation before allowing the original submission.
    pub fn bind_prepared_treasury_melt(
        &mut self,
        binding: &TreasuryBinding,
        id: Uuid,
        prepared: &cdk::wallet::PreparedMelt<'_>,
        now: u64,
    ) -> Result<()> {
        let proofs = prepared
            .proofs()
            .iter()
            .chain(prepared.proofs_to_swap())
            .cloned()
            .collect::<Vec<_>>();
        self.bind_treasury_preparation_evidence(
            binding,
            id,
            prepared.operation_id(),
            prepared.quote(),
            u64::from(prepared.input_fee_without_swap()),
            &proofs,
            now,
        )
    }

    /// Shared exact evidence check for a live PreparedMelt or a recovered CDK saga.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn bind_treasury_preparation_evidence(
        &mut self,
        binding: &TreasuryBinding,
        id: Uuid,
        operation: Uuid,
        current: &cdk::wallet::MeltQuote,
        input_fee: u64,
        proofs: &[cdk::nuts::Proof],
        now: u64,
    ) -> Result<()> {
        self.check_treasury_preparation_evidence(
            binding, id, operation, current, input_fee, proofs, now, true,
        )
    }

    /// Validate the same evidence without committing preparation. Used before
    /// restoring a missing local saga under its already retained operation UUID.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn check_treasury_preparation_evidence(
        &mut self,
        binding: &TreasuryBinding,
        id: Uuid,
        operation: Uuid,
        current: &cdk::wallet::MeltQuote,
        input_fee: u64,
        proofs: &[cdk::nuts::Proof],
        now: u64,
        persist: bool,
    ) -> Result<()> {
        let issued = self.treasury_instruction(binding, id)?;
        let budget = super::treasury_budget::verified(&self.connection, &issued.intent)?;
        ensure!(!operation.is_nil(), "missing CDK melt operation identity");
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let quote_bytes: Vec<u8> = tx.query_row(
            "SELECT quote_json FROM treasury_quotes WHERE intent_id=?",
            [id.to_string()],
            |r| r.get(0),
        )?;
        let original: cdk::wallet::MeltQuote = serde_json::from_slice(&quote_bytes)?;
        ensure!(
            current.id == original.id
                && current.request == original.request
                && current.mint_url == original.mint_url
                && current.unit == original.unit
                && current.amount == original.amount
                && current.fee_reserve == original.fee_reserve
                && current.expiry == original.expiry
                && current.payment_method == original.payment_method,
            "CDK prepared quote differs from frozen quote"
        );
        let total_fee = input_fee
            .checked_add(u64::from(original.fee_reserve))
            .ok_or_else(|| anyhow::anyhow!("melt fee overflow"))?;
        ensure!(
            total_fee <= budget.maximum_fee_sats
                && issued
                    .intent
                    .deposit_sats
                    .checked_add(total_fee)
                    .is_some_and(|n| n <= budget.input_sats),
            "CDK input fees exceed frozen treasury authority"
        );
        let selection_bytes: Vec<u8> = tx.query_row(
            "SELECT selection FROM treasury_proof_selections WHERE intent_id=?",
            [id.to_string()],
            |r| r.get(0),
        )?;
        let selection: BTreeMap<Uuid, Vec<String>> = serde_json::from_slice(&selection_bytes)?;
        let expected: std::collections::BTreeSet<String> =
            selection.values().flatten().cloned().collect();
        let mut actual = std::collections::BTreeSet::new();
        let mut total = 0u64;
        for proof in proofs {
            let y = proof.y()?.to_hex();
            ensure!(actual.insert(y.clone()), "duplicate prepared melt proof");
            let (c,amount): (String,u64) = tx.query_row(
                "SELECT o.c,o.amount FROM treasury_owned_outputs o JOIN treasury_proof_holds h ON h.mint_id=o.mint_id AND h.y=o.y WHERE h.intent_id=? AND h.mint_id=? AND h.y=?",
                params![id.to_string(),binding.mint_id,y],|r|Ok((r.get(0)?,r.get(1)?)))?;
            ensure!(
                proof.c.to_hex() == c && u64::from(proof.amount) == amount,
                "prepared melt proof differs from owned output"
            );
            total = total
                .checked_add(amount)
                .ok_or_else(|| anyhow::anyhow!("melt inputs overflow"))?;
        }
        ensure!(
            actual == expected && total == budget.input_sats,
            "CDK proof selection differs from reserved sources"
        );
        if !persist {
            binding.validate_new_invoice(&issued, now)?;
            ensure!(original.expiry > now, "prepared quote expired");
            return Ok(());
        }
        let bytes = serde_json::to_vec(&(1, id, operation, original.id, input_fee, actual))?;
        let previous: Option<Vec<u8>> = tx
            .query_row(
                "SELECT prepared_json FROM treasury_prepared WHERE intent_id=?",
                [id.to_string()],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(previous) = previous {
            ensure!(
                previous == bytes,
                "CDK melt operation conflicts with original"
            );
        } else {
            binding.validate_new_invoice(&issued, now)?;
            ensure!(original.expiry > now, "prepared quote expired");
            tx.execute(
                "INSERT INTO treasury_prepared(intent_id,operation_id,prepared_json) VALUES(?,?,?)",
                params![id.to_string(), operation.to_string(), bytes],
            )?;
            ensure!(tx.execute(
                "UPDATE treasury_instructions SET state='prepared' WHERE id=? AND state='bound'",
                [id.to_string()],
            )? == 1, "treasury state changed before preparation binding");
        }
        tx.commit()?;
        Ok(())
    }
}

impl Journal {
    /// Freeze private spend material before transport submission. Replays only
    /// compare retained bytes; they do not grant permission to send them again.
    #[allow(clippy::too_many_arguments)]
    pub fn freeze_treasury_melt_request(
        &mut self,
        binding: &TreasuryBinding,
        id: Uuid,
        saga: &cdk::wallet::types::WalletSaga,
        request: &cdk::nuts::MeltRequest<String>,
        seed: &[u8; 64],
        now: u64,
    ) -> Result<()> {
        let issued = self.treasury_instruction(binding, id)?;
        let budget = super::treasury_budget::verified(&self.connection, &issued.intent)?;
        let bytes = serde_json::to_vec(request)?;
        ensure!(bytes.len() <= 1_048_576, "melt request exceeds bound");
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let prepared_bytes: Vec<u8> = tx.query_row(
            "SELECT prepared_json FROM treasury_prepared WHERE intent_id=?",
            [id.to_string()],
            |r| r.get(0),
        )?;
        let (version, intent, operation, quote_id, input_fee, expected): (
            u8,
            Uuid,
            Uuid,
            String,
            u64,
            std::collections::BTreeSet<String>,
        ) = serde_json::from_slice(&prepared_bytes)?;
        ensure!(
            version == 1 && intent == id && operation == saga.id && &quote_id == request.quote_id(),
            "melt differs from prepared operation"
        );
        let derivation =
            crate::melt_derivation::MeltDerivation::capture(saga, request, input_fee, seed)?;
        let quote_bytes: Vec<u8> = tx.query_row(
            "SELECT quote_json FROM treasury_quotes WHERE intent_id=?",
            [id.to_string()],
            |r| r.get(0),
        )?;
        let quote: cdk::wallet::MeltQuote = serde_json::from_slice(&quote_bytes)?;
        let cdk::wallet::types::OperationData::Melt(data) = &saga.data else {
            anyhow::bail!("not a melt saga")
        };
        ensure!(
            quote.mint_url.as_ref() == Some(&saga.mint_url)
                && quote.amount == data.amount
                && quote.fee_reserve == data.fee_reserve
                && u64::from(data.amount) == issued.intent.deposit_sats,
            "melt differs from frozen quote"
        );
        let mut actual = std::collections::BTreeSet::new();
        let mut total = 0u64;
        for proof in request.inputs() {
            let y = proof.y()?.to_hex();
            ensure!(actual.insert(y.clone()), "duplicate final melt proof");
            let (c, amount): (String, u64) = tx.query_row(
                "SELECT o.c,o.amount FROM treasury_owned_outputs o JOIN treasury_proof_holds h ON h.mint_id=o.mint_id AND h.y=o.y WHERE h.intent_id=? AND h.mint_id=? AND h.y=?",
                params![id.to_string(),binding.mint_id,y], |r|Ok((r.get(0)?,r.get(1)?)))?;
            ensure!(
                proof.c.to_hex() == c && u64::from(proof.amount) == amount,
                "final melt input differs from owned output"
            );
            total = total
                .checked_add(amount)
                .ok_or_else(|| anyhow::anyhow!("melt input overflow"))?;
        }
        ensure!(
            actual == expected && total == budget.input_sats,
            "final melt inputs differ from reservation"
        );
        let material = serde_json::to_vec(&derivation)?;
        ensure!(
            material.len() <= 1_048_576,
            "melt recovery material exceeds bound"
        );
        let previous: Option<(Vec<u8>, Vec<u8>)> = tx
            .query_row(
                "SELECT request,derivation FROM treasury_melt_requests WHERE intent_id=?",
                [id.to_string()],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        if let Some((original_request, original_material)) = previous {
            ensure!(
                original_request == bytes && original_material == material,
                "melt request conflicts with original"
            );
        } else {
            binding.validate_new_invoice(&issued, now)?;
            ensure!(
                quote.expiry > now,
                "melt quote expired before request freeze"
            );
            tx.execute(
                "INSERT INTO treasury_melt_requests(intent_id,request,derivation) VALUES(?,?,?)",
                params![id.to_string(), bytes, material],
            )?;
        }
        tx.commit()?;
        Ok(())
    }
}

impl Journal {
    /// Commit the one submission attempt before the network call. Failure or a
    /// lost reply after this commit requires read-only reconciliation; this
    /// method never reauthorizes even identical request bytes.
    pub fn claim_treasury_melt_submission(
        &mut self,
        binding: &TreasuryBinding,
        id: Uuid,
        url: &url::Url,
        request: &[u8],
        now: u64,
    ) -> Result<()> {
        let issued = self.treasury_instruction(binding, id)?;
        binding.validate_new_invoice(&issued, now)?;
        ensure!(request.len() <= 1_048_576, "melt request exceeds bound");
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let (original, material): (Vec<u8>, Vec<u8>) = tx.query_row(
            "SELECT request,derivation FROM treasury_melt_requests WHERE intent_id=?",
            [id.to_string()],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        ensure!(
            original == request,
            "submission differs from frozen melt request"
        );
        let derivation: crate::melt_derivation::MeltDerivation = serde_json::from_slice(&material)?;
        let expected = derivation.mint_url.join("v1/melt/bolt11")?;
        ensure!(
            expected == *url,
            "submission destination differs from original mint"
        );
        let quote_bytes: Vec<u8> = tx.query_row(
            "SELECT quote_json FROM treasury_quotes WHERE intent_id=?",
            [id.to_string()],
            |r| r.get(0),
        )?;
        let quote: cdk::wallet::MeltQuote = serde_json::from_slice(&quote_bytes)?;
        ensure!(quote.expiry > now, "melt quote expired before submission");
        let changed = tx.execute(
            "UPDATE treasury_instructions SET state='submitted' WHERE id=? AND state='prepared'",
            [id.to_string()],
        )?;
        ensure!(
            changed == 1,
            "original melt already submitted or requires reconciliation"
        );
        tx.commit()?;
        Ok(())
    }
}

impl Journal {
    /// Preserve the initial transport response before CDK consumes it. This is
    /// private recovery evidence, never settlement or recovered-change authority.
    pub fn record_treasury_melt_reply(&mut self, id: Uuid, reply: &[u8]) -> Result<()> {
        ensure!(
            !reply.is_empty() && reply.len() <= 1_048_576,
            "invalid melt reply size"
        );
        let _: serde_json::Value = serde_json::from_slice(reply)?;
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let state: String = tx.query_row(
            "SELECT state FROM treasury_instructions WHERE id=?",
            [id.to_string()],
            |r| r.get(0),
        )?;
        ensure!(
            state == "submitted" || state == "reconciled",
            "melt has no submitted attempt"
        );
        let previous: Option<Vec<u8>> = tx
            .query_row(
                "SELECT reply FROM treasury_melt_replies WHERE intent_id=?",
                [id.to_string()],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(previous) = previous {
            ensure!(
                previous == reply,
                "initial melt reply conflicts with retained evidence"
            );
        } else {
            tx.execute(
                "INSERT INTO treasury_melt_replies(intent_id,reply) VALUES(?,?)",
                params![id.to_string(), reply],
            )?;
        }
        tx.commit()?;
        Ok(())
    }
}

impl Journal {
    /// Complete wallet-side ownership accounting. Core must still authenticate
    /// this report and independently verify the original Strike deposit.
    #[allow(clippy::too_many_arguments)]
    pub fn reconcile_treasury_melt(
        &mut self,
        binding: &TreasuryBinding,
        id: Uuid,
        paid: &cdk::nuts::MeltQuoteBolt11Response<String>,
        seed: &[u8; 64],
        keys: &crate::intake::PinnedKeys,
        owned_change: &[cdk::wallet::types::ProofInfo],
        fresh_states: &cdk::nuts::CheckStateResponse,
    ) -> Result<lightning_goats_cashu_receipt::treasury::MeltData> {
        use lightning_goats_cashu_receipt::treasury::{MeltData, hex_digest};
        let issued = self.treasury_instruction(binding, id)?;
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let budget = super::treasury_budget::verified(&tx, &issued.intent)?;
        let preparation_fee = issued
            .intent
            .input_sats
            .checked_sub(budget.input_sats)
            .ok_or_else(|| anyhow::anyhow!("invalid split preparation fee"))?;
        let state: String = tx.query_row(
            "SELECT state FROM treasury_instructions WHERE id=?",
            [id.to_string()],
            |r| r.get(0),
        )?;
        ensure!(
            state == "submitted",
            "melt is not awaiting original reconciliation"
        );
        let quote_bytes: Vec<u8> = tx.query_row(
            "SELECT quote_json FROM treasury_quotes WHERE intent_id=?",
            [id.to_string()],
            |r| r.get(0),
        )?;
        let quote: cdk::wallet::MeltQuote = serde_json::from_slice(&quote_bytes)?;
        crate::melt_reconciliation::validate_paid_quote(&issued, &quote, paid)?;
        let (request_bytes, derivation_bytes): (Vec<u8>, Vec<u8>) = tx.query_row(
            "SELECT request,derivation FROM treasury_melt_requests WHERE intent_id=?",
            [id.to_string()],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        let derivation: crate::melt_derivation::MeltDerivation =
            serde_json::from_slice(&derivation_bytes)?;
        ensure!(
            derivation.quote_id == quote.id
                && quote.mint_url.as_ref() == Some(&derivation.mint_url),
            "change recovery scope mismatch"
        );
        let mut restore_bytes = None;
        let signatures = match &paid.change {
            Some(signatures) => signatures.clone(),
            None if derivation.maximum_change_sats == 0 => vec![],
            None => {
                let (original_paid, response): (Vec<u8>, Vec<u8>) = tx.query_row(
                    "SELECT paid,response FROM treasury_restore_evidence WHERE intent_id=?",
                    [id.to_string()],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )?;
                ensure!(
                    original_paid == serde_json::to_vec(paid)?,
                    "restore evidence belongs to a different paid observation"
                );
                let restored: cdk::nuts::RestoreResponse = serde_json::from_slice(&response)?;
                let normalized =
                    crate::melt_reconciliation::normalize_restored_change(&derivation, &restored)?;
                restore_bytes = Some(response);
                normalized
            }
        };
        let change = crate::melt_reconciliation::verify_owned_change(
            &derivation,
            seed,
            &binding.mint_id,
            keys,
            &signatures,
            owned_change,
            fresh_states,
        )?;
        let fee = issued
            .intent
            .input_sats
            .checked_sub(issued.intent.deposit_sats)
            .and_then(|n| n.checked_sub(change))
            .ok_or_else(|| anyhow::anyhow!("melt conservation failure"))?;
        let input_fee = budget
            .input_sats
            .checked_sub(issued.intent.deposit_sats)
            .and_then(|n| n.checked_sub(derivation.maximum_change_sats))
            .ok_or_else(|| anyhow::anyhow!("invalid original melt input fee"))?;
        let melt_fee = fee
            .checked_sub(preparation_fee)
            .ok_or_else(|| anyhow::anyhow!("terminal fee omits verified split cost"))?;
        ensure!(
            melt_fee <= budget.maximum_fee_sats
                && input_fee
                    .checked_add(u64::from(quote.fee_reserve))
                    .is_some_and(|cap| melt_fee <= cap),
            "actual melt fee exceeds remaining input fee and quote reserve"
        );
        let mut outputs = owned_change
            .iter()
            .map(|info| {
                (
                    info.y.to_hex(),
                    info.proof.c.to_hex(),
                    u64::from(info.proof.amount),
                )
            })
            .collect::<Vec<_>>();
        outputs.sort();
        let paid_bytes = serde_json::to_vec(paid)?;
        ensure!(paid_bytes.len() <= 1_048_576, "paid evidence exceeds bound");
        let split_evidence: Option<(Vec<u8>,Vec<u8>)>=tx.query_row(
            "SELECT frozen_digest,result FROM treasury_split_results WHERE intent_id=? AND length(result)<=1048576",
            [id.to_string()],|r|Ok((r.get(0)?,r.get(1)?))).optional()?;
        let evidence = if let Some(split) = split_evidence {
            serde_json::to_vec(&(
                2,
                id,
                &request_bytes,
                &derivation_bytes,
                &paid_bytes,
                &restore_bytes,
                &outputs,
                &split,
            ))?
        } else {
            serde_json::to_vec(&(
                1,
                id,
                &request_bytes,
                &derivation_bytes,
                &paid_bytes,
                &restore_bytes,
                &outputs,
            ))?
        };
        let report = MeltData {
            version: 1,
            intent_id: id,
            intent_digest: hex_digest(issued.intent.digest()?),
            quote_id: quote.id,
            operation_evidence_digest: hex_digest(Sha256::digest(evidence).into()),
            sources: issued.intent.allocate_result(fee, change)?,
        };
        report.validate(&issued.intent)?;
        let change_parts =
            crate::melt_reconciliation::allocate_change_outputs(&issued, &report, &outputs)?;
        tx.execute(
            "INSERT INTO treasury_results(intent_id,report,paid_evidence) VALUES(?,?,?)",
            params![id.to_string(), serde_json::to_vec(&report)?, paid_bytes],
        )?;
        tx.execute(
            "INSERT INTO treasury_fee_completions(intent_id,completed_at,fee_sats)
             VALUES(?,MAX(unixepoch(),COALESCE((SELECT MAX(admitted_at) FROM treasury_execution_admissions),0),
                          COALESCE((SELECT MAX(completed_at) FROM treasury_fee_completions),0)),?)",
            params![id.to_string(), fee],
        )?;
        for (source, result) in issued.intent.sources.iter().zip(&report.sources) {
            tx.execute("INSERT INTO treasury_source_results(intent_id,acquisition_id,input_sats,change_sats) VALUES(?,?,?,?)",params![id.to_string(),source.acquisition_id.to_string(),result.input_sats,result.change_sats])?;
        }
        for (y, c, amount) in outputs {
            let already_acquired: bool = tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM owned_output_sources WHERE mint_id=? AND y=?)",
                params![binding.mint_id, y],
                |r| r.get(0),
            )?;
            ensure!(
                !already_acquired,
                "change output already belongs to an acquisition"
            );
            tx.execute("INSERT INTO treasury_change_outputs(mint_id,y,c,amount,intent_id) VALUES(?,?,?,?,?)",params![binding.mint_id,y,c,amount,id.to_string()])?;
        }
        for (y, acquisition, amount) in change_parts {
            tx.execute("INSERT INTO treasury_change_source_parts(mint_id,y,acquisition_id,amount) VALUES(?,?,?,?)",params![binding.mint_id,y,acquisition.to_string(),amount])?;
        }
        tx.execute(
            "UPDATE treasury_instructions SET state='reconciled' WHERE id=? AND state='submitted'",
            [id.to_string()],
        )?;
        tx.commit()?;
        Ok(report)
    }

    /// Original immutable report, including after its change has later moved.
    pub fn treasury_melt_report(
        &self,
        binding: &TreasuryBinding,
        id: Uuid,
    ) -> Result<lightning_goats_cashu_receipt::treasury::MeltData> {
        let issued = self.treasury_instruction(binding, id)?;
        let bytes: Vec<u8> = self.connection.query_row(
            "SELECT report FROM treasury_results WHERE intent_id=?",
            [id.to_string()],
            |r| r.get(0),
        )?;
        let report: lightning_goats_cashu_receipt::treasury::MeltData =
            serde_json::from_slice(&bytes)?;
        report.validate(&issued.intent)?;
        Ok(report)
    }
}

impl Journal {
    /// Read only original submitted/reconciled material. Expiry and disabled
    /// execution must not make recovery depend on creating a new invoice.
    pub fn treasury_recovery_context(
        &self,
        binding: &TreasuryBinding,
        id: Uuid,
    ) -> Result<crate::melt_reconciliation::RecoveryContext> {
        let issued = self.treasury_instruction(binding, id)?;
        let (quote_bytes,derivation_bytes,operation): (Vec<u8>,Vec<u8>,String) = self.connection.query_row(
            "SELECT q.quote_json,r.derivation,p.operation_id FROM treasury_instructions i JOIN treasury_quotes q ON q.intent_id=i.id JOIN treasury_melt_requests r ON r.intent_id=i.id JOIN treasury_prepared p ON p.intent_id=i.id WHERE i.id=? AND i.state IN ('submitted','reconciled') AND length(q.quote_json)<=? AND length(r.derivation)<=1048576",
            params![id.to_string(),MAX_TREASURY_BYTES], |r|Ok((r.get(0)?,r.get(1)?,r.get(2)?)))?;
        let quote: cdk::wallet::MeltQuote = serde_json::from_slice(&quote_bytes)?;
        let derivation: crate::melt_derivation::MeltDerivation =
            serde_json::from_slice(&derivation_bytes)?;
        ensure!(
            quote.id == derivation.quote_id
                && quote.mint_url.as_ref() == Some(&derivation.mint_url)
                && quote.request == issued.invoice
                && u64::from(quote.amount) == issued.intent.deposit_sats
                && quote.unit == cdk::nuts::CurrencyUnit::Sat
                && quote.payment_method == cdk::nuts::PaymentMethod::BOLT11
                && operation == derivation.cdk_operation.to_string(),
            "retained recovery context conflicts"
        );
        Ok(crate::melt_reconciliation::RecoveryContext {
            issued,
            quote,
            derivation,
        })
    }
}

impl Journal {
    /// Preserve the original paid observation and NUT-09 response separately.
    /// Recording restoration evidence does not release holds or prove ownership.
    pub fn record_treasury_restore(
        &mut self,
        binding: &TreasuryBinding,
        id: Uuid,
        paid: &cdk::nuts::MeltQuoteBolt11Response<String>,
        response: &cdk::nuts::RestoreResponse,
    ) -> Result<()> {
        let context = self.treasury_recovery_context(binding, id)?;
        crate::melt_reconciliation::validate_paid_quote(&context.issued, &context.quote, paid)?;
        ensure!(
            paid.change.is_none(),
            "paid quote already includes change evidence"
        );
        crate::melt_reconciliation::normalize_restored_change(&context.derivation, response)?;
        let paid_bytes = serde_json::to_vec(paid)?;
        let response_bytes = serde_json::to_vec(response)?;
        ensure!(
            paid_bytes.len() <= 1_048_576 && response_bytes.len() <= 1_048_576,
            "restore evidence exceeds bound"
        );
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let previous: Option<(Vec<u8>, Vec<u8>)> = tx
            .query_row(
                "SELECT paid,response FROM treasury_restore_evidence WHERE intent_id=?",
                [id.to_string()],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        if let Some((original_paid, original_response)) = previous {
            ensure!(
                original_paid == paid_bytes && original_response == response_bytes,
                "restore evidence conflicts with original"
            );
        } else {
            let state: String = tx.query_row(
                "SELECT state FROM treasury_instructions WHERE id=?",
                [id.to_string()],
                |r| r.get(0),
            )?;
            ensure!(state == "submitted", "melt is not awaiting restoration");
            tx.execute(
                "INSERT INTO treasury_restore_evidence(intent_id,paid,response) VALUES(?,?,?)",
                params![id.to_string(), paid_bytes, response_bytes],
            )?;
        }
        tx.commit()?;
        Ok(())
    }
}

impl Journal {
    pub fn completed_treasury_report(
        &self,
        binding: &TreasuryBinding,
        id: Uuid,
    ) -> Result<Option<lightning_goats_cashu_receipt::treasury::MeltData>> {
        self.treasury_instruction(binding, id)?;
        let state: String = self.connection.query_row(
            "SELECT state FROM treasury_instructions WHERE id=?",
            [id.to_string()],
            |r| r.get(0),
        )?;
        if state == "reconciled" {
            return Ok(Some(self.treasury_melt_report(binding, id)?));
        }
        Ok(None)
    }

    pub fn retained_treasury_restore(
        &self,
        binding: &TreasuryBinding,
        id: Uuid,
    ) -> Result<
        Option<(
            cdk::nuts::MeltQuoteBolt11Response<String>,
            cdk::nuts::RestoreResponse,
        )>,
    > {
        let context = self.treasury_recovery_context(binding, id)?;
        let bytes: Option<(Vec<u8>,Vec<u8>)> = self.connection.query_row("SELECT paid,response FROM treasury_restore_evidence WHERE intent_id=? AND length(paid)<=1048576 AND length(response)<=1048576",[id.to_string()],|r|Ok((r.get(0)?,r.get(1)?))).optional()?;
        bytes
            .map(|(paid, response)| {
                let paid = serde_json::from_slice(&paid)?;
                let response = serde_json::from_slice(&response)?;
                crate::melt_reconciliation::validate_paid_quote(
                    &context.issued,
                    &context.quote,
                    &paid,
                )?;
                crate::melt_reconciliation::normalize_restored_change(
                    &context.derivation,
                    &response,
                )?;
                Ok((paid, response))
            })
            .transpose()
    }
}

impl Journal {
    pub fn claim_treasury_quote_request(
        &mut self,
        binding: &TreasuryBinding,
        mint: &crate::mint_policy::MintPolicy,
        id: Uuid,
        url: &url::Url,
        request: &cdk::nuts::MeltQuoteBolt11Request,
        now: u64,
    ) -> Result<()> {
        let issued = self.treasury_instruction(binding, id)?;
        binding.validate_new_invoice(&issued, now)?;
        let mint_url: cdk::mint_url::MintUrl = mint.advertised_url.parse()?;
        ensure!(
            mint.id == binding.mint_id
                && mint.bolt11_exit_qualified
                && mint_url.join("v1/melt/quote/bolt11")? == *url
                && request.request.to_string() == issued.invoice
                && request.unit == cdk::nuts::CurrencyUnit::Sat
                && request.options.is_none(),
            "quote request changes frozen treasury authority"
        );
        let bytes = serde_json::to_vec(request)?;
        ensure!(
            bytes.len() <= MAX_TREASURY_BYTES,
            "quote request exceeds bound"
        );
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        super::treasury_budget::for_quote(&tx, &issued.intent)?;
        let ready:bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM treasury_instructions i WHERE i.id=?1 AND i.state='bound' AND (EXISTS(SELECT 1 FROM treasury_proof_selections p WHERE p.intent_id=i.id) OR EXISTS(SELECT 1 FROM treasury_split_reservations s WHERE s.intent_id=i.id))) AND NOT EXISTS(SELECT 1 FROM treasury_quotes WHERE intent_id=?2)",params![id.to_string(),id.to_string()],|r|r.get(0))?;
        ensure!(
            ready,
            "quote requires reserved proofs and no existing quote"
        );
        tx.execute(
            "INSERT INTO treasury_quote_requests(intent_id,destination,request) VALUES(?,?,?)",
            params![id.to_string(), url.as_str(), bytes],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Adopt only the original durable reply. No network request or spend is
    /// authorized here. Missing replies retain the original attempt unresolved.
    pub fn adopt_retained_treasury_quote(
        &mut self,
        binding: &TreasuryBinding,
        mint: &crate::mint_policy::MintPolicy,
        id: Uuid,
        now: u64,
    ) -> Result<Option<cdk::wallet::MeltQuote>> {
        let issued = self.treasury_instruction(binding, id)?;
        let state: String = self.connection.query_row(
            "SELECT state FROM treasury_instructions WHERE id=?",
            [id.to_string()],
            |r| r.get(0),
        )?;
        ensure!(
            state == "bound",
            "quote adoption cannot rewind prepared or submitted work"
        );
        let (destination, request, reply): (String, Vec<u8>, Option<Vec<u8>>) = self.connection.query_row(
            "SELECT destination,request,reply FROM treasury_quote_requests WHERE intent_id=? AND length(request)<=? AND (reply IS NULL OR length(reply)<=?)",
            params![id.to_string(), MAX_TREASURY_BYTES, MAX_TREASURY_BYTES],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )?;
        let mint_url: cdk::mint_url::MintUrl = mint.advertised_url.parse()?;
        ensure!(
            mint_url.join("v1/melt/quote/bolt11")?.as_str() == destination,
            "retained quote destination differs from configured mint"
        );
        let request: cdk::nuts::MeltQuoteBolt11Request = serde_json::from_slice(&request)?;
        ensure!(
            request.request.to_string() == issued.invoice
                && request.unit == cdk::nuts::CurrencyUnit::Sat
                && request.options.is_none(),
            "retained quote request changes authority"
        );
        let Some(reply) = reply else {
            return Ok(None);
        };
        let observed: cdk::nuts::MeltQuoteBolt11Response<String> = serde_json::from_slice(&reply)?;
        ensure!(
            observed
                .request
                .as_ref()
                .is_none_or(|invoice| invoice == &issued.invoice)
                && observed
                    .unit
                    .as_ref()
                    .is_none_or(|unit| *unit == cdk::nuts::CurrencyUnit::Sat)
                && observed.method == cdk::nuts::PaymentMethod::BOLT11
                && observed
                    .change
                    .as_ref()
                    .is_none_or(|change| change.is_empty()),
            "retained quote reply changes authority"
        );
        let quote = cdk::wallet::MeltQuote {
            id: observed.quote,
            mint_url: Some(mint.advertised_url.parse()?),
            unit: cdk::nuts::CurrencyUnit::Sat,
            amount: observed.amount,
            request: issued.invoice,
            fee_reserve: observed.fee_reserve,
            state: observed.state,
            expiry: observed.expiry,
            payment_proof: observed.payment_preimage,
            estimated_blocks: None,
            fee_index: None,
            payment_method: cdk::nuts::PaymentMethod::BOLT11,
            used_by_operation: None,
            version: 0,
        };
        self.bind_treasury_quote(binding, mint, id, &quote, now)?;
        Ok(Some(quote))
    }

    pub fn record_treasury_quote_reply(&mut self, id: Uuid, reply: &[u8]) -> Result<()> {
        ensure!(
            !reply.is_empty() && reply.len() <= MAX_TREASURY_BYTES,
            "invalid quote reply size"
        );
        let _: serde_json::Value = serde_json::from_slice(reply)?;
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let previous: Option<Vec<u8>> = tx.query_row(
            "SELECT reply FROM treasury_quote_requests WHERE intent_id=?",
            [id.to_string()],
            |r| r.get(0),
        )?;
        if let Some(previous) = previous {
            ensure!(previous == reply, "quote reply conflicts with original");
        } else {
            tx.execute(
                "UPDATE treasury_quote_requests SET reply=? WHERE intent_id=?",
                params![reply, id.to_string()],
            )?;
        }
        tx.commit()?;
        Ok(())
    }
}

impl Journal {
    /// Private authenticated core command; admission and status never spend funds.
    pub(crate) fn treasury_rpc(
        &mut self,
        binding: &TreasuryBinding,
        request: &lightning_goats_cashu_receipt::treasury_rpc::Request,
        accept_new: bool,
        now: u64,
    ) -> Result<lightning_goats_cashu_receipt::treasury_rpc::Reply> {
        use lightning_goats_cashu_receipt::treasury_rpc::{
            Command, FEE_TIME_VERSION, Phase, Reply,
        };
        request.validate()?;
        let intent = request.intent();
        if let Command::Retain(issued) = &request.command {
            if accept_new {
                self.bind_treasury_instruction(binding, issued, now)?;
            } else {
                // A paused listener can acknowledge only exact retained work.
                let retained = self.treasury_instruction(binding, intent.id)?;
                ensure!(
                    &retained == issued.as_ref(),
                    "treasury admission is disabled or conflicts"
                );
            }
        }
        let retained = self.treasury_instruction(binding, intent.id)?;
        ensure!(
            retained.intent == *intent,
            "treasury inspection changes original scope"
        );
        let state: String = self.connection.query_row(
            "SELECT state FROM treasury_instructions WHERE id=?",
            [intent.id.to_string()],
            |r| r.get(0),
        )?;
        let phase = match state.as_str() {
            "bound" => Phase::Bound,
            "prepared" => Phase::Prepared,
            "submitted" => Phase::Submitted,
            "reconciled" => Phase::Reconciled,
            _ => anyhow::bail!("invalid treasury state"),
        };
        let report = self.completed_treasury_report(binding, intent.id)?;
        let fee_recorded_at = if report.is_some() && request.version == FEE_TIME_VERSION {
            Some(self.connection.query_row(
                "SELECT completed_at FROM treasury_fee_completions WHERE intent_id=?",
                [intent.id.to_string()],
                |row| row.get(0),
            )?)
        } else {
            None
        };
        let reply = Reply {
            version: request.version,
            request_id: request.request_id,
            intent_id: intent.id,
            intent_digest: lightning_goats_cashu_receipt::treasury::hex_digest(intent.digest()?),
            phase,
            report,
            fee_recorded_at,
        };
        reply.validate_for(request)?;
        Ok(reply)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_holds_match_owned_receipts_and_survive_restart_without_double_reservation() {
        let (binding, mut issued) = crate::treasury::tests::fixture();
        let dir = crate::test_directory().unwrap();
        let path = dir.path().join("wallet.sqlite");
        let mut journal = Journal::open(&path, binding.wallet_epoch, 128).unwrap();
        let acquisition = issued.intent.sources[0].acquisition_id;
        // Synthetic already-owned source; production reaches this state only
        // through cryptographic owned-output verification and receipt export.
        journal.connection.execute("INSERT INTO acquisitions(id,mint_id,policy,transport,source_ref,proof_count,gross,state,receive_fee,net,acquired_evidence) VALUES(?,?,'receive-v1','cashu','fixture',1,101,'credit_export_pending',0,101,?)",
            params![acquisition.to_string(),binding.mint_id,vec![7u8;32]]).unwrap();
        journal
            .connection
            .execute(
                "INSERT INTO acquisition_context(acquisition_id,proofs) VALUES(?,?)",
                params![acquisition.to_string(), b"[]".as_slice()],
            )
            .unwrap();
        let receipt = journal.export_receipt(acquisition).unwrap();
        issued.intent.sources[0].receipt_digest =
            lightning_goats_cashu_receipt::treasury::hex_digest(
                crate::receipt_wire::from_private(binding.wallet_instance, &receipt)
                    .unwrap()
                    .digest()
                    .unwrap(),
            );
        journal
            .bind_treasury_instruction(&binding, &issued, 1000)
            .unwrap();
        journal
            .reserve_treasury_sources(&binding, issued.intent.id, 1000)
            .unwrap();
        let mut ys = vec![
            cdk::dhke::hash_to_curve(b"owned-one").unwrap().to_hex(),
            cdk::dhke::hash_to_curve(b"owned-two").unwrap().to_hex(),
        ];
        ys.sort();
        for (y, amount) in ys.iter().zip([100u64, 1]) {
            journal.connection.execute("INSERT INTO owned_output_sources(mint_id,y,c,amount,acquisition_id) VALUES(?,?,?,?,?)",
                params![binding.mint_id,y,y,amount,acquisition.to_string()]).unwrap();
        }
        let incomplete = BTreeMap::from([(acquisition, vec![ys[0].clone()])]);
        assert!(
            journal
                .reserve_treasury_proof_ids(&binding, issued.intent.id, &incomplete, 1000)
                .is_err()
        );
        let count: i64 = journal
            .connection
            .query_row("SELECT COUNT(*) FROM treasury_proof_holds", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(count, 0); // late conservation failure rolls back earlier inserts
        let selected = BTreeMap::from([(acquisition, ys)]);
        journal
            .reserve_treasury_proof_ids(&binding, issued.intent.id, &selected, 1000)
            .unwrap();
        let mint = crate::mint_policy::MintPolicy {
            id: binding.mint_id.clone(),
            advertised_url: "https://mint.example".into(),
            policy_version: "receive-v1".into(),
            receive_cashu: true,
            receive_nutzap: true,
            bolt11_exit_qualified: true,
        };
        let quote = cdk::wallet::MeltQuote {
            id: "original-quote".into(),
            mint_url: Some(mint.advertised_url.parse().unwrap()),
            unit: cdk::nuts::CurrencyUnit::Sat,
            amount: 100u64.into(),
            request: issued.invoice.clone(),
            fee_reserve: 1u64.into(),
            state: cdk::nuts::MeltQuoteState::Unpaid,
            expiry: 1250,
            payment_proof: None,
            estimated_blocks: None,
            fee_index: None,
            payment_method: cdk::nuts::PaymentMethod::BOLT11,
            used_by_operation: None,
            version: 0,
        };
        for field in 0..5 {
            let mut bad = quote.clone();
            match field {
                0 => bad.fee_reserve = 2u64.into(),
                1 => bad.amount = 99u64.into(),
                2 => bad.mint_url = Some("https://other.example".parse().unwrap()),
                3 => bad.request = "changed invoice".into(),
                _ => bad.state = cdk::nuts::MeltQuoteState::Pending,
            }
            assert!(
                journal
                    .bind_treasury_quote(&binding, &mint, issued.intent.id, &bad, 1000)
                    .is_err()
            );
        }
        journal
            .bind_treasury_quote(&binding, &mint, issued.intent.id, &quote, 1000)
            .unwrap();
        drop(journal);
        let mut journal = Journal::open(&path, binding.wallet_epoch, 128).unwrap();
        journal
            .reserve_treasury_sources(&binding, issued.intent.id, 2000)
            .unwrap();
        journal
            .reserve_treasury_proof_ids(&binding, issued.intent.id, &selected, 2000)
            .unwrap();
        assert!(
            journal
                .reserve_treasury_proof_ids(&binding, issued.intent.id, &incomplete, 2000)
                .is_err()
        );
        journal
            .bind_treasury_quote(&binding, &mint, issued.intent.id, &quote, 2000)
            .unwrap();
        let mut replacement = quote.clone();
        replacement.id = "replacement-quote".into();
        assert!(
            journal
                .bind_treasury_quote(&binding, &mint, issued.intent.id, &replacement, 2000)
                .is_err()
        );
        let original = journal
            .treasury_instruction(&binding, issued.intent.id)
            .unwrap();
        assert_eq!(
            serde_json::to_vec(&original).unwrap(),
            serde_json::to_vec(&issued).unwrap()
        );
        let mut second = issued.clone();
        second.intent.id = Uuid::from_u128(91);
        second.receive_request_id = Uuid::from_u128(92);
        second.payment_hash = "ff".repeat(32);
        second.invoice = crate::treasury::tests::signed_invoice_hash(
            cdk::lightning_invoice::Currency::Bitcoin,
            100_000,
            1000,
            300,
            "ff",
        );
        journal
            .bind_treasury_instruction(&binding, &second, 1000)
            .unwrap();
        assert!(
            journal
                .reserve_treasury_sources(&binding, second.intent.id, 1000)
                .is_err()
        );
        let held: i64 = journal
            .connection
            .query_row(
                "SELECT SUM(input_sats) FROM treasury_source_holds",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(held, 101);
    }
    #[tokio::test]
    async fn wallet_report_commits_atomically_and_replays_without_releasing_sources() -> Result<()>
    {
        report_recovery_case(false).await
    }

    #[tokio::test]
    async fn nonzero_change_survives_wallet_persistence_before_journal_failure() -> Result<()> {
        report_recovery_case(true).await
    }

    async fn report_recovery_case(nonzero: bool) -> Result<()> {
        let (binding, _) = crate::treasury::tests::fixture();
        let (mut issued, quote, paid) = crate::melt_reconciliation::tests::fixture();
        if nonzero {
            issued.intent.input_sats = 103;
            issued.intent.sources[0].input_sats = 103;
        }
        let maximum_change = if nonzero { 3u64 } else { 1 };
        let denomination = if nonzero { 2u64 } else { 1 };
        let dir = crate::test_directory()?;
        let path = dir.path().join("report.sqlite");
        let mut journal = Journal::open(&path, binding.wallet_epoch, 128)?;
        journal.bind_treasury_instruction(&binding, &issued, 1000)?;
        // Seed the already-submitted boundary. Separate real-CDK tests qualify
        // preparation and transport; this fixture exercises atomic accounting.
        journal.connection.execute(
            "INSERT INTO treasury_prepared(intent_id,operation_id,prepared_json) VALUES(?,?,?)",
            params![
                issued.intent.id.to_string(),
                Uuid::from_u128(900).to_string(),
                b"{}".as_slice()
            ],
        )?;
        journal.connection.execute(
            "INSERT INTO treasury_quotes(intent_id,mint_id,quote_id,quote_json) VALUES(?,?,?,?)",
            params![
                issued.intent.id.to_string(),
                binding.mint_id,
                quote.id,
                serde_json::to_vec(&quote)?
            ],
        )?;
        let seed = [42; 64];
        let key = cdk::nuts::SecretKey::from_slice(&[7; 32])?;
        let keys = cdk::nuts::Keys::new(BTreeMap::from([(denomination.into(), key.public_key())]));
        let keyset = cdk::nuts::Id::v1_from_keys(&keys);
        let blank =
            cdk::nuts::PreMintSecrets::from_seed_blank(keyset, 0, &seed, maximum_change.into())?;
        let derivation = crate::melt_derivation::MeltDerivation {
            cdk_operation: Uuid::from_u128(900),
            mint_url: quote.mint_url.clone().unwrap(),
            quote_id: quote.id.clone(),
            counter_start: 0,
            counter_end: blank.secrets.len() as u32,
            maximum_change_sats: maximum_change,
            outputs: blank.blinded_messages(),
        };
        journal.connection.execute(
            "INSERT INTO treasury_melt_requests(intent_id,request,derivation) VALUES(?,?,?)",
            params![
                issued.intent.id.to_string(),
                b"{}".as_slice(),
                serde_json::to_vec(&derivation)?
            ],
        )?;
        journal.connection.execute(
            "UPDATE treasury_instructions SET state='submitted' WHERE id=?",
            [issued.intent.id.to_string()],
        )?;
        let recovery = journal.treasury_recovery_context(&binding, issued.intent.id)?;
        assert_eq!(
            recovery.quote_status_url()?.as_str(),
            "https://mint.example/v1/melt/quote/bolt11/original"
        );
        assert_eq!(recovery.issued.invoice, issued.invoice);
        let mut pinned = crate::intake::PinnedKeys::default();
        pinned.insert(&binding.mint_id, keyset, denomination, key.public_key())?;
        let states = cdk::nuts::CheckStateResponse { states: vec![] };
        assert!(
            journal
                .reconcile_treasury_melt(
                    &binding,
                    issued.intent.id,
                    &paid,
                    &seed,
                    &pinned,
                    &[],
                    &states
                )
                .is_err()
        );
        let mut restored = cdk::nuts::RestoreResponse {
            outputs: vec![],
            signatures: vec![],
        };
        if nonzero {
            let output = derivation.outputs[0].clone();
            let c = cdk::dhke::sign_message(&key, &output.blinded_secret)?;
            let signature = cdk::nuts::BlindSignature::new(
                denomination.into(),
                c,
                keyset,
                &output.blinded_secret,
                &key,
            )?;
            restored.outputs.push(output);
            restored.signatures.push(signature);
        }
        journal.record_treasury_restore(&binding, issued.intent.id, &paid, &restored)?;
        journal.record_treasury_restore(&binding, issued.intent.id, &paid, &restored)?;
        // Raw paid observation remains change=None; explicit restore supplies
        // the complete empty change result, yielding one sat actual fee.
        drop(journal);
        let mut journal = Journal::open(&path, binding.wallet_epoch, 128)?;
        journal.connection.execute_batch("CREATE TRIGGER refuse_reconcile BEFORE UPDATE OF state ON treasury_instructions WHEN NEW.state='reconciled' BEGIN SELECT RAISE(ABORT,'synthetic late failure'); END;")?;
        assert!(
            journal
                .reconcile_treasury_melt(
                    &binding,
                    issued.intent.id,
                    &paid,
                    &seed,
                    &pinned,
                    &[],
                    &states
                )
                .is_err()
        );
        let count: i64 =
            journal
                .connection
                .query_row("SELECT COUNT(*) FROM treasury_results", [], |r| r.get(0))?;
        assert_eq!(count, 0);
        if !nonzero {
            journal
                .connection
                .execute_batch("DROP TRIGGER refuse_reconcile;")?;
        }
        let candidates = crate::melt_reconciliation::reconstruct_change(
            &derivation,
            &seed,
            &binding.mint_id,
            &pinned,
            &restored.signatures,
        )?;
        let fresh = cdk::nuts::CheckStateResponse {
            states: candidates
                .iter()
                .map(|info| (info.y, cdk::nuts::State::Unspent).into())
                .collect(),
        };
        let journal = std::sync::Arc::new(std::sync::Mutex::new(journal));
        let writer = std::sync::Arc::new(crate::writer_lock::WriterLock::acquire(dir.path())?);
        let transport = crate::melt_reconciliation::tests::QuoteTransport {
            response: paid.clone(),
            calls: Default::default(),
            states: Some(fresh),
            restore: None,
        };
        let recovery = crate::treasury_recovery::Recovery {
            transport: &transport,
            journal: journal.clone(),
            database: crate::wallet::open_wallet_storage(dir.path()).await?,
            writer: writer.clone(),
            binding: &binding,
            keys: &pinned,
            seed: &seed,
        };
        *writer.coordinator().lock().unwrap() = Some(crate::writer_lock::Operation {
            id: Uuid::new_v4(),
            authority: crate::writer_lock::Authority::TreasuryRecovery,
        });
        assert!(recovery.reconcile(issued.intent.id).await.is_err());
        assert_eq!(transport.calls.load(std::sync::atomic::Ordering::SeqCst), 0);
        *writer.coordinator().lock().unwrap() = None;
        if nonzero {
            assert!(recovery.reconcile(issued.intent.id).await.is_err());
            let stored = recovery
                .database
                .get_proofs_by_ys(candidates.iter().map(|info| info.y).collect())
                .await?;
            assert_eq!(stored.len(), 1);
            assert_eq!(u64::from(stored[0].proof.amount), 2);
            {
                let mut locked = journal.lock().unwrap();
                let count: i64 = locked.connection.query_row(
                    "SELECT COUNT(*) FROM treasury_results",
                    [],
                    |r| r.get(0),
                )?;
                assert_eq!(count, 0);
                locked
                    .connection
                    .execute_batch("DROP TRIGGER refuse_reconcile;")?;
                *locked = Journal::open(&path, binding.wallet_epoch, 128)?;
            }
        }
        let recovery = crate::treasury_recovery::Recovery {
            database: crate::wallet::open_wallet_storage(dir.path()).await?,
            ..recovery
        };
        let report = recovery.reconcile(issued.intent.id).await?.unwrap();
        let replay = recovery.reconcile(issued.intent.id).await?.unwrap();
        assert_eq!(serde_json::to_vec(&report)?, serde_json::to_vec(&replay)?);
        assert_eq!(
            transport.calls.load(std::sync::atomic::Ordering::SeqCst),
            if nonzero { 4 } else { 1 }
        );
        drop(recovery);
        if nonzero {
            let mut locked = journal.lock().unwrap();
            let acquisition = issued.intent.sources[0].acquisition_id;
            locked.connection.execute("INSERT INTO acquisitions(id,mint_id,policy,transport,source_ref,proof_count,gross,state,receive_fee,net,acquired_evidence) VALUES(?,?,'receive-v1','cashu','original-source',1,103,'credited',0,103,?)",params![acquisition.to_string(),binding.mint_id,vec![7u8;32]])?;
            locked.connection.execute(
                "INSERT INTO acquisition_context(acquisition_id,proofs) VALUES(?,?)",
                params![acquisition.to_string(), b"[]".as_slice()],
            )?;
            locked.connection.execute("INSERT INTO treasury_source_holds(intent_id,acquisition_id,input_sats) VALUES(?,?,103)",params![issued.intent.id.to_string(),acquisition.to_string()])?;
            let limits: custody::CustodyLimits = serde_json::from_value(serde_json::json!({
                "maximum_total_sats":103,"maximum_per_mint_sats":{(binding.mint_id.clone()):103}
            }))?;
            limits.reserve(&locked.connection, &binding.mint_id, 101)?;
            assert_eq!(
                limits
                    .reserve(&locked.connection, &binding.mint_id, 102)
                    .unwrap_err()
                    .downcast_ref(),
                Some(&custody::AdmissionCapacity::Full),
            );
            let receipt = locked.export_receipt(acquisition)?;
            let mut next = issued.clone();
            next.intent.id = Uuid::from_u128(901);
            next.receive_request_id = Uuid::from_u128(902);
            next.payment_hash = "ff".repeat(32);
            next.invoice = crate::treasury::tests::signed_invoice_hash(
                cdk::lightning_invoice::Currency::Bitcoin,
                1000,
                1000,
                300,
                "ff",
            );
            next.intent.input_sats = 2;
            next.intent.deposit_sats = 1;
            next.intent.sources[0].input_sats = 2;
            next.intent.sources[0].receipt_digest =
                lightning_goats_cashu_receipt::treasury::hex_digest(
                    crate::receipt_wire::from_private(binding.wallet_instance, &receipt)?
                        .digest()?,
                );
            locked.bind_treasury_instruction(&binding, &next, 1000)?;
            locked.reserve_treasury_sources(&binding, next.intent.id, 1000)?;
            let selected = BTreeMap::from([(acquisition, vec![candidates[0].y.to_hex()])]);
            locked.reserve_treasury_proof_ids(&binding, next.intent.id, &selected, 1000)?;
            locked.reserve_treasury_proof_ids(&binding, next.intent.id, &selected, 2000)?;
            let mut excess = next.clone();
            excess.intent.id = Uuid::from_u128(903);
            excess.receive_request_id = Uuid::from_u128(904);
            excess.payment_hash = "ee".repeat(32);
            excess.invoice = crate::treasury::tests::signed_invoice_hash(
                cdk::lightning_invoice::Currency::Bitcoin,
                1000,
                1000,
                300,
                "ee",
            );
            locked.bind_treasury_instruction(&binding, &excess, 1000)?;
            assert!(
                locked
                    .reserve_treasury_sources(&binding, excess.intent.id, 1000)
                    .is_err()
            );
        }
        let attributed: i64 = journal.lock().unwrap().connection.query_row(
            "SELECT COALESCE(SUM(amount),0) FROM treasury_change_source_parts",
            [],
            |r| r.get(0),
        )?;
        assert_eq!(attributed, if nonzero { 2 } else { 0 });
        assert_eq!(report.sources[0].deposit_sats, 100);
        assert_eq!(report.sources[0].fee_sats, 1);
        assert_eq!(report.sources[0].change_sats, if nonzero { 2 } else { 0 });
        drop(journal);
        let mut journal = Journal::open(&path, binding.wallet_epoch, 128)?;
        let (time, fee): (i64, u64) = journal.connection.query_row(
            "SELECT completed_at,fee_sats FROM treasury_fee_completions WHERE intent_id=?",
            [issued.intent.id.to_string()],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        assert_eq!(fee, 1);
        use lightning_goats_cashu_receipt::treasury_rpc::{Command, FEE_TIME_VERSION, Request};
        let mut query = Request {
            version: 1,
            request_id: Uuid::new_v4(),
            command: Command::Status(Box::new(issued.intent.clone())),
        };
        let old = journal.treasury_rpc(&binding, &query, false, 1000)?;
        assert!(serde_json::to_value(&old)?.get("fee_recorded_at").is_none());
        query.version = FEE_TIME_VERSION;
        let current = journal.treasury_rpc(&binding, &query, false, 1000)?;
        assert_eq!(current.fee_recorded_at, Some(time));
        assert!(old.report == current.report);
        assert_eq!(
            crate::journal::treasury_scheduling::daily_fees(&journal.connection, time + 86_400)?,
            1
        );
        assert_eq!(
            crate::journal::treasury_scheduling::daily_fees(&journal.connection, time + 86_401)?,
            0
        );
        let recovery = journal.treasury_recovery_context(&binding, issued.intent.id)?;
        assert_eq!(recovery.derivation.cdk_operation, Uuid::from_u128(900));
        assert_eq!(
            serde_json::to_vec(&report)?,
            serde_json::to_vec(&journal.treasury_melt_report(&binding, issued.intent.id)?)?
        );
        Ok(())
    }
    #[test]
    fn shared_change_proof_requires_all_sources_and_one_global_hold() -> Result<()> {
        let (binding, mut full) = crate::treasury::tests::fixture();
        full.intent.input_sats = 2;
        full.intent.deposit_sats = 1;
        full.intent.sources[0].input_sats = 1;
        full.invoice = crate::treasury::tests::signed_invoice_hash(
            cdk::lightning_invoice::Currency::Bitcoin,
            1000,
            1000,
            300,
            "ab",
        );
        let mut second = full.intent.sources[0].clone();
        second.ledger_id = 2;
        second.acquisition_id = Uuid::from_u128(99);
        full.intent.sources.push(second);
        let mut partial = full.clone();
        partial.intent.id = Uuid::from_u128(100);
        partial.receive_request_id = Uuid::from_u128(101);
        partial.payment_hash = "ff".repeat(32);
        partial.invoice = crate::treasury::tests::signed_invoice_hash(
            cdk::lightning_invoice::Currency::Bitcoin,
            1000,
            1000,
            300,
            "ff",
        );
        partial.intent.input_sats = 1;
        partial.intent.maximum_fee_sats = 0;
        partial.intent.sources.truncate(1);
        let mut origin = full.clone();
        origin.intent.id = Uuid::from_u128(102);
        origin.receive_request_id = Uuid::from_u128(103);
        origin.payment_hash = "ee".repeat(32);
        origin.invoice = crate::treasury::tests::signed_invoice_hash(
            cdk::lightning_invoice::Currency::Bitcoin,
            1000,
            1000,
            300,
            "ee",
        );
        let dir = crate::test_directory()?;
        let path = dir.path().join("shared.sqlite");
        // One physical proof, even though it is attributed to two sources.
        let mut journal = Journal::open(&path, binding.wallet_epoch, 1)?;
        for issued in [&origin, &full, &partial] {
            journal.bind_treasury_instruction(&binding, issued, 1000)?;
        }
        // Seed already reconciled custody and prior value reservations. Receipt
        // authentication/value limits are covered by the separate source tests.
        for source in &full.intent.sources {
            journal.connection.execute("INSERT INTO acquisitions(id,mint_id,policy,transport,source_ref,proof_count,gross,state,receive_fee,net,acquired_evidence) VALUES(?,?,'v1','cashu','fixture',1,1,'credited',0,1,?)",params![source.acquisition_id.to_string(),binding.mint_id,vec![7u8;32]])?;
        }
        for issued in [&full, &partial] {
            for source in &issued.intent.sources {
                journal.connection.execute("INSERT INTO treasury_source_holds(intent_id,acquisition_id,input_sats) VALUES(?,?,?)",params![issued.intent.id.to_string(),source.acquisition_id.to_string(),source.input_sats])?;
            }
        }
        journal.connection.execute(
            "INSERT INTO treasury_prepared(intent_id,operation_id,prepared_json) VALUES(?,?,?)",
            params![
                origin.intent.id.to_string(),
                Uuid::from_u128(104).to_string(),
                b"{}".as_slice()
            ],
        )?;
        journal.connection.execute(
            "INSERT INTO treasury_melt_requests(intent_id,request,derivation) VALUES(?,?,?)",
            params![
                origin.intent.id.to_string(),
                b"{}".as_slice(),
                b"{}".as_slice()
            ],
        )?;
        journal.connection.execute(
            "INSERT INTO treasury_results(intent_id,report,paid_evidence) VALUES(?,?,?)",
            params![
                origin.intent.id.to_string(),
                b"{}".as_slice(),
                b"{}".as_slice()
            ],
        )?;
        let y = cdk::dhke::hash_to_curve(b"shared-change")?.to_hex();
        journal.connection.execute(
            "INSERT INTO treasury_change_outputs(mint_id,y,c,amount,intent_id) VALUES(?,?,?,2,?)",
            params![binding.mint_id, y, y, origin.intent.id.to_string()],
        )?;
        for source in &full.intent.sources {
            journal.connection.execute("INSERT INTO treasury_change_source_parts(mint_id,y,acquisition_id,amount) VALUES(?,?,?,1)",params![binding.mint_id,y,source.acquisition_id.to_string()])?;
        }
        let incomplete =
            BTreeMap::from([(partial.intent.sources[0].acquisition_id, vec![y.clone()])]);
        assert!(
            journal
                .reserve_treasury_proof_ids(&binding, partial.intent.id, &incomplete, 1000)
                .is_err()
        );
        let holds: i64 =
            journal
                .connection
                .query_row("SELECT COUNT(*) FROM treasury_proof_holds", [], |r| {
                    r.get(0)
                })?;
        assert_eq!(holds, 0);
        let complete = full
            .intent
            .sources
            .iter()
            .map(|source| (source.acquisition_id, vec![y.clone()]))
            .collect();
        assert!(
            journal
                .select_treasury_proofs(&binding, partial.intent.id, 1000)
                .is_err()
        );
        assert_eq!(
            journal.select_treasury_proofs(&binding, full.intent.id, 1000)?,
            complete
        );
        journal.reserve_treasury_proof_ids(&binding, full.intent.id, &complete, 1000)?;
        let holds: i64 =
            journal
                .connection
                .query_row("SELECT COUNT(*) FROM treasury_proof_holds", [], |r| {
                    r.get(0)
                })?;
        assert_eq!(holds, 1);
        assert!(
            journal
                .reserve_treasury_proof_ids(&binding, partial.intent.id, &incomplete, 1000)
                .is_err()
        );
        drop(journal);
        let mut reopened = Journal::open(&path, binding.wallet_epoch, 1)?;
        reopened.reserve_treasury_proof_ids(&binding, full.intent.id, &complete, 2000)?;
        assert_eq!(
            reopened.select_treasury_proofs(&binding, full.intent.id, 2000)?,
            complete
        );
        Ok(())
    }
}
