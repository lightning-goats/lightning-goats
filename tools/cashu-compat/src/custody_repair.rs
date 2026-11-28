//! Prepare a separate CDK wallet image. Never open a live wallet for writing.
use crate::backup_copy::{Frozen, integrity, protected_directory};
use anyhow::{Context, Result, ensure};
use cdk::{mint_url::MintUrl, nuts::PublicKey};
use rusqlite::{Connection, OpenFlags, TransactionBehavior, params};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::{
    collections::BTreeSet,
    fs::{File, OpenOptions, Permissions},
    io::{Read, Seek, SeekFrom, Write},
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::Path,
};
use uuid::Uuid;

const MAX_STORE: u64 = 1024 * 1024 * 1024;
const MAX_JSON: u64 = 2 * 1024 * 1024;
const STORE: &str = "mint-copy.sqlite";
const OUTPUT: &str = "repaired";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Plan {
    version: u8,
    checkpoint_id: Uuid,
    observation_anchor_sha256: String,
    source_sha256: String,
    mint_id: String,
    mint_url: MintUrl,
    observation_sha256: Vec<String>,
    approved_points: BTreeSet<String>,
}

#[derive(Deserialize)]
struct Proof {
    y: String,
    amount_sats: u64,
    local_state: String,
    mint_state: String,
}

