//! Protected observation of original melt and custody state from a fenced checkpoint.
use crate::{
    backup_copy::{Frozen, protected_directory},
    melt_reconciliation::poll_original_quote,
    transport::MintTransport,
};
use anyhow::{Context, Result, ensure};
use cdk::{
    mint_url::MintUrl,
    nuts::{CheckStateRequest, CheckStateResponse, MeltQuoteState, PublicKey, State},
    wallet::MeltQuote,
};
use lightning_goats_cashu_receipt::treasury::{Issued, MAX_TREASURY_BYTES, hex_digest};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs::{File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    os::unix::fs::OpenOptionsExt,
    path::Path,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use uuid::Uuid;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Selection {
    version: u8,
    purpose: String,
    checkpoint_id: Uuid,
    staged_receipt_sha256: String,
    operation_id: Uuid,
    issued: Issued,
    quote: MeltQuote,
}

impl Selection {
    pub(crate) async fn observe<T: cdk_http_client::Transport>(
        &self,
        approved: &MintUrl,
        digest: &str,
        transport: &T,
    ) -> Result<Value> {
        ensure!(
            self.version == 1
                && self.purpose == "cashu_restore_mint_melt"
                && !self.checkpoint_id.is_nil()
                && !self.operation_id.is_nil()
                && !self.issued.receive_request_id.is_nil()
                && self.issued.invoice.len() <= 8192
                && self.quote.payment_proof.is_none(),
            "invalid original mint selection"
        );
        lightning_goats_nostr_event::validate_hex(&self.staged_receipt_sha256, 32)?;
        let original = &self.issued;
        let observation = poll_original_quote(transport, original, &self.quote, approved).await?;
        Ok(json!({
            "version": 1,
            "result": "CASHU_RESTORE_MINT_MELT_OBSERVED",
            "checkpoint_id": self.checkpoint_id,
            "staged_receipt_sha256": self.staged_receipt_sha256,
            "selection_sha256": digest,
            "intent_id": original.intent.id,
            "intent_digest": hex_digest(original.intent.digest()?),
            "mint_id": original.intent.mint_id,
            "operation_id": self.operation_id,
            "mint_url": approved,
            "observed_at": SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs(),
            "paid_preimage_verified": observation.state == MeltQuoteState::Paid,
            "observation": observation,
            "external_reconciliation_verified": false,
            "restore_authorized": false,
            "resume_authorized": false,
        }))
    }
}

fn read_selection<T: serde::de::DeserializeOwned>(
    selection_path: &Path,
    expected_sha256: &str,
    output: &Path,
) -> Result<T> {
    ensure!(
        rustix::process::geteuid().as_raw() == 0,
        "root observer required"
    );
    protected_directory(
        selection_path
            .parent()
            .context("selection parent missing")?,
    )?;
    let parent = output.parent().context("observation parent missing")?;
    protected_directory(parent)?;
    ensure!(
        output
            .symlink_metadata()
            .is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound),
        "mint observation output must be a new file"
    );
    let maximum = MAX_TREASURY_BYTES as u64;
    let mut frozen = Frozen::open(selection_path, maximum, false)?;
    frozen.file.seek(SeekFrom::Start(0))?;
    let mut bytes = Vec::new();
    (&mut frozen.file)
        .take(maximum + 1)
        .read_to_end(&mut bytes)?;
    frozen.unchanged(selection_path, maximum, false)?;
    let digest = hex_digest(Sha256::digest(&bytes).into());
    ensure!(
        bytes.len() <= MAX_TREASURY_BYTES && digest == expected_sha256,
        "mint selection checksum differs"
    );
    Ok(serde_json::from_slice(&bytes)?)
}

fn write_observation(output: &Path, receipt: &Value) -> Result<()> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(output)?;
    serde_json::to_writer(&mut file, receipt)?;
    file.write_all(b"\n")?;
    file.sync_all()?;
    File::open(output.parent().context("observation parent missing")?)?.sync_all()?;
    Ok(())
}

