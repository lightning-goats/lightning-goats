//! Atomic proof/provenance reservation for one original denomination split.
//! Reservation is not a grant to send a mint request. The original swap request,
//! seed derivation, exposure admission and recovery gates are still required.
use super::*;
use crate::{
    treasury::TreasuryBinding,
    treasury_split::{Limits, Plan},
};
use lightning_goats_cashu_receipt::treasury::MAX_SOURCES;
use serde::{Deserialize, Serialize};

mod admission;
mod selection;
mod submission;
pub use admission::AdmissionPolicy;

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub split_id: Uuid,
    pub input_ys: Vec<String>,
    pub fee_sats: u64,
    pub denominations: Vec<u64>,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    y: String,
    c: String,
    amount: u64,
    parts: BTreeMap<Uuid, u64>,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Reservation {
    request: Request,
    inputs: Vec<Input>,
    plan: Plan,
}
impl Reservation {
    pub(crate) fn input_ys(&self) -> Result<Vec<cdk::nuts::PublicKey>> {
        self.request
            .input_ys
            .iter()
            .map(|y| Ok(cdk::nuts::PublicKey::from_hex(y)?))
            .collect()
    }
    pub fn plan(&self) -> &Plan {
        &self.plan
    }
    pub fn split_id(&self) -> Uuid {
        self.request.split_id
    }
}

impl Journal {
    /// Freeze canonical input commitments and all their owners in the same
    /// transaction as global proof holds. A replay returns the retained plan;
    /// expiry never releases a hold or permits a replacement split UUID.
    pub fn reserve_treasury_split(
        &mut self,
        binding: &TreasuryBinding,
        id: Uuid,
        request: &Request,
        limits: Limits,
        now: u64,
    ) -> Result<Reservation> {
        let issued = self.treasury_instruction(binding, id)?;
        ensure!(
            !request.split_id.is_nil()
                && !request.input_ys.is_empty()
                && request.input_ys.len() <= self.max_proofs
                && request.input_ys.windows(2).all(|v| v[0] < v[1]),
            "invalid split identity or proof inventory"
        );
        for y in &request.input_ys {
            ensure!(
                cdk::nuts::PublicKey::from_hex(y)?.to_hex() == *y,
                "noncanonical split input Y"
            );
        }
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let previous: Option<Vec<u8>> = tx
            .query_row(
                "SELECT reservation FROM treasury_split_reservations WHERE intent_id=?",
                [id.to_string()],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(previous) = previous {
            ensure!(previous.len() <= 1_048_576, "oversized split reservation");
            let retained: Reservation = serde_json::from_slice(&previous)?;
            ensure!(
                retained.request == *request,
                "split reservation conflicts with original"
            );
            retained.plan.validate_for(&issued.intent, limits)?;
            tx.commit()?;
            return Ok(retained);
        }
        binding.validate_new_invoice(&issued, now)?;
        // No late reshaping of a selected, quoted or partially prepared melt.
        for table in [
            "treasury_proof_selections",
            "treasury_quote_requests",
            "treasury_quotes",
            "treasury_preparation_claims",
            "treasury_prepared",
            "treasury_proof_holds",
        ] {
            let present: bool = tx.query_row(
                &format!("SELECT EXISTS(SELECT 1 FROM {table} WHERE intent_id=?)"),
                [id.to_string()],
                |r| r.get(0),
            )?;
            ensure!(
                !present,
                "treasury operation already advanced beyond splitting"
            );
        }
        for source in &issued.intent.sources {
            let held: u64 = tx.query_row(
                "SELECT input_sats FROM treasury_source_holds WHERE intent_id=? AND acquisition_id=?",
                params![id.to_string(), source.acquisition_id.to_string()], |r| r.get(0))?;
            ensure!(
                held == source.input_sats,
                "split source value was not reserved"
            );
        }
        let mut inputs = Vec::new();
        let mut owned = BTreeMap::<Uuid, u64>::new();
        for y in &request.input_ys {
            let mut statement = tx.prepare(
                "SELECT c,amount FROM treasury_owned_outputs WHERE mint_id=? AND y=? LIMIT 2",
            )?;
            let rows = statement
                .query_map(params![binding.mint_id, y], |r| {
                    Ok((r.get::<_, String>(0)?, r.get::<_, u64>(1)?))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            ensure!(
                rows.len() == 1,
                "missing or ambiguous split input provenance"
            );
            let (c, amount) = &rows[0];
            ensure!(
                *amount > 0
                    && *amount <= i64::MAX as u64
                    && cdk::nuts::PublicKey::from_hex(c)?.to_hex() == *c,
                "invalid split input commitment"
            );
            let mut parts = BTreeMap::new();
            let mut total = 0u64;
            let mut statement = tx.prepare(
                "SELECT acquisition_id,amount FROM treasury_output_parts WHERE mint_id=? AND y=? LIMIT ?")?;
            let rows = statement.query_map(params![binding.mint_id, y, MAX_SOURCES + 1], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, u64>(1)?))
            })?;
            for row in rows {
                let (owner, value) = row?;
                let owner = Uuid::parse_str(&owner)?;
                ensure!(
                    !owner.is_nil()
                        && value > 0
                        && parts.insert(owner, value).is_none()
                        && parts.len() <= MAX_SOURCES,
                    "invalid split source part"
                );
                total = total
                    .checked_add(value)
                    .ok_or_else(|| anyhow::anyhow!("split value overflow"))?;
                let balance = owned.entry(owner).or_default();
                *balance = balance
                    .checked_add(value)
                    .ok_or_else(|| anyhow::anyhow!("split owner overflow"))?;
                ensure!(owned.len() <= MAX_SOURCES, "too many split owners");
            }
            ensure!(total == *amount, "split parts do not conserve proof value");
            // Plain INSERT rejects another intent's claim and any unexpected
            // same-intent claim; all earlier inserts roll back on conflict.
            tx.execute(
                "INSERT INTO treasury_proof_holds(mint_id,y,intent_id) VALUES(?,?,?)",
                params![binding.mint_id, y, id.to_string()],
            )?;
            inputs.push(Input {
                y: y.clone(),
                c: c.clone(),
                amount: *amount,
                parts,
            });
        }
        let plan = Plan::build(
            &issued.intent,
            owned,
            request.fee_sats,
            request.denominations.clone(),
            limits,
        )?;
        let retained = Reservation {
            request: request.clone(),
            inputs,
            plan,
        };
        let bytes = serde_json::to_vec(&retained)?;
        ensure!(bytes.len() <= 1_048_576, "split reservation exceeds bound");
        tx.execute(
            "INSERT INTO treasury_split_reservations(intent_id,split_id,reservation) VALUES(?,?,?)",
            params![id.to_string(), request.split_id.to_string(), bytes],
        )?;
        tx.commit()?;
        Ok(retained)
    }
}
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Assignment {
    blinded: String,
    y: String,
    owner: Uuid,
    amount: u64,
    purpose: crate::treasury_split::Purpose,
}

/// Original request and seed-derived owner mapping. Contains spendable proof
/// secrets; keep private and never log or expose through public status.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Frozen {
    intent_digest: String,
    split_id: Uuid,
    request: Vec<u8>,
    derivation: crate::ownership::SwapDerivation,
    assignments: Vec<Assignment>,
}