#[derive(Deserialize)]
struct Observation {
    version: u8,
    result: String,
    checkpoint_id: Uuid,
    staged_receipt_sha256: String,
    mint_store_sha256: String,
    mint_id: String,
    mint_url: MintUrl,
    total_candidates: usize,
    after_y: Option<String>,
    next_after_y: Option<String>,
    queried_mint: bool,
    proofs: Vec<Proof>,
    stale_local_unspent_sats: u64,
    external_reconciliation_verified: bool,
    restore_authorized: bool,
    resume_authorized: bool,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Receipt {
    version: u8,
    checkpoint_id: Uuid,
    mint_id: String,
    plan_sha256: String,
    source_sha256: String,
    repaired_sha256: String,
    retired_proofs: usize,
    retired_sats: u64,
    restore_authorized: bool,
    resume_authorized: bool,
}

fn read_bound<T: DeserializeOwned>(path: &Path, expected: Option<&str>) -> Result<T> {
    let mut input = Frozen::open(path, MAX_JSON, false)?;
    ensure!(
        expected.is_none_or(|hash| hash == input.sha256()),
        "reviewed evidence hash differs"
    );
    input.file.seek(SeekFrom::Start(0))?;
    let mut data = Vec::new();
    (&mut input.file)
        .take(MAX_JSON + 1)
        .read_to_end(&mut data)?;
    ensure!(data.len() as u64 <= MAX_JSON, "evidence exceeds bound");
    input.unchanged(path, MAX_JSON, false)?;
    Ok(serde_json::from_slice(&data)?)
}

fn no_sidecars(path: &Path) -> Result<()> {
    for suffix in ["-wal", "-shm", "-journal"] {
        let mut sidecar = path.as_os_str().to_os_string();
        sidecar.push(suffix);
        ensure!(
            std::fs::symlink_metadata(sidecar)
                .is_err_and(|error| error.kind() == std::io::ErrorKind::NotFound),
            "repair requires a standalone copy"
        );
    }
    Ok(())
}

fn repair(connection: &mut Connection, directory: &Path, plan: &Plan) -> Result<(usize, u64)> {
    connection.execute_batch(
        "PRAGMA trusted_schema=OFF; PRAGMA foreign_keys=ON; PRAGMA synchronous=FULL;",
    )?;
    integrity(connection)?;
    let namespace: (String, String) = connection.query_row(
        "SELECT mint_id,mint_url FROM lg_mint_namespace WHERE singleton=1",
        [],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    ensure!(
        namespace.0 == plan.mint_id && namespace.1.parse::<MintUrl>()? == plan.mint_url,
        "repair namespace differs"
    );
    let triggers: bool = connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE type='trigger' AND tbl_name='proof')",
        [],
        |row| row.get(0),
    )?;
    ensure!(!triggers, "unknown proof update trigger");
    let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let retired = {
        let mut query = tx.prepare("SELECT y,amount,state,mint_url,unit,used_by_operation FROM proof WHERE state IS NOT 'SPENT' ORDER BY y")?;
        let mut rows = query.query([])?;
        let mut retired = Vec::new();
        let mut approved = plan.approved_points.clone();
        let mut previous = None;
        let mut count = 0usize;
        let total = tx.query_row(
            "SELECT count(*) FROM proof WHERE state IS NOT 'SPENT'",
            [],
            |row| row.get::<_, usize>(0),
        )?;
        for (index, expected) in plan.observation_sha256.iter().enumerate() {
            let report: Observation = read_bound(
                &directory.join(format!("observation-{index}.json")),
                Some(expected),
            )?;
            let remaining = total
                .checked_sub(count)
                .context("custody coverage overflow")?;
            let page_size = remaining.min(128);
            ensure!(
                report.version == 1
                    && report.result == "CASHU_RESTORE_MINT_CUSTODY_OBSERVED"
                    && report.checkpoint_id == plan.checkpoint_id
                    && report.staged_receipt_sha256 == plan.observation_anchor_sha256
                    && report.mint_store_sha256 == plan.source_sha256
                    && report.mint_id == plan.mint_id
                    && report.mint_url == plan.mint_url
                    && report.total_candidates == total
                    && report.after_y == previous
                    && report.proofs.len() == page_size
                    && report.queried_mint == (page_size != 0)
                    && !report.restore_authorized
                    && !report.resume_authorized
                    && !report.external_reconciliation_verified,
                "observation differs from the scoped snapshot"
            );
            ensure!(
                (index + 1 == plan.observation_sha256.len()) == (remaining <= 128),
                "missing or extra custody page"
            );
            let mut page_sats = 0u64;
            for proof in &report.proofs {
                let point: PublicKey = proof.y.parse()?;
                let row = rows.next()?.context("observation has an unrelated proof")?;
                let stored_y: Vec<u8> = row.get(0)?;
                let stored_mint: String = row.get(3)?;
                ensure!(
                    proof.y == point.to_string()
                        && stored_y == point.to_bytes()
                        && proof.amount_sats != 0
                        && proof.amount_sats == row.get::<_, u64>(1)?
                        && proof.local_state == row.get::<_, String>(2)?
                        && stored_mint.parse::<MintUrl>()? == plan.mint_url
                        && row.get::<_, String>(4)? == "sat",
                    "observation proof differs from the original row"
                );
                ensure!(
                    matches!(
                        proof.mint_state.as_str(),
                        "SPENT" | "UNSPENT" | "PENDING" | "RESERVED" | "PENDING_SPENT"
                    ),
                    "invalid observed mint state"
                );
                if proof.local_state == "UNSPENT" && proof.mint_state == "SPENT" {
                    ensure!(
                        row.get::<_, Option<String>>(5)?.is_none(),
                        "observed proof belongs to an operation"
                    );
                    ensure!(
                        approved.remove(&proof.y),
                        "observed spent proof has no repair approval"
                    );
                    page_sats = page_sats
                        .checked_add(proof.amount_sats)
                        .context("page sats overflow")?;
                    retired.push((stored_y, proof.amount_sats));
                }
            }
            let next = report
                .proofs
                .last()
                .map(|proof| proof.y.clone())
                .filter(|_| remaining > 128);
            ensure!(
                report.next_after_y == next && report.stale_local_unspent_sats == page_sats,
                "observation cursor or repair total differs"
            );
            previous = next;
            count += page_size;
        }
        ensure!(
            rows.next()?.is_none() && count == total && approved.is_empty(),
            "custody evidence omits original or approved proofs"
        );
        retired
    };
    let mut sats = 0u64;
    for (point, amount) in &retired {
        ensure!(tx.execute("UPDATE proof SET state='SPENT' WHERE y=?1 AND amount=?2 AND state='UNSPENT' AND used_by_operation IS NULL",
                           params![point, amount])? == 1, "repair proof changed");
        sats = sats.checked_add(*amount).context("retired sats overflow")?;
    }
    integrity(&tx)?;
    tx.commit()?;
    Ok((retired.len(), sats))
}