/// Read one original quote through the constrained mint transport.
pub async fn observe(
    selection: &Path,
    expected_sha256: &str,
    mint_url: &str,
    output: &Path,
) -> Result<()> {
    let selection: Selection = read_selection(selection, expected_sha256, output)?;
    let approved: MintUrl = mint_url.parse()?;
    let transport = MintTransport::new(mint_url.parse()?, Duration::from_secs(15), 1_048_576)?;
    let receipt = selection
        .observe(&approved, expected_sha256, &transport)
        .await?;
    write_observation(output, &receipt)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CustodyProof {
    y: String,
    amount_sats: u64,
    local_state: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CustodySelection {
    version: u8,
    purpose: String,
    checkpoint_id: Uuid,
    staged_receipt_sha256: String,
    mint_store_sha256: String,
    mint_id: String,
    mint_url: MintUrl,
    total_candidates: u64,
    after_y: Option<String>,
    next_after_y: Option<String>,
    proofs: Vec<CustodyProof>,
}

fn match_custody_states(
    points: &[PublicKey],
    reply: CheckStateResponse,
) -> Result<BTreeMap<PublicKey, State>> {
    let mut states = BTreeMap::new();
    for state in reply.states {
        ensure!(
            states.insert(state.y, state.state).is_none(),
            "duplicate mint proof state"
        );
    }
    ensure!(
        states.len() == points.len() && points.iter().all(|p| states.contains_key(p)),
        "mint proof states do not cover exact selected points"
    );
    Ok(states)
}

impl CustodySelection {
    pub(crate) async fn observe<T: cdk_http_client::Transport>(
        &self,
        approved: &MintUrl,
        digest: &str,
        transport: &T,
    ) -> Result<Value> {
        ensure!(
            self.version == 1
                && self.purpose == "cashu_restore_mint_custody"
                && !self.checkpoint_id.is_nil()
                && !self.mint_id.is_empty()
                && self.mint_id.len() <= 256
                && !self.mint_id.chars().any(char::is_control)
                && &self.mint_url == approved
                && self.proofs.len() <= 128
                && self.proofs.len() as u64 <= self.total_candidates,
            "invalid mint custody selection"
        );
        lightning_goats_nostr_event::validate_hex(&self.staged_receipt_sha256, 32)?;
        lightning_goats_nostr_event::validate_hex(&self.mint_store_sha256, 32)?;
        if self.after_y.is_none() {
            ensure!(
                self.proofs.len() as u64 == self.total_candidates.min(128)
                    && self.next_after_y.is_some() == (self.total_candidates > 128),
                "incomplete first custody page"
            );
        }
        if let Some(after) = &self.after_y {
            let after_point: PublicKey = after.parse()?;
            ensure!(after_point.to_string() == *after, "invalid custody cursor");
        }
        if let Some(next) = &self.next_after_y {
            ensure!(
                self.proofs.len() == 128 && self.proofs.last().is_some_and(|p| &p.y == next),
                "invalid custody continuation"
            );
        }
        let mut previous = self.after_y.as_deref().unwrap_or("");
        let mut points = Vec::with_capacity(self.proofs.len());
        for proof in &self.proofs {
            let point: PublicKey = proof.y.parse()?;
            ensure!(
                point.to_string() == proof.y
                    && proof.y.as_str() > previous
                    && proof.amount_sats != 0
                    && matches!(
                        proof.local_state.as_str(),
                        "UNSPENT" | "RESERVED" | "PENDING" | "UNKNOWN" | "PENDING_SPENT"
                    ),
                "invalid original proof identity or state"
            );
            points.push(point);
            previous = &proof.y;
        }
        let states = if points.is_empty() {
            BTreeMap::new()
        } else {
            // NUT-07's POST asks only for proof states. No secrets, signatures,
            // witnesses, new outputs or mint spend instruction are sent.
            let response: CheckStateResponse = transport
                .http_post(
                    approved.join("v1/checkstate")?,
                    None,
                    &CheckStateRequest { ys: points.clone() },
                )
                .await
                .map_err(|_| anyhow::anyhow!("original mint custody lookup failed"))?;
            match_custody_states(&points, response)?
        };
        let mut reported = Vec::with_capacity(points.len());
        let mut amounts = [0u64; 4];
        for (proof, point) in self.proofs.iter().zip(points) {
            let state = states[&point];
            let bucket = match state {
                State::Unspent => 0,
                State::Spent => 1,
                _ => 2,
            };
            amounts[bucket] = amounts[bucket]
                .checked_add(proof.amount_sats)
                .context("custody amount overflow")?;
            if state == State::Spent && proof.local_state == "UNSPENT" {
                amounts[3] = amounts[3]
                    .checked_add(proof.amount_sats)
                    .context("stale custody amount overflow")?;
            }
            reported.push(json!({"y": proof.y, "amount_sats": proof.amount_sats,
                "local_state": proof.local_state, "mint_state": state}));
        }
        Ok(json!({
            "version": 1, "result": "CASHU_RESTORE_MINT_CUSTODY_OBSERVED",
            "checkpoint_id": self.checkpoint_id, "staged_receipt_sha256": self.staged_receipt_sha256,
            "mint_store_sha256": self.mint_store_sha256, "selection_sha256": digest,
            "mint_id": self.mint_id, "mint_url": approved, "total_candidates": self.total_candidates,
            "after_y": self.after_y, "next_after_y": self.next_after_y,
            "observed_at": SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs(),
            "queried_mint": !reported.is_empty(), "proofs": reported,
            "mint_unspent_sats": amounts[0], "mint_spent_sats": amounts[1],
            "mint_unresolved_sats": amounts[2], "stale_local_unspent_sats": amounts[3],
            "external_reconciliation_verified": false, "restore_authorized": false, "resume_authorized": false,
        }))
    }
}

/// Read one custody page without opening the wallet or issuing a mint spend.
pub async fn observe_custody(
    selection: &Path,
    expected_sha256: &str,
    mint_url: &str,
    output: &Path,
) -> Result<()> {
    let selection: CustodySelection = read_selection(selection, expected_sha256, output)?;
    let approved: MintUrl = mint_url.parse()?;
    let transport = MintTransport::new(mint_url.parse()?, Duration::from_secs(15), 1_048_576)?;
    let receipt = selection
        .observe(&approved, expected_sha256, &transport)
        .await?;
    write_observation(output, &receipt)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn custody_reply_rejects_missing_duplicate_and_foreign_points() -> Result<()> {
        let x = "79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798";
        let one: PublicKey = format!("02{x}").parse()?;
        let two: PublicKey = format!("03{x}").parse()?;
        let state = |y| (y, State::Spent).into();
        for states in [vec![], vec![state(one), state(one)], vec![state(two)]] {
            assert!(match_custody_states(&[one], CheckStateResponse { states }).is_err());
        }
        let exact = match_custody_states(
            &[one, two],
            CheckStateResponse {
                states: vec![(two, State::Pending).into(), state(one)],
            },
        )?;
        assert_eq!(exact[&one], State::Spent);
        assert_eq!(exact[&two], State::Pending);
        Ok(())
    }
}