impl Journal {
    /// Retain exact transport bytes and the CDK operation before submission.
    /// This does not claim submission or bypass exposure/fee/crypto validation.
    #[allow(clippy::too_many_arguments)]
    pub fn freeze_treasury_split_request(
        &mut self,
        binding: &TreasuryBinding,
        mint: &crate::mint_policy::MintPolicy,
        id: Uuid,
        bytes: &[u8],
        saga: &cdk::wallet::types::WalletSaga,
        seed: &[u8; 64],
        limits: Limits,
        now: u64,
    ) -> Result<Frozen> {
        ensure!(bytes.len() <= 1_048_576, "split request exceeds bound");
        ensure!(
            mint.id == binding.mint_id && mint.bolt11_exit_qualified,
            "split mint is not qualified"
        );
        let issued = self.treasury_instruction(binding, id)?;
        let request: cdk::nuts::SwapRequest = serde_json::from_slice(bytes)?;
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let raw:Vec<u8> = tx.query_row(
            "SELECT reservation FROM treasury_split_reservations WHERE intent_id=? AND length(reservation)<=1048576",
            [id.to_string()],|r|r.get(0))?;
        let reserved: Reservation = serde_json::from_slice(&raw)?;
        reserved.plan.validate_for(&issued.intent, limits)?;
        let derivation = crate::ownership::SwapDerivation::capture_split(
            saga,
            &request,
            reserved.request.fee_sats,
        )?;
        ensure!(
            derivation.mint_url == mint.advertised_url.parse()?,
            "split mint changed"
        );
        ensure!(
            request.inputs().len() == reserved.inputs.len(),
            "split input set changed"
        );
        let mut seen = std::collections::BTreeSet::new();
        for proof in request.inputs() {
            let y = proof.y()?.to_hex();
            ensure!(seen.insert(y.clone()), "duplicate split input");
            let input = reserved
                .inputs
                .iter()
                .find(|input| input.y == y)
                .ok_or_else(|| anyhow::anyhow!("split input was not reserved"))?;
            ensure!(
                input.c == proof.c.to_hex() && input.amount == u64::from(proof.amount),
                "split input differs from frozen commitment"
            );
            let owner: String = tx.query_row(
                "SELECT intent_id FROM treasury_proof_holds WHERE mint_id=? AND y=?",
                params![binding.mint_id, y],
                |r| r.get(0),
            )?;
            ensure!(owner == id.to_string(), "split proof hold changed");
        }
        let ys = derivation.output_ys(seed)?;
        let mut outputs: Vec<_> = request.outputs().iter().zip(ys).collect();
        outputs.sort_by_key(|(output, _)| {
            (u64::from(output.amount), output.blinded_secret.to_bytes())
        });
        ensure!(
            outputs.len() == reserved.plan.outputs().len(),
            "split output count changed"
        );
        let mut assignments = Vec::new();
        for ((output, y), planned) in outputs.into_iter().zip(reserved.plan.outputs()) {
            ensure!(
                u64::from(output.amount) == planned.amount,
                "split denominations changed"
            );
            assignments.push(Assignment {
                blinded: output.blinded_secret.to_hex(),
                y: y.to_hex(),
                owner: planned.acquisition_id,
                amount: planned.amount,
                purpose: planned.purpose,
            });
        }
        let frozen = Frozen {
            intent_digest: lightning_goats_cashu_receipt::treasury::hex_digest(
                issued.intent.digest()?,
            ),
            split_id: reserved.request.split_id,
            request: bytes.to_vec(),
            derivation,
            assignments,
        };
        let previous: Option<Vec<u8>> = tx
            .query_row(
                "SELECT frozen FROM treasury_split_requests WHERE intent_id=?",
                [id.to_string()],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(previous) = previous {
            ensure!(previous.len() <= 2_097_152, "oversized frozen split");
            let previous: Frozen = serde_json::from_slice(&previous)?;
            ensure!(
                previous == frozen,
                "split request or original CDK operation changed"
            );
        } else {
            binding.validate_new_invoice(&issued, now)?;
            let quote_bytes:Vec<u8>=tx.query_row("SELECT quote_json FROM treasury_quotes WHERE intent_id=? AND length(quote_json)<=65536",
                [id.to_string()],|r|r.get(0))?;
            let quote: cdk::wallet::MeltQuote = serde_json::from_slice(&quote_bytes)?;
            let fee_cap = issued
                .intent
                .maximum_fee_sats
                .checked_sub(reserved.request.fee_sats)
                .ok_or_else(|| anyhow::anyhow!("split exhausted fee authority"))?;
            ensure!(
                quote.expiry > now
                    && quote.request == issued.invoice
                    && quote.mint_url.as_ref() == Some(&frozen.derivation.mint_url)
                    && u64::from(quote.amount) == issued.intent.deposit_sats
                    && u64::from(quote.fee_reserve) <= fee_cap
                    && issued
                        .intent
                        .deposit_sats
                        .checked_add(reserved.request.fee_sats)
                        .and_then(|n| n.checked_add(u64::from(quote.fee_reserve)))
                        .is_some_and(|n| n <= issued.intent.input_sats),
                "split requires an affordable original unexpired quote"
            );

            let encoded = serde_json::to_vec(&frozen)?;
            ensure!(encoded.len() <= 2_097_152, "frozen split exceeds bound");
            tx.execute(
                "INSERT INTO treasury_split_requests(intent_id,cdk_operation,frozen) VALUES(?,?,?)",
                params![id.to_string(), saga.id.to_string(), encoded],
            )?;
        }
        tx.commit()?;
        Ok(frozen)
    }
}
/// A verified output keeps the original source owner and destination purpose.
/// This data contains no proof secret or private signing material.
#[derive(Serialize)]
pub struct VerifiedOutput {
    pub y: String,
    pub c: String,
    pub owner: Uuid,
    pub amount: u64,
    pub purpose: crate::treasury_split::Purpose,
}
/// Sealed reconciliation evidence. Only exact seed/DLEQ/mint-state verification
/// constructs this result; a CDK balance or completion label cannot do so.
pub struct VerifiedSplit {
    intent_id: Uuid,
    frozen_digest: [u8; 32],
    fee_sats: u64,
    input_sats: u64,
    outputs: Vec<VerifiedOutput>,
}
impl VerifiedSplit {
    pub fn intent_id(&self) -> Uuid {
        self.intent_id
    }
    pub fn frozen_digest(&self) -> &[u8; 32] {
        &self.frozen_digest
    }
    pub fn fee_sats(&self) -> u64 {
        self.fee_sats
    }
    pub fn input_sats(&self) -> u64 {
        self.input_sats
    }
    pub fn outputs(&self) -> &[VerifiedOutput] {
        &self.outputs
    }
}
impl Journal {
    /// Read-only original-operation reconciliation. A separate atomic result
    /// commit must consume this evidence before any new outputs can be selected.
    #[allow(clippy::too_many_arguments)]
    pub fn verify_treasury_split_outputs(
        &self,
        binding: &TreasuryBinding,
        id: Uuid,
        seed: &[u8; 64],
        keys: &crate::intake::PinnedKeys,
        owned: &[cdk::wallet::types::ProofInfo],
        fresh_inputs: &cdk::nuts::CheckStateResponse,
        fresh_outputs: &cdk::nuts::CheckStateResponse,
    ) -> Result<VerifiedSplit> {
        use cdk::nuts::State;
        let issued = self.treasury_instruction(binding, id)?;
        let submitted: Vec<u8> = self.connection.query_row(
            "SELECT request_digest FROM treasury_split_submissions WHERE intent_id=?",
            [id.to_string()],
            |r| r.get(0),
        )?;
        let raw:Vec<u8>=self.connection.query_row(
            "SELECT frozen FROM treasury_split_requests WHERE intent_id=? AND length(frozen)<=2097152",
            [id.to_string()],|r|r.get(0))?;
        let frozen: Frozen = serde_json::from_slice(&raw)?;
        ensure!(
            submitted == Sha256::digest(&frozen.request).to_vec(),
            "split submission request binding changed"
        );
        ensure!(
            frozen.intent_digest
                == lightning_goats_cashu_receipt::treasury::hex_digest(issued.intent.digest()?),
            "split reconciliation scope changed"
        );
        let reservation:Vec<u8>=self.connection.query_row(
            "SELECT reservation FROM treasury_split_reservations WHERE intent_id=? AND length(reservation)<=1048576",
            [id.to_string()],|r|r.get(0))?;
        let reserved: Reservation = serde_json::from_slice(&reservation)?;
        ensure!(
            frozen.split_id == reserved.request.split_id,
            "split identity changed"
        );
        let request: cdk::nuts::SwapRequest = serde_json::from_slice(&frozen.request)?;
        ensure!(
            request.outputs() == &frozen.derivation.outputs,
            "split output binding changed"
        );
        let expected_inputs: std::collections::BTreeSet<_> =
            reserved.inputs.iter().map(|p| p.y.as_str()).collect();
        let mut inputs = BTreeMap::new();
        for state in &fresh_inputs.states {
            ensure!(
                inputs.insert(state.y.to_hex(), state.state).is_none(),
                "duplicate split input state"
            );
        }
        ensure!(
            inputs.len() == expected_inputs.len()
                && expected_inputs
                    .iter()
                    .all(|y| inputs.get(*y) == Some(&State::Spent)),
            "original split inputs are not authoritatively spent"
        );
        let verified = crate::ownership::verify_derived_outputs(
            &frozen.derivation,
            seed,
            &binding.mint_id,
            keys,
            owned,
            fresh_outputs,
        )?;
        ensure!(
            verified.len() == frozen.assignments.len(),
            "split ownership mapping changed"
        );
        let mut assigned = std::collections::BTreeSet::new();
        let mut outputs = Vec::new();
        let mut output_sats = 0u64;
        for (y, c, amount) in verified {
            ensure!(
                !expected_inputs.contains(y.as_str()) && assigned.insert(y.clone()),
                "split output reuses an input identity"
            );
            let matching: Vec<_> = frozen.assignments.iter().filter(|a| a.y == y).collect();
            ensure!(
                matching.len() == 1 && matching[0].amount == amount,
                "split output attribution changed"
            );
            let assignment = matching[0];
            output_sats = output_sats
                .checked_add(amount)
                .ok_or_else(|| anyhow::anyhow!("split output overflow"))?;
            outputs.push(VerifiedOutput {
                y,
                c,
                owner: assignment.owner,
                amount,
                purpose: assignment.purpose,
            });
        }
        let input_sats = reserved
            .inputs
            .iter()
            .try_fold(0u64, |sum, p| sum.checked_add(p.amount))
            .ok_or_else(|| anyhow::anyhow!("split input overflow"))?;
        ensure!(
            output_sats.checked_add(reserved.request.fee_sats) == Some(input_sats)
                && input_sats == u64::from(request.input_amount()?),
            "split value or fee changed"
        );
        Ok(VerifiedSplit {
            intent_id: id,
            frozen_digest: Sha256::digest(&raw).into(),
            fee_sats: reserved.request.fee_sats,
            input_sats,
            outputs,
        })
    }
}
impl VerifiedSplit {
    /// Consume cryptographically verified evidence in one custody transaction.
    /// Spent inputs remain globally held; only unrelated remainder becomes free.
    /// This does not grant donor credit, settle the later melt or authorize feeding.
    pub fn commit(self, journal: &mut Journal, binding: &TreasuryBinding) -> Result<bool> {
        let issued = journal.treasury_instruction(binding, self.intent_id)?;
        ensure!(
            self.fee_sats <= issued.intent.maximum_fee_sats,
            "split fee exceeds original authority"
        );
        let tx = journal
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let raw:Vec<u8>=tx.query_row(
            "SELECT frozen FROM treasury_split_requests WHERE intent_id=? AND length(frozen)<=2097152",
            [self.intent_id.to_string()],|r|r.get(0))?;
        ensure!(
            <[u8; 32]>::from(Sha256::digest(&raw)) == self.frozen_digest,
            "verified split request changed before commit"
        );
        let encoded = serde_json::to_vec(&(
            1,
            self.intent_id,
            self.frozen_digest,
            self.input_sats,
            self.fee_sats,
            &self.outputs,
        ))?;
        ensure!(encoded.len() <= 1_048_576, "split result exceeds bound");
        let previous: Option<Vec<u8>> = tx
            .query_row(
                "SELECT result FROM treasury_split_results WHERE intent_id=?",
                [self.intent_id.to_string()],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(previous) = previous {
            ensure!(previous == encoded, "split result conflicts with original");
            tx.commit()?;
            return Ok(false);
        }
        let reservation:Vec<u8>=tx.query_row(
            "SELECT reservation FROM treasury_split_reservations WHERE intent_id=? AND length(reservation)<=1048576",
            [self.intent_id.to_string()],|r|r.get(0))?;
        let reserved: Reservation = serde_json::from_slice(&reservation)?;
        ensure!(
            reserved.request.fee_sats == self.fee_sats,
            "split fee binding changed"
        );
        tx.execute("INSERT INTO treasury_split_results(intent_id,frozen_digest,input_sats,fee_sats,result) VALUES(?,?,?,?,?)",
            params![self.intent_id.to_string(),self.frozen_digest.as_slice(),self.input_sats,self.fee_sats,encoded])?;
        for input in &reserved.inputs {
            let owner: String = tx.query_row(
                "SELECT intent_id FROM treasury_proof_holds WHERE mint_id=? AND y=?",
                params![binding.mint_id, input.y],
                |r| r.get(0),
            )?;
            ensure!(
                owner == self.intent_id.to_string(),
                "split input claim changed before commit"
            );
            tx.execute(
                "INSERT INTO treasury_split_spent_inputs(mint_id,y,intent_id) VALUES(?,?,?)",
                params![binding.mint_id, input.y, self.intent_id.to_string()],
            )?;
        }
        for output in &self.outputs {
            let exists:bool=tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM owned_output_sources WHERE mint_id=?1 AND y=?2 UNION ALL SELECT 1 FROM treasury_change_outputs WHERE mint_id=?1 AND y=?2 UNION ALL SELECT 1 FROM treasury_split_outputs WHERE mint_id=?1 AND y=?2)",
                params![binding.mint_id,output.y],|r|r.get(0))?;
            ensure!(!exists, "split output identity already exists");
            let purpose = match output.purpose {
                crate::treasury_split::Purpose::Treasury => "treasury",
                crate::treasury_split::Purpose::Remainder => "remainder",
            };
            tx.execute("INSERT INTO treasury_split_outputs(mint_id,y,c,amount,acquisition_id,purpose,intent_id) VALUES(?,?,?,?,?,?,?)",
                params![binding.mint_id,output.y,output.c,output.amount,output.owner.to_string(),purpose,self.intent_id.to_string()])?;
            if output.purpose == crate::treasury_split::Purpose::Treasury {
                tx.execute(
                    "INSERT INTO treasury_proof_holds(mint_id,y,intent_id) VALUES(?,?,?)",
                    params![binding.mint_id, output.y, self.intent_id.to_string()],
                )?;
            }
        }
        tx.commit()?;
        Ok(true)
    }
}