/// The digest is independently approved. Write only to a new offline copy and
/// preserve the proof rows and their original operation references.
pub fn prepare(directory: &Path, plan_sha256: &str) -> Result<Receipt> {
    ensure!(
        rustix::process::geteuid().as_raw() == 0,
        "root copy owner required"
    );
    protected_directory(directory)?;
    let plan: Plan = read_bound(&directory.join("REPAIR-PLAN.json"), Some(plan_sha256))?;
    ensure!(
        plan.version == 1
            && !plan.checkpoint_id.is_nil()
            && plan.observation_sha256.len() <= 1000
            && !plan.observation_sha256.is_empty()
            && !plan.mint_id.is_empty()
            && !plan.approved_points.is_empty()
            && plan.approved_points.len() <= plan.observation_sha256.len() * 128,
        "invalid custody repair plan"
    );
    let source = directory.join(STORE);
    no_sidecars(&source)?;
    let mut frozen = Frozen::open(&source, MAX_STORE, false)?;
    ensure!(
        frozen.sha256() == plan.source_sha256,
        "original mint copy differs from plan"
    );
    let destination = directory.join(OUTPUT);
    if destination.exists() {
        protected_directory(&destination)?;
        for (index, expected) in plan.observation_sha256.iter().enumerate() {
            ensure!(
                Frozen::open(
                    &directory.join(format!("observation-{index}.json")),
                    MAX_JSON,
                    false
                )?
                .sha256()
                    == expected,
                "retry observation differs from plan"
            );
        }
        let receipt: Receipt = read_bound(&destination.join("REPAIR.json"), None)?;
        let repaired = destination.join(STORE);
        no_sidecars(&repaired)?;
        ensure!(
            receipt.version == 1
                && receipt.plan_sha256 == plan_sha256
                && receipt.checkpoint_id == plan.checkpoint_id
                && receipt.mint_id == plan.mint_id
                && receipt.source_sha256 == plan.source_sha256
                && !receipt.restore_authorized
                && !receipt.resume_authorized
                && Frozen::open(&repaired, MAX_STORE, false)?.sha256() == receipt.repaired_sha256,
            "existing repair is not an exact retry"
        );
        return Ok(receipt);
    }
    let work = tempfile::Builder::new()
        .prefix(".repair-")
        .permissions(Permissions::from_mode(0o700))
        .tempdir_in(directory)?;
    let repaired = work.path().join(STORE);
    {
        let mut output = OpenOptions::new()
            .create_new(true)
            .write(true)
            .mode(0o600)
            .open(&repaired)?;
        frozen.file.seek(SeekFrom::Start(0))?;
        ensure!(
            std::io::copy(&mut (&mut frozen.file).take(MAX_STORE + 1), &mut output)? <= MAX_STORE,
            "original store exceeded copy size limit"
        );
        output.sync_all()?;
    }
    frozen.unchanged(&source, MAX_STORE, false)?;
    let (retired_proofs, retired_sats) = {
        let mut db = Connection::open_with_flags(&repaired, OpenFlags::SQLITE_OPEN_READ_WRITE)?;
        repair(&mut db, directory, &plan)?
    };
    no_sidecars(&repaired)?;
    File::open(&repaired)?.sync_all()?;
    let receipt = Receipt {
        version: 1,
        checkpoint_id: plan.checkpoint_id,
        mint_id: plan.mint_id,
        plan_sha256: plan_sha256.to_owned(),
        source_sha256: plan.source_sha256,
        repaired_sha256: Frozen::open(&repaired, MAX_STORE, false)?
            .sha256()
            .to_owned(),
        retired_proofs,
        retired_sats,
        restore_authorized: false,
        resume_authorized: false,
    };
    let mut report = OpenOptions::new()
        .create_new(true)
        .write(true)
        .mode(0o600)
        .open(work.path().join("REPAIR.json"))?;
    serde_json::to_writer(&mut report, &receipt)?;
    report.write_all(b"\n")?;
    report.sync_all()?;
    File::open(work.path())?.sync_all()?;
    rustix::fs::renameat_with(
        rustix::fs::CWD,
        work.path(),
        rustix::fs::CWD,
        &destination,
        rustix::fs::RenameFlags::NOREPLACE,
    )?;
    File::open(directory)?.sync_all()?;
    Ok(receipt)
}
