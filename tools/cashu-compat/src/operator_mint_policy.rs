//! Review an exact mint-policy candidate while the existing wallet writer is stopped.
use crate::{
    journal::cdk_storage::verify_mint_namespaces,
    service_config::{Config, read_protected},
    status::{open_readonly_journal, require_bound_wallet},
    writer_lock::WriterLock,
};
use anyhow::{Result, ensure};
use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::path::Path;
use uuid::Uuid;

#[derive(Serialize)]
pub struct Review {
    version: u8,
    wallet_instance: Uuid,
    wallet_epoch: Uuid,
    current_sha256: String,
    candidate_sha256: String,
    mints: Vec<MintReview>,
    applied: bool,
}

#[derive(Serialize)]
struct MintReview {
    mint_id: String,
    policy_version: String,
    receive_cashu: bool,
    receive_nutzap: bool,
    bolt11_exit_qualified: bool,
    retained_namespace: bool,
    uncredited_acquisitions: u64,
    open_treasury_instructions: u64,
}

fn check_scope(mut current: Value, mut candidate: Value) -> Result<()> {
    let current = current
        .as_object_mut()
        .expect("decoded wallet is an object");
    let candidate = candidate
        .as_object_mut()
        .expect("decoded wallet is an object");
    for field in ["receive_cashu", "receive_nutzap"] {
        ensure!(
            candidate.get(field) != Some(&Value::Bool(true))
                || current.get(field) == Some(&Value::Bool(true)),
            "mint policy cannot activate a paused receiving rail"
        );
        current.remove(field);
        candidate.remove(field);
    }
    // Permit a dependent receive switch to be paused when retiring the last
    // mint; retain all other runtime, publication and financial authority.
    for (section, flag) in [
        ("relay_intake", "enabled"),
        ("publication_signing", "announcements"),
        ("publication_delivery", "announcements"),
        ("treasury_intake", "accept_new"),
    ] {
        if candidate.get(section).and_then(|v| v.get(flag)) == Some(&Value::Bool(false))
            && let Some(original) = current.get_mut(section).and_then(Value::as_object_mut)
        {
            original.insert(flag.into(), Value::Bool(false));
        }
    }
    if let Some(endpoint) = candidate
        .get("public_request")
        .and_then(|v| v.get("endpoint"))
    {
        ensure!(
            current
                .get("public_request")
                .and_then(|v| v.get("endpoint"))
                == Some(endpoint),
            "mint policy changes must retain the public request transport"
        );
    }
    for field in [
        "mints",
        "keysets",
        "descriptors",
        "retired_descriptors",
        "public_request",
    ] {
        current.remove(field);
        candidate.remove(field);
    }
    ensure!(
        current == candidate,
        "candidate changes configuration outside mint policy"
    );
    Ok(())
}

fn check_retention(current: &Config, candidate: &Config) -> Result<()> {
    for old in &current.mints {
        ensure!(
            candidate
                .mints
                .iter()
                .any(|mint| mint.id == old.id && mint.advertised_url == old.advertised_url),
            "retire receiving while retaining the mint identity and recovery policy"
        );
    }
    let before = current.prepare()?;
    let after = candidate.prepare()?;
    ensure!(
        after.keys.preserves(&before.keys),
        "candidate drops or replaces pinned recovery keys"
    );
    if let Some(display) = &current.public_request {
        let old = current.prepare_request(&display.endpoint, display.encoding)?;
        let new = candidate
            .public_request
            .as_ref()
            .map(|display| candidate.prepare_request(&display.endpoint, display.encoding))
            .transpose()?;
        if new.as_ref().is_none_or(|request| request.id() != old.id()) {
            ensure!(
                !candidate.descriptors.iter().any(|id| id == old.id()),
                "replace the previous public request with an explicit retirement window or revoke it"
            );
        }
    }
    Ok(())
}

pub fn review(current_path: &Path, candidate_path: &Path) -> Result<Review> {
    let current_bytes = read_protected(current_path)?;
    let candidate_bytes = read_protected(candidate_path)?;
    let current = Config::decode(&current_bytes)?;
    let candidate = Config::decode(&candidate_bytes)?;
    check_scope(
        serde_json::from_slice(&current_bytes)?,
        serde_json::from_slice(&candidate_bytes)?,
    )?;
    check_retention(&current, &candidate)?;
    current.require_runtime_identity()?;
    let _writer = WriterLock::lock_existing(&current.state_directory)?;
    let keys = current.load_keys()?;
    let db = open_readonly_journal(&current)?;
    db.pragma_update(None, "trusted_schema", false)?;
    require_bound_wallet(&db, &current, &keys)?;
    let retained = verify_mint_namespaces(&db, &candidate.mints)?;
    let mut mints = Vec::new();
    for mint in candidate.mints {
        let uncredited_acquisitions = db.query_row(
            "SELECT COUNT(*) FROM acquisitions WHERE mint_id=? AND state!='credited'",
            [&mint.id],
            |row| row.get(0),
        )?;
        let open_treasury_instructions = db.query_row(
            "SELECT COUNT(*) FROM treasury_instructions WHERE state!='reconciled' AND json_extract(issued_json,'$.intent.mint_id')=?",
            [&mint.id],
            |row| row.get(0),
        )?;
        mints.push(MintReview {
            policy_version: mint.policy_version,
            receive_cashu: candidate.receive_cashu
                && mint.receive_cashu
                && mint.bolt11_exit_qualified,
            receive_nutzap: candidate.receive_nutzap
                && mint.receive_nutzap
                && mint.bolt11_exit_qualified,
            bolt11_exit_qualified: mint.bolt11_exit_qualified,
            retained_namespace: retained.contains_key(&mint.id),
            mint_id: mint.id,
            uncredited_acquisitions,
            open_treasury_instructions,
        });
    }
    ensure!(
        read_protected(current_path)? == current_bytes
            && read_protected(candidate_path)? == candidate_bytes,
        "configuration changed during mint policy review"
    );
    Ok(Review {
        version: 1,
        wallet_instance: current.wallet_instance,
        wallet_epoch: current.wallet_epoch,
        current_sha256: hex::encode(Sha256::digest(current_bytes)),
        candidate_sha256: hex::encode(Sha256::digest(candidate_bytes)),
        mints,
        applied: false,
    })
}
