//! Protected observation of one originally submitted melt from a fenced checkpoint.
use crate::{
    backup_copy::{Frozen, protected_directory},
    melt_reconciliation::poll_original_quote,
    transport::MintTransport,
};
use anyhow::{Context, Result, ensure};
use cdk::{mint_url::MintUrl, nuts::MeltQuoteState, wallet::MeltQuote};
use lightning_goats_cashu_receipt::treasury::{Issued, MAX_TREASURY_BYTES, hex_digest};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
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

/// Uses the existing constrained HTTPS mint transport and writes one private
/// observation. No wallet database, secrets, proof reservations or CDK writer
/// are opened.
pub async fn observe(
    selection_path: &Path,
    expected_sha256: &str,
    mint_url: &str,
    output: &Path,
) -> Result<()> {
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
    let original: Selection = serde_json::from_slice(&bytes)?;
    let approved: MintUrl = mint_url.parse()?;
    let transport = MintTransport::new(mint_url.parse()?, Duration::from_secs(15), 1_048_576)?;
    let receipt = original.observe(&approved, &digest, &transport).await?;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(output)?;
    serde_json::to_writer(&mut file, &receipt)?;
    file.write_all(b"\n")?;
    file.sync_all()?;
    File::open(parent)?.sync_all()?;
    Ok(())
}