/// Quote creation may inspect a reserved plan before its swap completes. This
/// allowance never applies to melt proof selection or actual spending.
pub(super) fn planned_fee(
    connection: &Connection,
    intent: &lightning_goats_cashu_receipt::treasury::Intent,
) -> Result<Option<u64>> {
    let bytes: Option<Vec<u8>> = connection
        .query_row(
            "SELECT reservation FROM treasury_split_reservations WHERE intent_id=?",
            [intent.id.to_string()],
            |r| r.get(0),
        )
        .optional()?;
    let Some(bytes) = bytes else { return Ok(None) };
    ensure!(bytes.len() <= 1_048_576, "oversized split reservation");
    let retained: Reservation = serde_json::from_slice(&bytes)?;
    retained.plan.validate_for(
        intent,
        Limits {
            maximum_input_sats: i64::MAX as u64,
            maximum_fee_sats: intent.maximum_fee_sats,
            maximum_outputs: 1024,
        },
    )?;
    Ok(Some(retained.request.fee_sats))
}

#[cfg(test)]
mod tests {
    use super::*;
    use lightning_goats_cashu_receipt::treasury::Issued;

    pub(super) struct Fixture {
        pub(super) directory: tempfile::TempDir,
        pub(super) journal: Journal,
        pub(super) binding: TreasuryBinding,
        pub(super) issued: Issued,
        pub(super) request: Request,
    }
    pub(super) fn fixture() -> Result<Fixture> {
        let (binding, mut issued) = crate::treasury::tests::fixture();
        let directory = crate::test_directory()?;
        let mut journal = Journal::open(
            &directory.path().join("wallet.sqlite"),
            binding.wallet_epoch,
            128,
        )?;
        let acquisition = issued.intent.sources[0].acquisition_id;
        // Synthetic owned state; this fixture does not claim cryptographic mint
        // acceptance. Production acquisition performs that verification first.
        journal.connection.execute("INSERT INTO acquisitions(id,mint_id,policy,transport,source_ref,proof_count,gross,state,receive_fee,net,acquired_evidence) VALUES(?,?,'receive-v1','cashu','fixture',2,128,'credit_export_pending',0,128,?)",
            params![acquisition.to_string(),binding.mint_id,vec![7u8;32]])?;
        journal.connection.execute(
            "INSERT INTO acquisition_context(acquisition_id,proofs) VALUES(?,?)",
            params![acquisition.to_string(), b"[]".as_slice()],
        )?;
        let receipt = journal.export_receipt(acquisition)?;
        issued.intent.sources[0].receipt_digest =
            lightning_goats_cashu_receipt::treasury::hex_digest(
                crate::receipt_wire::from_private(binding.wallet_instance, &receipt)?.digest()?,
            );
        journal.bind_treasury_instruction(&binding, &issued, 1000)?;
        journal.reserve_treasury_sources(&binding, issued.intent.id, 1000)?;
        let mut ys = Vec::new();
        for secret in [b"split-one".as_slice(), b"split-two".as_slice()] {
            let y = cdk::dhke::hash_to_curve(secret)?.to_hex();
            journal.connection.execute("INSERT INTO owned_output_sources(mint_id,y,c,amount,acquisition_id) VALUES(?,?,?,64,?)",
                params![binding.mint_id,y,y,acquisition.to_string()])?;
            ys.push(y);
        }
        ys.sort();
        let request = Request {
            split_id: Uuid::from_u128(50),
            input_ys: ys,
            fee_sats: 1,
            denominations: vec![1, 2, 4, 8, 16, 32, 64],
        };
        Ok(Fixture {
            directory,
            journal,
            binding,
            issued,
            request,
        })
    }
    pub(super) fn limits() -> Limits {
        Limits {
            maximum_input_sats: 128,
            maximum_fee_sats: 1,
            maximum_outputs: 32,
        }
    }
    fn count(journal: &Journal, table: &str) -> Result<u64> {
        Ok(journal
            .connection
            .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))?)
    }

    #[test]
    fn split_replay_retains_original_proofs_costs_and_remainder_after_restart_and_expiry()
    -> Result<()> {
        let mut f = fixture()?;
        let retained = f.journal.reserve_treasury_split(
            &f.binding,
            f.issued.intent.id,
            &f.request,
            limits(),
            1000,
        )?;
        assert_eq!(retained.plan().fee_sats(), 1);
        for (purpose, expected) in [
            (crate::treasury_split::Purpose::Treasury, 100),
            (crate::treasury_split::Purpose::Remainder, 27),
        ] {
            assert_eq!(
                retained
                    .plan()
                    .outputs()
                    .iter()
                    .filter(|o| o.purpose == purpose)
                    .map(|o| o.amount)
                    .sum::<u64>(),
                expected
            );
        }
        assert_eq!(count(&f.journal, "treasury_proof_holds")?, 2);
        assert_eq!(count(&f.journal, "treasury_melt_requests")?, 0);
        drop(f.journal);
        f.journal = Journal::open(
            &f.directory.path().join("wallet.sqlite"),
            f.binding.wallet_epoch,
            128,
        )?;
        let replay = f.journal.reserve_treasury_split(
            &f.binding,
            f.issued.intent.id,
            &f.request,
            limits(),
            2000,
        )?;
        assert!(replay == retained);
        for change in 0..4 {
            let mut changed = f.request.clone();
            match change {
                0 => changed.split_id = Uuid::from_u128(51),
                1 => changed.fee_sats = 0,
                2 => {
                    changed.input_ys.pop();
                }
                _ => {
                    changed.denominations.pop();
                }
            }
            assert!(
                f.journal
                    .reserve_treasury_split(
                        &f.binding,
                        f.issued.intent.id,
                        &changed,
                        limits(),
                        2000
                    )
                    .is_err()
            );
        }
        let selected = BTreeMap::from([(
            f.issued.intent.sources[0].acquisition_id,
            f.request.input_ys.clone(),
        )]);
        let error = f
            .journal
            .reserve_treasury_proof_ids(&f.binding, f.issued.intent.id, &selected, 1000)
            .unwrap_err();
        assert!(error.to_string().contains("original split must reconcile"));
        assert_eq!(count(&f.journal, "treasury_split_reservations")?, 1);
        Ok(())
    }

    #[test]
    fn split_limit_and_provenance_failures_roll_back_every_new_proof_hold() -> Result<()> {
        let mut f = fixture()?;
        let mut too_small = limits();
        too_small.maximum_input_sats = 127;
        assert!(
            f.journal
                .reserve_treasury_split(&f.binding, f.issued.intent.id, &f.request, too_small, 1000)
                .is_err()
        );
        assert_eq!(count(&f.journal, "treasury_proof_holds")?, 0);
        f.journal.connection.execute(
            "UPDATE owned_output_sources SET c='invalid' WHERE y=?",
            [&f.request.input_ys[1]],
        )?;
        assert!(
            f.journal
                .reserve_treasury_split(&f.binding, f.issued.intent.id, &f.request, limits(), 1000)
                .is_err()
        );
        assert_eq!(count(&f.journal, "treasury_proof_holds")?, 0);
        assert_eq!(count(&f.journal, "treasury_split_reservations")?, 0);
        // Repair only this synthetic fixture and prove a fresh expired invoice
        // cannot acquire holds even though replay of existing work is allowed.
        f.journal
            .connection
            .execute("UPDATE owned_output_sources SET c=y", [])?;
        assert!(
            f.journal
                .reserve_treasury_split(&f.binding, f.issued.intent.id, &f.request, limits(), 2000)
                .is_err()
        );
        assert_eq!(count(&f.journal, "treasury_proof_holds")?, 0);
        f.journal.reserve_treasury_split(
            &f.binding,
            f.issued.intent.id,
            &f.request,
            limits(),
            1000,
        )?;
        Ok(())
    }

    #[test]
    fn shared_change_keeps_an_unassigned_owners_value_and_rejects_incomplete_provenance()
    -> Result<()> {
        let mut f = fixture()?;
        let prior = Uuid::from_u128(90).to_string();
        let foreign = Uuid::from_u128(91);
        let source = f.issued.intent.sources[0].acquisition_id;
        let y = &f.request.input_ys[1];
        // A synthetic completed prior melt establishes shared change. The test
        // exercises durable attribution, not validation of that prior melt.
        f.journal.connection.execute("INSERT INTO treasury_instructions(id,issued_json,receive_id,payment_hash,state) VALUES(?,?,?,?,'reconciled')",
            params![prior,b"{}".as_slice(),Uuid::from_u128(92).to_string(),"ef".repeat(32)])?;
        f.journal.connection.execute(
            "INSERT INTO treasury_prepared(intent_id,operation_id,prepared_json) VALUES(?,?,?)",
            params![prior, Uuid::from_u128(93).to_string(), b"{}".as_slice()],
        )?;
        f.journal.connection.execute(
            "INSERT INTO treasury_melt_requests(intent_id,request,derivation) VALUES(?,?,?)",
            params![prior, b"{}".as_slice(), b"{}".as_slice()],
        )?;
        f.journal.connection.execute(
            "INSERT INTO treasury_results(intent_id,report,paid_evidence) VALUES(?,?,?)",
            params![prior, b"{}".as_slice(), b"{}".as_slice()],
        )?;
        f.journal.connection.execute(
            "INSERT INTO treasury_change_outputs(mint_id,y,c,amount,intent_id) VALUES(?,?,?,96,?)",
            params![f.binding.mint_id, y, y, prior],
        )?;
        for (owner, value) in [(source, 64), (foreign, 32)] {
            f.journal.connection.execute("INSERT INTO treasury_change_source_parts(mint_id,y,acquisition_id,amount) VALUES(?,?,?,?)",
                params![f.binding.mint_id,y,owner.to_string(),value])?;
        }
        let limits = Limits {
            maximum_input_sats: 160,
            ..limits()
        };
        // A duplicate Y in both owned-output origins is ambiguous, even if its
        // parts happen to look affordable. No new holds survive the rejection.
        assert!(
            f.journal
                .reserve_treasury_split(&f.binding, f.issued.intent.id, &f.request, limits, 1000)
                .is_err()
        );
        assert_eq!(count(&f.journal, "treasury_proof_holds")?, 0);
        f.journal
            .connection
            .execute("DELETE FROM owned_output_sources WHERE y=?", [y])?;
        f.journal.connection.execute(
            "UPDATE treasury_change_source_parts SET amount=31 WHERE acquisition_id=?",
            [foreign.to_string()],
        )?;
        assert!(
            f.journal
                .reserve_treasury_split(&f.binding, f.issued.intent.id, &f.request, limits, 1000)
                .is_err()
        );
        assert_eq!(count(&f.journal, "treasury_proof_holds")?, 0);
        f.journal.connection.execute(
            "UPDATE treasury_change_source_parts SET amount=32 WHERE acquisition_id=?",
            [foreign.to_string()],
        )?;
        let retained = f.journal.reserve_treasury_split(
            &f.binding,
            f.issued.intent.id,
            &f.request,
            limits,
            1000,
        )?;
        let foreign_outputs: Vec<_> = retained
            .plan()
            .outputs()
            .iter()
            .filter(|o| o.acquisition_id == foreign)
            .collect();
        assert_eq!(foreign_outputs.iter().map(|o| o.amount).sum::<u64>(), 32);
        assert!(
            foreign_outputs
                .iter()
                .all(|o| o.purpose == crate::treasury_split::Purpose::Remainder)
        );
        assert_eq!(
            retained
                .plan()
                .outputs()
                .iter()
                .map(|o| o.amount)
                .sum::<u64>(),
            159
        );
        Ok(())
    }

    #[test]
    fn competing_proof_claim_rolls_back_the_first_input_without_changing_the_owner() -> Result<()> {
        let mut f = fixture()?;
        let competing = Uuid::from_u128(99);
        // A separate retained operation owns the second proof. Its serialized
        // instruction is irrelevant to the proof exclusion being exercised.
        f.journal.connection.execute("INSERT INTO treasury_instructions(id,issued_json,receive_id,payment_hash) VALUES(?,?,?,?)",
            params![competing.to_string(),b"{}".as_slice(),Uuid::from_u128(98).to_string(),"cd".repeat(32)])?;
        f.journal.connection.execute(
            "INSERT INTO treasury_proof_holds(mint_id,y,intent_id) VALUES(?,?,?)",
            params![
                f.binding.mint_id,
                f.request.input_ys[1],
                competing.to_string()
            ],
        )?;
        assert!(
            f.journal
                .reserve_treasury_split(&f.binding, f.issued.intent.id, &f.request, limits(), 1000)
                .is_err()
        );
        assert_eq!(count(&f.journal, "treasury_proof_holds")?, 1);
        let owner: String = f.journal.connection.query_row(
            "SELECT intent_id FROM treasury_proof_holds",
            [],
            |r| r.get(0),
        )?;
        assert_eq!(owner, competing.to_string());
        assert_eq!(count(&f.journal, "treasury_split_reservations")?, 0);
        Ok(())
    }
    #[test]
    fn split_request_freeze_binds_original_cdk_operation_seed_and_bytes_without_new_credit()
    -> Result<()> {
        use cdk::{
            Amount,
            mint_url::MintUrl,
            nuts::{CurrencyUnit, Id, PreMintSecrets, Proof, SwapRequest},
            secret::Secret,
            wallet::types::{
                OperationData, SwapOperationData, SwapSagaState, WalletSaga, WalletSagaState,
            },
        };
        use std::str::FromStr;
        let mut f = fixture()?;
        let seed = [42; 64];
        let reserved = f.journal.reserve_treasury_split(
            &f.binding,
            f.issued.intent.id,
            &f.request,
            limits(),
            1000,
        )?;
        let keyset = Id::from_str("00deadbeef123456")?;
        let proofs = ["split-one", "split-two"]
            .into_iter()
            .map(|s| {
                let secret = Secret::new(s);
                Ok(Proof::new(
                    Amount::from(64),
                    keyset,
                    secret.clone(),
                    cdk::dhke::hash_to_curve(secret.as_bytes())?,
                ))
            })
            .collect::<Result<Vec<_>>>()?;
        let output_count = u32::try_from(reserved.plan().outputs().len())?;
        let derived = PreMintSecrets::restore_batch(keyset, &seed, 0, output_count)?;
        let outputs = derived
            .secrets
            .iter()
            .zip(reserved.plan().outputs())
            .map(|(item, planned)| {
                let mut output = item.blinded_message.clone();
                output.amount = planned.amount.into();
                output
            })
            .collect::<Vec<_>>();
        let request = SwapRequest::new(proofs, outputs.clone());
        let bytes = serde_json::to_vec(&request)?;
        let saga = WalletSaga::new(
            Uuid::from_u128(88),
            WalletSagaState::Swap(SwapSagaState::SwapRequested),
            Amount::from(128),
            MintUrl::from_str("https://mint.example")?,
            CurrencyUnit::Sat,
            OperationData::Swap(SwapOperationData {
                input_amount: Amount::from(128),
                output_amount: Amount::from(127),
                counter_start: Some(0),
                counter_end: Some(output_count),
                blinded_messages: Some(outputs),
            }),
        );
        let mint = crate::mint_policy::MintPolicy {
            id: f.binding.mint_id.clone(),
            advertised_url: "https://mint.example".into(),
            policy_version: "receive-v1".into(),
            receive_cashu: true,
            receive_nutzap: true,
            bolt11_exit_qualified: true,
        };
        assert!(
            f.journal
                .freeze_treasury_split_request(
                    &f.binding,
                    &mint,
                    f.issued.intent.id,
                    &bytes,
                    &saga,
                    &seed,
                    limits(),
                    1000
                )
                .is_err()
        );
        assert_eq!(count(&f.journal, "treasury_split_requests")?, 0);
        let quote_request = cdk::nuts::MeltQuoteBolt11Request {
            request: f.issued.invoice.parse()?,
            unit: CurrencyUnit::Sat,
            options: None,
        };
        let quote_url = saga.mint_url.join("v1/melt/quote/bolt11")?;
        f.journal.claim_treasury_quote_request(
            &f.binding,
            &mint,
            f.issued.intent.id,
            &quote_url,
            &quote_request,
            1000,
        )?;
        assert!(
            f.journal
                .claim_treasury_quote_request(
                    &f.binding,
                    &mint,
                    f.issued.intent.id,
                    &quote_url,
                    &quote_request,
                    1000
                )
                .is_err()
        );
        let mut pre_split_quote = cdk::wallet::MeltQuote {
            id: "after-split".into(),
            mint_url: Some(saga.mint_url.clone()),
            unit: CurrencyUnit::Sat,
            amount: 100u64.into(),
            request: f.issued.invoice.clone(),
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
        assert!(
            f.journal
                .bind_treasury_quote(
                    &f.binding,
                    &mint,
                    f.issued.intent.id,
                    &pre_split_quote,
                    1000
                )
                .is_err()
        );
        assert_eq!(count(&f.journal, "treasury_quotes")?, 0);
        pre_split_quote.fee_reserve = 0u64.into();
        f.journal.bind_treasury_quote(
            &f.binding,
            &mint,
            f.issued.intent.id,
            &pre_split_quote,
            1000,
        )?;
        assert!(
            f.journal
                .claim_treasury_preparation(&f.binding, f.issued.intent.id, 1000)
                .is_err()
        );
        assert_eq!(count(&f.journal, "treasury_preparation_claims")?, 0);
        assert!(
            f.journal
                .freeze_treasury_split_request(
                    &f.binding,
                    &mint,
                    f.issued.intent.id,
                    &bytes,
                    &saga,
                    &seed,
                    limits(),
                    1251
                )
                .is_err()
        );
        assert!(
            f.journal
                .freeze_treasury_split_request(
                    &f.binding,
                    &mint,
                    f.issued.intent.id,
                    &bytes,
                    &saga,
                    &[1; 64],
                    limits(),
                    1000
                )
                .is_err()
        );
        assert_eq!(count(&f.journal, "treasury_split_requests")?, 0);
        let mut altered_outputs = request.outputs().clone();
        let last = altered_outputs.len() - 1;
        altered_outputs[last].amount = (u64::from(altered_outputs[last].amount) + 1).into();
        altered_outputs[last - 1].amount = (u64::from(altered_outputs[last - 1].amount) - 1).into();
        let altered_request = SwapRequest::new(request.inputs().clone(), altered_outputs.clone());
        let mut altered_saga = saga.clone();
        if let OperationData::Swap(ref mut data) = altered_saga.data {
            data.blinded_messages = Some(altered_outputs);
        }
        assert!(
            f.journal
                .freeze_treasury_split_request(
                    &f.binding,
                    &mint,
                    f.issued.intent.id,
                    &serde_json::to_vec(&altered_request)?,
                    &altered_saga,
                    &seed,
                    limits(),
                    1000
                )
                .is_err()
        );
        assert_eq!(count(&f.journal, "treasury_split_requests")?, 0);
        let frozen = f.journal.freeze_treasury_split_request(
            &f.binding,
            &mint,
            f.issued.intent.id,
            &bytes,
            &saga,
            &seed,
            limits(),
            1000,
        )?;
        assert_eq!(
            frozen.assignments.iter().map(|a| a.amount).sum::<u64>(),
            127
        );
        assert_eq!(frozen.request, bytes);
        assert_eq!(frozen.split_id, f.request.split_id);
        assert_eq!(count(&f.journal, "acquisitions")?, 1);
        assert_eq!(count(&f.journal, "treasury_results")?, 0);
        let admission = AdmissionPolicy {
            enabled: true,
            limits: limits(),
            operations_per_24h: 1,
            input_sats_per_24h: 128,
            maximum_in_flight_sats: 128,
        };
        let destination = saga.mint_url.join("v1/swap")?;
        assert!(
            f.journal
                .record_treasury_split_reply(f.issued.intent.id, b"{}")
                .is_err()
        );
        assert!(
            f.journal
                .claim_treasury_split_submission(
                    &f.binding,
                    f.issued.intent.id,
                    &destination,
                    &bytes,
                    0,
                    &admission,
                    1000
                )
                .is_err()
        );
        f.journal
            .authorize_treasury_split(&f.binding, f.issued.intent.id, &admission, 1000)?;
        assert!(
            f.journal
                .claim_treasury_split_submission(
                    &f.binding,
                    f.issued.intent.id,
                    &"https://other.example/v1/swap".parse()?,
                    &bytes,
                    0,
                    &admission,
                    1000
                )
                .is_err()
        );
        let mut changed = vec![b' '];
        changed.extend_from_slice(&bytes);
        assert!(
            f.journal
                .claim_treasury_split_submission(
                    &f.binding,
                    f.issued.intent.id,
                    &destination,
                    &changed,
                    0,
                    &admission,
                    1000
                )
                .is_err()
        );
        assert!(
            f.journal
                .claim_treasury_split_submission(
                    &f.binding,
                    f.issued.intent.id,
                    &destination,
                    &bytes,
                    1,
                    &admission,
                    1000
                )
                .is_err()
        );
        assert!(
            f.journal
                .claim_treasury_split_submission(
                    &f.binding,
                    f.issued.intent.id,
                    &destination,
                    &bytes,
                    u64::MAX,
                    &admission,
                    1000
                )
                .is_err()
        );
        assert!(
            f.journal
                .claim_treasury_split_submission(
                    &f.binding,
                    f.issued.intent.id,
                    &destination,
                    &bytes,
                    0,
                    &admission,
                    1251
                )
                .is_err()
        );
        assert!(
            f.journal
                .claim_treasury_split_submission(
                    &f.binding,
                    f.issued.intent.id,
                    &destination,
                    &bytes,
                    0,
                    &AdmissionPolicy::default(),
                    1000
                )
                .is_err()
        );
        let mut lower = admission.clone();
        lower.limits.maximum_input_sats = 127;
        assert!(
            f.journal
                .claim_treasury_split_submission(
                    &f.binding,
                    f.issued.intent.id,
                    &destination,
                    &bytes,
                    0,
                    &lower,
                    1000
                )
                .is_err()
        );
        assert_eq!(count(&f.journal, "treasury_split_submissions")?, 0);
        f.journal.claim_treasury_split_submission(
            &f.binding,
            f.issued.intent.id,
            &destination,
            &bytes,
            0,
            &admission,
            1000,
        )?;
        assert_eq!(count(&f.journal, "treasury_split_submissions")?, 1);
        assert_eq!(count(&f.journal, "treasury_split_replies")?, 0);
        drop(f.journal);
        f.journal = Journal::open(
            &f.directory.path().join("wallet.sqlite"),
            f.binding.wallet_epoch,
            128,
        )?;
        assert!(
            f.journal
                .claim_treasury_split_submission(
                    &f.binding,
                    f.issued.intent.id,
                    &destination,
                    &bytes,
                    0,
                    &admission,
                    1000
                )
                .is_err()
        );
        assert_eq!(count(&f.journal, "treasury_split_submissions")?, 1);
        // These bytes exercise transport-record idempotency, not mint proof
        // acceptance; cryptographic reconciliation below supplies signed proofs.
        f.journal
            .record_treasury_split_reply(f.issued.intent.id, b"{}")?;
        f.journal
            .record_treasury_split_reply(f.issued.intent.id, b"{}")?;
        assert!(
            f.journal
                .record_treasury_split_reply(f.issued.intent.id, b"[]")
                .is_err()
        );
        let replay = f.journal.freeze_treasury_split_request(
            &f.binding,
            &mint,
            f.issued.intent.id,
            &bytes,
            &saga,
            &seed,
            limits(),
            2000,
        )?;
        assert!(frozen == replay);
        let mut different = saga.clone();
        different.id = Uuid::from_u128(89);
        assert!(
            f.journal
                .freeze_treasury_split_request(
                    &f.binding,
                    &mint,
                    f.issued.intent.id,
                    &bytes,
                    &different,
                    &seed,
                    limits(),
                    2000
                )
                .is_err()
        );
        let mut changed_bytes = vec![b' '];
        changed_bytes.extend_from_slice(&bytes);
        assert!(
            f.journal
                .freeze_treasury_split_request(
                    &f.binding,
                    &mint,
                    f.issued.intent.id,
                    &changed_bytes,
                    &saga,
                    &seed,
                    limits(),
                    2000
                )
                .is_err()
        );
        assert_eq!(count(&f.journal, "treasury_split_requests")?, 1);
        // Reconciliation requires original spent inputs and exact seed-derived,
        // DLEQ-verified unspent outputs. It cannot manufacture a new acquisition.
        let issuer = cdk::nuts::SecretKey::from_slice(&[7; 32])?;
        let mut pinned = crate::intake::PinnedKeys::default();
        let mut owned = Vec::new();
        for (premint, output) in derived.secrets.iter().zip(request.outputs()) {
            pinned.insert(
                &f.binding.mint_id,
                keyset,
                u64::from(output.amount),
                issuer.public_key(),
            )?;
            let blinded_signature = cdk::dhke::sign_message(&issuer, &output.blinded_secret)?;
            let signed = cdk::nuts::BlindSignature::new(
                output.amount,
                blinded_signature,
                keyset,
                &output.blinded_secret,
                &issuer,
            )?;
            let c =
                cdk::dhke::unblind_message(&blinded_signature, &premint.r, &issuer.public_key())?;
            let mut proof = Proof::new(output.amount, keyset, premint.secret.clone(), c);
            let dleq = signed.dleq.expect("fixture DLEQ");
            proof.dleq = Some(cdk::nuts::ProofDleq::new(dleq.e, dleq.s, premint.r.clone()));
            owned.push(cdk::wallet::types::ProofInfo::new(
                proof,
                saga.mint_url.clone(),
                cdk::nuts::State::Unspent,
                CurrencyUnit::Sat,
            )?);
        }
        let fresh_inputs = cdk::nuts::CheckStateResponse {
            states: request
                .inputs()
                .iter()
                .map(|p| Ok((p.y()?, cdk::nuts::State::Spent).into()))
                .collect::<Result<Vec<_>>>()?,
        };
        let fresh_outputs = cdk::nuts::CheckStateResponse {
            states: owned
                .iter()
                .map(|p| (p.y, cdk::nuts::State::Unspent).into())
                .collect(),
        };
        let verified = f.journal.verify_treasury_split_outputs(
            &f.binding,
            f.issued.intent.id,
            &seed,
            &pinned,
            &owned,
            &fresh_inputs,
            &fresh_outputs,
        )?;
        assert_eq!(verified.intent_id(), f.issued.intent.id);
        assert_eq!(verified.fee_sats(), 1);
        assert_eq!(verified.input_sats(), 128);
        assert_eq!(
            verified.outputs().iter().map(|o| o.amount).sum::<u64>(),
            127
        );
        assert!(
            verified
                .outputs()
                .iter()
                .all(|o| o.owner == f.issued.intent.sources[0].acquisition_id)
        );
        let mut unspent_input = fresh_inputs.clone();
        unspent_input.states[0].state = cdk::nuts::State::Unspent;
        assert!(
            f.journal
                .verify_treasury_split_outputs(
                    &f.binding,
                    f.issued.intent.id,
                    &seed,
                    &pinned,
                    &owned,
                    &unspent_input,
                    &fresh_outputs
                )
                .is_err()
        );
        let mut missing_state = fresh_outputs.clone();
        missing_state.states.pop();
        assert!(
            f.journal
                .verify_treasury_split_outputs(
                    &f.binding,
                    f.issued.intent.id,
                    &seed,
                    &pinned,
                    &owned,
                    &fresh_inputs,
                    &missing_state
                )
                .is_err()
        );
        let mut no_dleq = owned.clone();
        no_dleq[0].proof.dleq = None;
        assert!(
            f.journal
                .verify_treasury_split_outputs(
                    &f.binding,
                    f.issued.intent.id,
                    &seed,
                    &pinned,
                    &no_dleq,
                    &fresh_inputs,
                    &fresh_outputs
                )
                .is_err()
        );
        assert!(
            f.journal
                .verify_treasury_split_outputs(
                    &f.binding,
                    f.issued.intent.id,
                    &[1; 64],
                    &pinned,
                    &owned,
                    &fresh_inputs,
                    &fresh_outputs
                )
                .is_err()
        );
        assert_eq!(count(&f.journal, "treasury_results")?, 0);
        assert_eq!(count(&f.journal, "acquisitions")?, 1);
        // A late storage failure must roll back result, spent-input exclusions,
        // inserted outputs and new treasury holds together.
        f.journal.connection.execute_batch("CREATE TRIGGER split_fixture_failure BEFORE INSERT ON treasury_split_outputs WHEN (SELECT COUNT(*) FROM treasury_split_outputs)>0 BEGIN SELECT RAISE(ABORT,'fixture late failure'); END;")?;
        assert!(verified.commit(&mut f.journal, &f.binding).is_err());
        assert_eq!(count(&f.journal, "treasury_split_results")?, 0);
        assert_eq!(count(&f.journal, "treasury_split_outputs")?, 0);
        assert_eq!(count(&f.journal, "treasury_split_spent_inputs")?, 0);
        assert_eq!(count(&f.journal, "treasury_proof_holds")?, 2);
        f.journal
            .connection
            .execute_batch("DROP TRIGGER split_fixture_failure;")?;
        let verified = f.journal.verify_treasury_split_outputs(
            &f.binding,
            f.issued.intent.id,
            &seed,
            &pinned,
            &owned,
            &fresh_inputs,
            &fresh_outputs,
        )?;
        assert!(verified.commit(&mut f.journal, &f.binding)?);
        assert_eq!(count(&f.journal, "treasury_split_results")?, 1);
        assert_eq!(count(&f.journal, "treasury_split_spent_inputs")?, 2);
        let total: u64 = f.journal.connection.query_row(
            "SELECT SUM(amount) FROM treasury_owned_outputs",
            [],
            |r| r.get(0),
        )?;
        assert_eq!(total, 127);
        let free:u64=f.journal.connection.query_row("SELECT SUM(o.amount) FROM treasury_owned_outputs o WHERE NOT EXISTS(SELECT 1 FROM treasury_proof_holds h WHERE h.mint_id=o.mint_id AND h.y=o.y)",[],|r|r.get(0))?;
        assert_eq!(free, 27);
        let held:u64=f.journal.connection.query_row("SELECT SUM(o.amount) FROM treasury_owned_outputs o JOIN treasury_proof_holds h ON h.mint_id=o.mint_id AND h.y=o.y WHERE h.intent_id=?",[f.issued.intent.id.to_string()],|r|r.get(0))?;
        assert_eq!(held, 100);
        let original_sources: u64 = f.journal.connection.query_row(
            "SELECT SUM(input_sats) FROM treasury_source_holds WHERE intent_id=?",
            [f.issued.intent.id.to_string()],
            |r| r.get(0),
        )?;
        assert_eq!(original_sources, 101);
        let fee: u64 = f.journal.connection.query_row(
            "SELECT fee_sats FROM treasury_split_results WHERE intent_id=?",
            [f.issued.intent.id.to_string()],
            |r| r.get(0),
        )?;
        assert_eq!(fee, 1);
        // Reopen an older two-origin view definition: startup must replace it
        // transactionally, preserving retained rows and spent-input exclusion.
        f.journal.connection.execute_batch("DROP VIEW treasury_owned_outputs; CREATE VIEW treasury_owned_outputs AS SELECT mint_id,y,c,amount FROM owned_output_sources UNION ALL SELECT mint_id,y,c,amount FROM treasury_change_outputs; DROP VIEW treasury_output_parts; CREATE VIEW treasury_output_parts AS SELECT mint_id,y,acquisition_id,amount FROM owned_output_sources UNION ALL SELECT mint_id,y,acquisition_id,amount FROM treasury_change_source_parts;")?;
        drop(f.journal);
        f.journal = Journal::open(
            &f.directory.path().join("wallet.sqlite"),
            f.binding.wallet_epoch,
            128,
        )?;
        let total: u64 = f.journal.connection.query_row(
            "SELECT SUM(amount) FROM treasury_owned_outputs",
            [],
            |r| r.get(0),
        )?;
        assert_eq!(total, 127);
        let parts: u64 = f.journal.connection.query_row(
            "SELECT SUM(amount) FROM treasury_output_parts",
            [],
            |r| r.get(0),
        )?;
        assert_eq!(parts, 127);
        let replay = f.journal.verify_treasury_split_outputs(
            &f.binding,
            f.issued.intent.id,
            &seed,
            &pinned,
            &owned,
            &fresh_inputs,
            &fresh_outputs,
        )?;
        assert!(!replay.commit(&mut f.journal, &f.binding)?);
        assert_eq!(count(&f.journal, "treasury_split_results")?, 1);
        assert_eq!(count(&f.journal, "treasury_results")?, 0);
        assert_eq!(count(&f.journal, "acquisitions")?, 1);
        // Melt preparation uses only the post-split treasury outputs and the
        // remainder of the original fee authority. Old spent holds remain.
        let selected = f
            .journal
            .select_treasury_proofs(&f.binding, f.issued.intent.id, 1000)?;
        let selected_ys: std::collections::BTreeSet<_> =
            selected.values().flatten().cloned().collect();
        assert!(!selected_ys.is_empty());
        assert!(f.request.input_ys.iter().all(|y| !selected_ys.contains(y)));
        let melt_proofs = owned
            .iter()
            .filter(|p| selected_ys.contains(&p.y.to_hex()))
            .map(|p| p.proof.clone())
            .collect::<Vec<_>>();
        assert_eq!(
            melt_proofs.iter().map(|p| u64::from(p.amount)).sum::<u64>(),
            100
        );
        let mut quote = cdk::wallet::MeltQuote {
            id: "after-split".into(),
            mint_url: Some(saga.mint_url.clone()),
            unit: CurrencyUnit::Sat,
            amount: 100u64.into(),
            request: f.issued.invoice.clone(),
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
        assert!(
            f.journal
                .bind_treasury_quote(&f.binding, &mint, f.issued.intent.id, &quote, 1000)
                .is_err()
        );
        assert_eq!(count(&f.journal, "treasury_quotes")?, 1);
        quote.fee_reserve = 0u64.into();
        f.journal
            .bind_treasury_quote(&f.binding, &mint, f.issued.intent.id, &quote, 1000)?;
        let claim = f
            .journal
            .claim_treasury_preparation(&f.binding, f.issued.intent.id, 1000)?;
        assert_eq!(
            claim
                .ys
                .iter()
                .map(|y| y.to_hex())
                .collect::<std::collections::BTreeSet<_>>(),
            selected_ys
        );
        let recovered =
            f.journal
                .claimed_treasury_preparation(&f.binding, f.issued.intent.id, 1000)?;
        assert_eq!(claim.ys, recovered.ys);
        assert_eq!(claim.operation, recovered.operation);
        let operation = claim.operation.unwrap();
        assert!(
            f.journal
                .bind_treasury_preparation_evidence(
                    &f.binding,
                    f.issued.intent.id,
                    operation,
                    &quote,
                    1,
                    &melt_proofs,
                    1000
                )
                .is_err()
        );
        assert_eq!(count(&f.journal, "treasury_prepared")?, 0);
        assert!(
            f.journal
                .bind_treasury_preparation_evidence(
                    &f.binding,
                    f.issued.intent.id,
                    Uuid::new_v4(),
                    &quote,
                    0,
                    &melt_proofs,
                    1000
                )
                .is_err()
        );
        f.journal.bind_treasury_preparation_evidence(
            &f.binding,
            f.issued.intent.id,
            operation,
            &quote,
            0,
            &melt_proofs,
            1000,
        )?;
        let context =
            f.journal
                .unsubmitted_treasury_preparation(&f.binding, f.issued.intent.id, 1000)?;
        assert_eq!(context.operation, operation);
        assert_eq!(
            context
                .ys
                .iter()
                .map(|y| y.to_hex())
                .collect::<std::collections::BTreeSet<_>>(),
            selected_ys
        );
        Ok(())
    }
}
