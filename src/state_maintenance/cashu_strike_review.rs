//! Authoritative read of one treasury receive selected from a fenced checkpoint.
use super::files;
use crate::{
    cashu::treasury::TREASURY_USER, ledger::StoredStrikeReceiveRequest, strike::StrikeClient,
};
use anyhow::{Context, Result, ensure};
use lightning_goats_cashu_receipt::treasury::{Issued, MAX_TREASURY_BYTES, hex_digest};
use serde::Deserialize;
use serde_json::json;
use std::path::Path;
use uuid::Uuid;
use zeroize::Zeroizing;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Selection {
    version: u8,
    purpose: String,
    checkpoint_id: Uuid,
    staged_receipt_sha256: String,
    receive_id: Uuid,
    intent_digest: String,
    issued: Issued,
}

impl Selection {
    fn original_request(&self) -> Result<StoredStrikeReceiveRequest> {
        let issued = &self.issued;
        ensure!(
            self.version == 1
                && self.purpose == "cashu_restore_strike_receive"
                && !self.checkpoint_id.is_nil()
                && !self.receive_id.is_nil()
                && !issued.receive_request_id.is_nil()
                && self.intent_digest == hex_digest(issued.intent.digest()?),
            "restore selection differs from the original intent"
        );
        crate::nostr_event::validate_hex(&self.staged_receipt_sha256, 32)?;
        let amount_msat = issued
            .intent
            .deposit_sats
            .checked_mul(1000)
            .context("invalid restore invoice amount")?;
        ensure!(
            issued.invoice.len() <= 8192,
            "restore invoice exceeds bound"
        );
        crate::bolt11::verify(
            &issued.invoice,
            amount_msat,
            &issued.payment_hash,
            &self.intent_digest,
        )?;
        Ok(StoredStrikeReceiveRequest {
            receive_request_id: issued.receive_request_id,
            address_user: TREASURY_USER.into(),
            credit_pool: TREASURY_USER.into(),
            amount_msat,
            description_hash: self.intent_digest.clone(),
            payment_hash: issued.payment_hash.clone(),
            invoice: issued.invoice.clone(),
            created_provider: None,
        })
    }
}

/// Verify the selected original without opening a database or applying evidence.
pub async fn verify(
    selection: &Path,
    expected_sha256: &str,
    credential: &Path,
    strike_origin: &str,
    output: &Path,
) -> Result<()> {
    files::require_root()?;
    let parent = files::private_file_parent(output)?;
    ensure!(
        output
            .symlink_metadata()
            .is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound),
        "Strike observation output must be a new file"
    );
    let (selection, digest): (Selection, _) =
        files::review_selection(selection, expected_sha256, MAX_TREASURY_BYTES as u64)?;
    let original = selection.original_request()?;
    ensure!(
        selection.issued.intent.destination.origin == strike_origin,
        "reviewed Strike origin differs from the original"
    );
    files::private_file_parent(credential)?;
    let secret = Zeroizing::new(files::read_private(credential, 4096)?);
    let key = std::str::from_utf8(&secret)?.trim();
    let client = StrikeClient::new(strike_origin, key.to_owned())?;
    let receive = client
        .verify_cashu_treasury_receive(&selection.issued.intent, &original, selection.receive_id)
        .await?;
    let receipt = json!({
        "version": 1,
        "result": "CASHU_RESTORE_STRIKE_RECEIVE_VERIFIED",
        "checkpoint_id": selection.checkpoint_id,
        "staged_receipt_sha256": selection.staged_receipt_sha256,
        "selection_sha256": digest,
        "intent_id": selection.issued.intent.id,
        "intent_digest": selection.intent_digest,
        "observed_at": crate::gateway::now_epoch()?,
        "observation": receive.treasury_evidence()?,
        "external_reconciliation_verified": false,
        "restore_authorized": false,
        "resume_authorized": false,
    });
    let mut encoded = serde_json::to_vec(&receipt)?;
    encoded.push(b'\n');
    files::write_new(output, &encoded)?;
    files::sync_directory(parent)
}
