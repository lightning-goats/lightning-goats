//! Read the authoritative outcome of one retained feeder request.
use super::files;
use crate::gateway::{FeedOutcome, GatewayClient};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::path::Path;
use uuid::Uuid;

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum CheckpointStatus {
    IntentCommitted,
    Confirmed,
    Unknown,
    ReconciledNotFed,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Selection {
    version: u8,
    purpose: String,
    checkpoint_id: Uuid,
    staged_receipt_sha256: String,
    request_id: Uuid,
    checkpoint_status: CheckpointStatus,
    threshold_sats: u64,
}

/// Observe the original UUID. The gateway can retain an acknowledgment while
/// answering its status GET, and this path never dispatches a feeder command.
pub async fn observe(
    selection: &Path,
    expected_sha256: &str,
    gateway_origin: &str,
    output: &Path,
) -> Result<()> {
    files::require_root()?;
    let parent = files::private_file_parent(output)?;
    ensure!(
        output
            .symlink_metadata()
            .is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound),
        "feeder observation output must be a new file"
    );
    let (selection, digest): (Selection, _) =
        files::review_selection(selection, expected_sha256, 4096)?;
    ensure!(
        selection.version == 1
            && selection.purpose == "cashu_restore_feeder_request"
            && !selection.checkpoint_id.is_nil()
            && !selection.request_id.is_nil()
            && selection.threshold_sats != 0,
        "invalid original feeder selection"
    );
    crate::nostr_event::validate_hex(&selection.staged_receipt_sha256, 32)?;
    let gateway = GatewayClient::new(gateway_origin)?;
    let observation = gateway.feed_request_status(selection.request_id).await?;
    let checkpoint_conflict = matches!(
        (&selection.checkpoint_status, observation.status),
        (CheckpointStatus::Confirmed, FeedOutcome::NotDispatched)
            | (CheckpointStatus::ReconciledNotFed, FeedOutcome::Confirmed)
    );
    let mut encoded = serde_json::to_vec(&json!({
        "version": 1,
        "result": "CASHU_RESTORE_FEEDER_REQUEST_OBSERVED",
        "checkpoint_id": selection.checkpoint_id,
        "staged_receipt_sha256": selection.staged_receipt_sha256,
        "selection_sha256": digest,
        "gateway_origin": gateway_origin,
        "checkpoint_status": selection.checkpoint_status,
        "checkpoint_conflict": checkpoint_conflict,
        "threshold_sats": selection.threshold_sats,
        "observed_at": crate::gateway::now_epoch()?,
        "observation": observation,
        "external_reconciliation_verified": false,
        "restore_authorized": false,
        "resume_authorized": false,
    }))?;
    encoded.push(b'\n');
    files::write_new(output, &encoded)?;
    files::sync_directory(parent)
}
