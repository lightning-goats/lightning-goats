//! Durable shared intake index. Call only after policy and cryptographic validation.
//! No token, relay notification or wallet balance alone establishes acquisition.

use std::{collections::BTreeMap, path::Path};

use anyhow::{Result, bail, ensure};
use cdk::nuts::Proof;
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use sha2::{Digest, Sha256};
use uuid::Uuid;

pub mod custody;
mod publication;
pub mod publication_delivery;
pub mod relay_scans;
mod scheduling;
mod treasury;
mod treasury_budget;
pub(crate) mod treasury_cleanup;
pub(crate) mod treasury_scheduling;
mod treasury_selection;
pub mod treasury_split;
pub use scheduling::Work;

// Shared by ordinary Cashu and nutzap intake. Include held/exhausted work:
// scheduling exhaustion does not establish ownership or release a proof claim.
const MAX_PENDING_ACQUISITIONS: u64 = 1024;
const MAX_PENDING_PROOF_BYTES: u64 = 16 * 1024 * 1024;
const MAX_INTAKE_SOURCES_PER_ACQUISITION: u64 = 64;

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS relay_scans (
    scope BLOB NOT NULL, relay TEXT NOT NULL, lane TEXT NOT NULL,
    floor INTEGER NOT NULL, span INTEGER NOT NULL, overlap INTEGER NOT NULL,
    cursor INTEGER NOT NULL, horizon INTEGER NOT NULL, width INTEGER NOT NULL,
    claim TEXT, scan_since INTEGER, scan_until INTEGER,
    next_attempt INTEGER NOT NULL DEFAULT 0,
    outcome TEXT NOT NULL DEFAULT 'unscanned',
    PRIMARY KEY(scope,relay,lane),
    CHECK(lane IN ('recent','history')),
    CHECK(floor>=0 AND span>0 AND overlap>=0 AND overlap<span),
    CHECK(cursor>=floor AND horizon>=floor AND width>0 AND width<=span)
);
CREATE TABLE IF NOT EXISTS relay_scan_results (
    scope BLOB NOT NULL, relay TEXT NOT NULL, lane TEXT NOT NULL,
    admitted INTEGER NOT NULL CHECK(admitted>=0),
    rejected INTEGER NOT NULL CHECK(rejected>=0 AND admitted+rejected<=256),
    PRIMARY KEY(scope,relay,lane),
    FOREIGN KEY(scope,relay,lane) REFERENCES relay_scans(scope,relay,lane)
);
CREATE TABLE IF NOT EXISTS nostr_publications (
    id TEXT PRIMARY KEY, intent BLOB NOT NULL, ciphertext TEXT,
    signed_event TEXT, published INTEGER NOT NULL DEFAULT 0 CHECK(published IN (0,1)),
    CHECK(published=0 OR signed_event IS NOT NULL)
);
CREATE TABLE IF NOT EXISTS publication_signing_schedule (
    publication_id TEXT PRIMARY KEY REFERENCES nostr_publications(id),
    attempts INTEGER NOT NULL CHECK(attempts>0),
    next_attempt INTEGER NOT NULL CHECK(next_attempt>=0)
);
CREATE TABLE IF NOT EXISTS publication_routes (
    publication_id TEXT PRIMARY KEY REFERENCES nostr_publications(id),
    targets BLOB NOT NULL, signed_list TEXT, planned_at INTEGER NOT NULL CHECK(planned_at>=0)
);
CREATE TABLE IF NOT EXISTS publication_routing_schedule (
    publication_id TEXT PRIMARY KEY REFERENCES nostr_publications(id),
    attempts INTEGER NOT NULL CHECK(attempts>0),
    next_attempt INTEGER NOT NULL CHECK(next_attempt>=0)
);
CREATE TABLE IF NOT EXISTS publication_delivery (
    publication_id TEXT NOT NULL REFERENCES publication_routes(publication_id),
    relay TEXT NOT NULL, attempts INTEGER NOT NULL DEFAULT 0 CHECK(attempts>=0),
    next_attempt INTEGER NOT NULL DEFAULT 0 CHECK(next_attempt>=0),
    acknowledged INTEGER NOT NULL DEFAULT 0 CHECK(acknowledged IN (0,1)),
    PRIMARY KEY(publication_id,relay)
);
CREATE TABLE IF NOT EXISTS runtime_registration (
    singleton INTEGER PRIMARY KEY CHECK(singleton=1),
    instance TEXT NOT NULL, key_binding BLOB NOT NULL
);
CREATE TABLE IF NOT EXISTS treasury_instructions (
    id TEXT PRIMARY KEY, issued_json BLOB NOT NULL,
    receive_id TEXT NOT NULL UNIQUE, payment_hash TEXT NOT NULL UNIQUE,
    state TEXT NOT NULL DEFAULT 'bound' CHECK(state IN ('bound','prepared','submitted','reconciled'))
);
CREATE TABLE IF NOT EXISTS wallet_epoch (
    singleton INTEGER PRIMARY KEY CHECK(singleton=1), epoch TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS acquisitions (
    id TEXT PRIMARY KEY, mint_id TEXT NOT NULL, policy TEXT NOT NULL,
    transport TEXT NOT NULL CHECK(transport IN ('cashu','nutzap')),
    source_ref TEXT NOT NULL, proof_count INTEGER NOT NULL CHECK(proof_count>0),
    gross INTEGER NOT NULL CHECK(gross>0),
    state TEXT NOT NULL DEFAULT 'validated' CHECK(state IN
      ('validated','acquisition_prepared','acquiring','acquired',
       'credit_export_pending','credited','held_for_reconciliation')),
    request BLOB, acquired_evidence BLOB, receive_fee INTEGER, net INTEGER,
    core_ack BLOB,
    CHECK(net IS NULL OR (net>0 AND receive_fee>=0 AND net+receive_fee=gross))
);
CREATE TABLE IF NOT EXISTS receive_status_capabilities (
 acquisition_id TEXT PRIMARY KEY REFERENCES acquisitions(id),
 capability TEXT NOT NULL UNIQUE CHECK(length(capability)=64)
);
CREATE TABLE IF NOT EXISTS acquisition_admission_times (
 acquisition_id TEXT PRIMARY KEY REFERENCES acquisitions(id),
 admitted_at INTEGER NOT NULL CHECK(admitted_at>=0)
);
CREATE TRIGGER IF NOT EXISTS receive_status_capabilities_no_update
 BEFORE UPDATE ON receive_status_capabilities BEGIN SELECT RAISE(ABORT,'retained receipt capability'); END;
CREATE TRIGGER IF NOT EXISTS receive_status_capabilities_no_delete
 BEFORE DELETE ON receive_status_capabilities BEGIN SELECT RAISE(ABORT,'retained receipt capability'); END;

CREATE TABLE IF NOT EXISTS acquisition_schedule (
    acquisition_id TEXT PRIMARY KEY REFERENCES acquisitions(id),
    attempts INTEGER NOT NULL CHECK(attempts>0),
    next_attempt INTEGER NOT NULL CHECK(next_attempt>=0)
);
CREATE TABLE IF NOT EXISTS receipt_delivery_schedule (
    acquisition_id TEXT PRIMARY KEY REFERENCES acquisitions(id),
    attempts INTEGER NOT NULL CHECK(attempts>0),
    next_attempt INTEGER NOT NULL CHECK(next_attempt>=0)
);
CREATE TABLE IF NOT EXISTS owned_output_sources (
    mint_id TEXT NOT NULL, y TEXT NOT NULL, c TEXT NOT NULL,
    amount INTEGER NOT NULL CHECK(amount>0),
    acquisition_id TEXT NOT NULL REFERENCES acquisitions(id),
    PRIMARY KEY(mint_id,y)
);
CREATE TABLE IF NOT EXISTS treasury_cdk_cleanup (
    intent_id TEXT PRIMARY KEY REFERENCES treasury_results(intent_id),
    operation_id TEXT NOT NULL UNIQUE,
    report_digest BLOB NOT NULL CHECK(length(report_digest)=32)
);
CREATE TABLE IF NOT EXISTS treasury_work_schedule (
    intent_id TEXT NOT NULL REFERENCES treasury_instructions(id), action TEXT NOT NULL,
    attempts INTEGER NOT NULL CHECK(attempts>0), next_attempt INTEGER NOT NULL CHECK(next_attempt>=0),
    PRIMARY KEY(intent_id,action)
);
CREATE TABLE IF NOT EXISTS treasury_execution_admissions (
    intent_id TEXT PRIMARY KEY REFERENCES treasury_instructions(id),
    admitted_at INTEGER NOT NULL CHECK(admitted_at>=0), input_sats INTEGER NOT NULL CHECK(input_sats>0)
);
CREATE TABLE IF NOT EXISTS treasury_preparation_claims (
    intent_id TEXT PRIMARY KEY REFERENCES treasury_instructions(id)
);
CREATE TABLE IF NOT EXISTS treasury_preparation_operations (
    intent_id TEXT PRIMARY KEY REFERENCES treasury_preparation_claims(intent_id),
    operation_id TEXT NOT NULL UNIQUE
);
CREATE TRIGGER IF NOT EXISTS treasury_preparation_operation_no_update
 BEFORE UPDATE ON treasury_preparation_operations BEGIN SELECT RAISE(ABORT,'retained treasury preparation operation'); END;
CREATE TRIGGER IF NOT EXISTS treasury_preparation_operation_no_delete
 BEFORE DELETE ON treasury_preparation_operations BEGIN SELECT RAISE(ABORT,'retained treasury preparation operation'); END;
CREATE TABLE IF NOT EXISTS treasury_prepared (
    intent_id TEXT PRIMARY KEY REFERENCES treasury_instructions(id),
    operation_id TEXT NOT NULL UNIQUE, prepared_json BLOB NOT NULL
);
CREATE TABLE IF NOT EXISTS treasury_melt_requests (
    intent_id TEXT PRIMARY KEY REFERENCES treasury_prepared(intent_id),
    request BLOB NOT NULL, derivation BLOB NOT NULL
);
CREATE TABLE IF NOT EXISTS treasury_melt_replies (
    intent_id TEXT PRIMARY KEY REFERENCES treasury_melt_requests(intent_id),
    reply BLOB NOT NULL
);
CREATE TABLE IF NOT EXISTS treasury_restore_evidence (
    intent_id TEXT PRIMARY KEY REFERENCES treasury_melt_requests(intent_id),
    paid BLOB NOT NULL, response BLOB NOT NULL
);
CREATE TABLE IF NOT EXISTS treasury_results (
    intent_id TEXT PRIMARY KEY REFERENCES treasury_melt_requests(intent_id),
    report BLOB NOT NULL, paid_evidence BLOB NOT NULL
);
CREATE TABLE IF NOT EXISTS treasury_fee_completions (
    intent_id TEXT PRIMARY KEY REFERENCES treasury_results(intent_id),
    completed_at INTEGER NOT NULL CHECK(completed_at>=0),
    fee_sats INTEGER NOT NULL CHECK(fee_sats>=0)
);
CREATE TRIGGER IF NOT EXISTS treasury_fee_completions_no_update
 BEFORE UPDATE ON treasury_fee_completions BEGIN SELECT RAISE(ABORT,'retained treasury fee'); END;
CREATE TRIGGER IF NOT EXISTS treasury_fee_completions_no_delete
 BEFORE DELETE ON treasury_fee_completions BEGIN SELECT RAISE(ABORT,'retained treasury fee'); END;
CREATE TABLE IF NOT EXISTS treasury_change_outputs (
    mint_id TEXT NOT NULL, y TEXT NOT NULL, c TEXT NOT NULL,
    amount INTEGER NOT NULL CHECK(amount>0),
    intent_id TEXT NOT NULL REFERENCES treasury_results(intent_id),
    PRIMARY KEY(mint_id,y)
);
CREATE TABLE IF NOT EXISTS treasury_change_source_parts (
    mint_id TEXT NOT NULL, y TEXT NOT NULL, acquisition_id TEXT NOT NULL,
    amount INTEGER NOT NULL CHECK(amount>0),
    PRIMARY KEY(mint_id,y,acquisition_id),
    FOREIGN KEY(mint_id,y) REFERENCES treasury_change_outputs(mint_id,y)
);
CREATE TABLE IF NOT EXISTS treasury_source_results (
    intent_id TEXT NOT NULL REFERENCES treasury_results(intent_id),
    acquisition_id TEXT NOT NULL,
    input_sats INTEGER NOT NULL CHECK(input_sats>0),
    change_sats INTEGER NOT NULL CHECK(change_sats>=0 AND change_sats<=input_sats),
    PRIMARY KEY(intent_id,acquisition_id)
);
CREATE TABLE IF NOT EXISTS treasury_split_execution_claims (
    intent_id TEXT PRIMARY KEY REFERENCES treasury_split_reservations(intent_id),
    claimed_at INTEGER NOT NULL CHECK(claimed_at>=0),
    reservation_digest BLOB NOT NULL CHECK(length(reservation_digest)=32)
);
CREATE TABLE IF NOT EXISTS treasury_split_submissions (
    intent_id TEXT PRIMARY KEY REFERENCES treasury_split_requests(intent_id),
    submitted_at INTEGER NOT NULL CHECK(submitted_at>=0),
    destination TEXT NOT NULL,
    request_digest BLOB NOT NULL CHECK(length(request_digest)=32),
    melt_input_fee_sats INTEGER NOT NULL CHECK(melt_input_fee_sats>=0)
);
CREATE TABLE IF NOT EXISTS treasury_split_replies (
    intent_id TEXT PRIMARY KEY REFERENCES treasury_split_submissions(intent_id),
    reply BLOB NOT NULL CHECK(length(reply)<=1048576)
);
CREATE TABLE IF NOT EXISTS treasury_split_results (
    intent_id TEXT PRIMARY KEY REFERENCES treasury_split_requests(intent_id),
    frozen_digest BLOB NOT NULL CHECK(length(frozen_digest)=32),
    input_sats INTEGER NOT NULL CHECK(input_sats>0),
    fee_sats INTEGER NOT NULL CHECK(fee_sats>=0 AND fee_sats<input_sats),
    result BLOB NOT NULL CHECK(length(result)<=1048576)
);
CREATE TABLE IF NOT EXISTS treasury_split_outputs (
    mint_id TEXT NOT NULL, y TEXT NOT NULL, c TEXT NOT NULL,
    amount INTEGER NOT NULL CHECK(amount>0),
    acquisition_id TEXT NOT NULL,
    purpose TEXT NOT NULL CHECK(purpose IN ('treasury','remainder')),
    intent_id TEXT NOT NULL REFERENCES treasury_split_results(intent_id),
    PRIMARY KEY(mint_id,y)
);
CREATE TABLE IF NOT EXISTS treasury_split_spent_inputs (
    mint_id TEXT NOT NULL, y TEXT NOT NULL,
    intent_id TEXT NOT NULL REFERENCES treasury_split_results(intent_id),
    PRIMARY KEY(mint_id,y)
);
DROP VIEW IF EXISTS treasury_owned_outputs;
CREATE VIEW treasury_owned_outputs AS
    SELECT o.mint_id,o.y,o.c,o.amount FROM (
        SELECT mint_id,y,c,amount FROM owned_output_sources
        UNION ALL SELECT mint_id,y,c,amount FROM treasury_change_outputs
        UNION ALL SELECT mint_id,y,c,amount FROM treasury_split_outputs
    ) o WHERE NOT EXISTS(SELECT 1 FROM treasury_split_spent_inputs s WHERE s.mint_id=o.mint_id AND s.y=o.y);
DROP VIEW IF EXISTS treasury_output_parts;
CREATE VIEW treasury_output_parts AS
    SELECT o.mint_id,o.y,o.acquisition_id,o.amount FROM (
        SELECT mint_id,y,acquisition_id,amount FROM owned_output_sources
        UNION ALL SELECT mint_id,y,acquisition_id,amount FROM treasury_change_source_parts
        UNION ALL SELECT mint_id,y,acquisition_id,amount FROM treasury_split_outputs
    ) o WHERE NOT EXISTS(SELECT 1 FROM treasury_split_spent_inputs s WHERE s.mint_id=o.mint_id AND s.y=o.y);
CREATE TABLE IF NOT EXISTS treasury_quote_requests (
    intent_id TEXT PRIMARY KEY REFERENCES treasury_instructions(id),
    destination TEXT NOT NULL, request BLOB NOT NULL, reply BLOB
);
CREATE TABLE IF NOT EXISTS treasury_quotes (
    intent_id TEXT PRIMARY KEY REFERENCES treasury_instructions(id),
    mint_id TEXT NOT NULL, quote_id TEXT NOT NULL, quote_json BLOB NOT NULL,
    UNIQUE(mint_id,quote_id)
);
CREATE TABLE IF NOT EXISTS treasury_split_reservations (
    intent_id TEXT PRIMARY KEY REFERENCES treasury_instructions(id),
    split_id TEXT NOT NULL UNIQUE,
    reservation BLOB NOT NULL CHECK(length(reservation)<=1048576)
);
CREATE TABLE IF NOT EXISTS treasury_split_admissions (
    intent_id TEXT PRIMARY KEY REFERENCES treasury_split_reservations(intent_id),
    admitted_at INTEGER NOT NULL CHECK(admitted_at>=0),
    input_sats INTEGER NOT NULL CHECK(input_sats>0),
    fee_sats INTEGER NOT NULL CHECK(fee_sats>=0),
    reservation_digest BLOB NOT NULL CHECK(length(reservation_digest)=32)
);
CREATE TABLE IF NOT EXISTS treasury_split_requests (
    intent_id TEXT PRIMARY KEY REFERENCES treasury_split_reservations(intent_id),
    cdk_operation TEXT NOT NULL UNIQUE,
    frozen BLOB NOT NULL CHECK(length(frozen)<=2097152)
);
CREATE TABLE IF NOT EXISTS treasury_proof_selections (
    intent_id TEXT PRIMARY KEY REFERENCES treasury_instructions(id), selection BLOB NOT NULL
);
CREATE TABLE IF NOT EXISTS treasury_proof_holds (
    mint_id TEXT NOT NULL, y TEXT NOT NULL,
    intent_id TEXT NOT NULL REFERENCES treasury_instructions(id),
    PRIMARY KEY(mint_id,y)
);
CREATE TABLE IF NOT EXISTS treasury_source_holds (
    intent_id TEXT NOT NULL REFERENCES treasury_instructions(id),
    acquisition_id TEXT NOT NULL REFERENCES acquisitions(id),
    input_sats INTEGER NOT NULL CHECK(input_sats>0),
    PRIMARY KEY(intent_id,acquisition_id)
);
CREATE INDEX IF NOT EXISTS treasury_source_holds_acquisition ON treasury_source_holds(acquisition_id);
CREATE TABLE IF NOT EXISTS proof_index (
    mint_id TEXT NOT NULL, unit TEXT NOT NULL CHECK(unit='sat'), y BLOB NOT NULL,
    evidence BLOB NOT NULL, acquisition_id TEXT NOT NULL REFERENCES acquisitions(id),
    PRIMARY KEY(mint_id,unit,y)
);
CREATE TABLE IF NOT EXISTS swap_derivations (
    acquisition_id TEXT PRIMARY KEY REFERENCES acquisitions(id), material BLOB NOT NULL
);
CREATE TABLE IF NOT EXISTS acquisition_context (
    acquisition_id TEXT PRIMARY KEY REFERENCES acquisitions(id),
    proofs BLOB NOT NULL, source_author TEXT
);
CREATE TABLE IF NOT EXISTS operation_holds (
    acquisition_id TEXT PRIMARY KEY REFERENCES acquisitions(id),
    previous_state TEXT NOT NULL, reason TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS operation_replies (
    acquisition_id TEXT PRIMARY KEY REFERENCES acquisitions(id), reply BLOB NOT NULL
);
CREATE TABLE IF NOT EXISTS intake_sources (
    transport TEXT NOT NULL, source_ref TEXT NOT NULL,
    acquisition_id TEXT NOT NULL REFERENCES acquisitions(id),
    PRIMARY KEY(transport,source_ref)
);
"#;

/// Already validated by the common intake engine. This type cannot attest mint payment.
/// Never log this value: proofs contain secrets. No Debug implementation deliberately.
pub struct ValidatedIntake<'a> {
    pub mint_id: &'a str,
    pub policy_version: &'a str,
    pub transport: Transport,
    /// HTTP admission ID or verified Nostr event ID; never a reusable descriptor ID.
    pub source_ref: &'a str,
    pub source_author: Option<&'a str>,
    pub proofs: &'a [Proof],
}

#[derive(Clone, Copy)]
pub enum Transport {
    Cashu,
    Nutzap,
}
impl Transport {
    fn as_str(self) -> &'static str {
        match self {
            Self::Cashu => "cashu",
            Self::Nutzap => "nutzap",
        }
    }
}

#[derive(Clone, Copy)]
pub enum HoldReason {
    AmbiguousAcquisition,
    InvalidMintResponse,
    MissingOwnedOutputs,
    ConservationMismatch,
}
impl HoldReason {
    fn as_str(self) -> &'static str {
        match self {
            Self::AmbiguousAcquisition => "ambiguous_acquisition",
            Self::InvalidMintResponse => "invalid_mint_response",
            Self::MissingOwnedOutputs => "missing_owned_outputs",
            Self::ConservationMismatch => "conservation_mismatch",
        }
    }
}

/// Public proof/source conflict; distinct from storage or internal consistency failures.
#[derive(Debug)]
pub struct AdmissionConflict;
impl std::fmt::Display for AdmissionConflict {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("proof or source admission conflict")
    }
}
impl std::error::Error for AdmissionConflict {}

#[derive(Debug, PartialEq, Eq)]
pub enum Admission {
    New(Uuid),
    Duplicate(Uuid),
}

pub struct Journal {
    connection: Connection,
    max_proofs: usize,
    custody_limits: Option<custody::CustodyLimits>,
}

impl Journal {
    /// Explicit initialization alone may establish this immutable startup binding.
    pub(crate) fn runtime_registration(
        &mut self,
        instance: Uuid,
        key_binding: &[u8; 32],
        initialize: bool,
    ) -> Result<()> {
        ensure!(!instance.is_nil(), "invalid wallet instance");
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        if initialize {
            tx.execute(
                "INSERT INTO runtime_registration(singleton,instance,key_binding) VALUES(1,?,?)",
                params![instance.to_string(), key_binding.as_slice()],
            )?;
        }
        let registered: Option<(String, Vec<u8>)> = tx
            .query_row(
                "SELECT instance,key_binding FROM runtime_registration WHERE singleton=1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        ensure!(
            registered
                .is_some_and(|(id, binding)| id == instance.to_string() && binding == key_binding),
            "wallet runtime registration missing or mismatched; reconcile before startup"
        );
        tx.commit()?;
        Ok(())
    }
    /// Caller owns the protected wallet directory and its access policy. An epoch
    /// mismatch fences startup; restoring a backup never silently resets this ID.
    pub fn open(path: &Path, expected_epoch: Uuid, max_proofs: usize) -> Result<Self> {
        ensure!(max_proofs > 0, "invalid proof limit");
        ensure!(!expected_epoch.is_nil(), "wallet epoch must be provisioned");
        let _private_file = crate::writer_lock::private_state_file(path)?;
        let mut connection = Connection::open(path)?;
        connection.execute_batch(
            "PRAGMA foreign_keys=ON; PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL; PRAGMA busy_timeout=10000;",
        )?;
        let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute_batch(SCHEMA)?;
        tx.execute(
            "INSERT OR IGNORE INTO wallet_epoch(singleton,epoch) VALUES(1,?)",
            [expected_epoch.to_string()],
        )?;
        let epoch: String = tx.query_row(
            "SELECT epoch FROM wallet_epoch WHERE singleton=1",
            [],
            |r| r.get(0),
        )?;
        ensure!(
            epoch == expected_epoch.to_string(),
            "wallet epoch mismatch; reconcile before startup"
        );
        // Older reports have no wallet completion timestamp. Charge their
        // frozen maximum for one window from the first writer upgrade.
        tx.execute(
            "INSERT INTO treasury_fee_completions(intent_id,completed_at,fee_sats)
             SELECT r.intent_id,
                    MAX(unixepoch(),COALESCE((SELECT MAX(admitted_at) FROM treasury_execution_admissions),0),
                        COALESCE((SELECT MAX(completed_at) FROM treasury_fee_completions),0)),
                    json_extract(i.issued_json,'$.intent.maximum_fee_sats')
             FROM treasury_results r JOIN treasury_instructions i ON i.id=r.intent_id
             WHERE NOT EXISTS(SELECT 1 FROM treasury_fee_completions c WHERE c.intent_id=r.intent_id)",
            [],
        )?;
        tx.commit()?;
        Ok(Self {
            connection,
            max_proofs,
            custody_limits: None,
        })
    }

    pub(crate) fn set_custody_limits(&mut self, limits: custody::CustodyLimits) {
        self.custody_limits = Some(limits);
    }

    pub(crate) fn require_epoch(&self, expected: Uuid) -> Result<()> {
        let epoch: String = self.connection.query_row(
            "SELECT epoch FROM wallet_epoch WHERE singleton=1",
            [],
            |r| r.get(0),
        )?;
        ensure!(
            !expected.is_nil() && epoch == expected.to_string(),
            "wallet epoch mismatch"
        );
        Ok(())
    }

    /// All-or-nothing admission: overlapping old/new sets are rejected before
    /// any fresh proof is claimed. Proof identity excludes memo/witness/transport.
    pub fn admit(&mut self, intake: &ValidatedIntake<'_>) -> Result<Admission> {
        let now = i64::try_from(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_secs(),
        )?;
        self.admit_at(intake, now)
    }

    pub(crate) fn admit_at(&mut self, intake: &ValidatedIntake<'_>, now: i64) -> Result<Admission> {
        ensure!(now >= 0, "invalid intake observation time");
        ensure!(
            !intake.proofs.is_empty() && intake.proofs.len() <= self.max_proofs,
            "invalid proof count"
        );
        for field in [intake.mint_id, intake.policy_version, intake.source_ref] {
            ensure!(
                !field.is_empty() && field.len() <= 256,
                "invalid intake binding"
            );
        }
        match intake.transport {
            Transport::Cashu => ensure!(
                intake.source_author.is_none(),
                "ordinary Cashu must be unattributed"
            ),
            Transport::Nutzap => {
                let author = intake
                    .source_author
                    .ok_or_else(|| anyhow::anyhow!("nutzap author missing"))?;
                lightning_goats_nostr_event::validate_public_key(author)?;
            }
        }
        let admitted_proofs = crate::transport::serialize_body(&intake.proofs, 1_048_576)
            .map_err(|_| anyhow::anyhow!("admitted proof material exceeds limit"))?;
        let mut proofs = BTreeMap::new();
        let mut gross = 0_i64;
        for proof in intake.proofs {
            let amount = i64::try_from(u64::from(proof.amount))?;
            ensure!(amount > 0, "invalid proof amount");
            gross = gross
                .checked_add(amount)
                .ok_or_else(|| anyhow::anyhow!("amount overflow"))?;
            let mut evidence = Sha256::new();
            evidence.update(proof.keyset_id.to_string().as_bytes());
            evidence.update(amount.to_be_bytes());
            evidence.update(proof.c.to_bytes());
            let y = proof.y()?.to_bytes().to_vec();
            ensure!(
                proofs.insert(y, evidence.finalize().to_vec()).is_none(),
                "duplicate proof in payload"
            );
        }
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let mut existing = None;
        let mut old_count = 0;
        for (y, evidence) in &proofs {
            let row: Option<(String, Vec<u8>)> = tx.query_row(
                "SELECT acquisition_id,evidence FROM proof_index WHERE mint_id=? AND unit='sat' AND y=?",
                params![intake.mint_id, y], |r| Ok((r.get(0)?, r.get(1)?)),
            ).optional()?;
            if let Some((id, previous)) = row {
                ensure!(&previous == evidence, AdmissionConflict);
                if let Some(ref previous) = existing {
                    ensure!(previous == &id, AdmissionConflict);
                }
                existing = Some(id);
                old_count += 1;
            }
        }
        let is_duplicate = existing.is_some();
        let previous: Option<String> = tx
            .query_row(
                "SELECT acquisition_id FROM intake_sources WHERE transport=? AND source_ref=?",
                params![intake.transport.as_str(), intake.source_ref],
                |r| r.get(0),
            )
            .optional()?;
        ensure!(
            previous
                .as_ref()
                .is_none_or(|p| existing.as_ref() == Some(p)),
            AdmissionConflict
        );
        let id = if let Some(id) = existing {
            let count: i64 = tx.query_row(
                "SELECT proof_count FROM acquisitions WHERE id=?",
                [&id],
                |r| r.get(0),
            )?;
            ensure!(
                old_count == proofs.len() && count == i64::try_from(proofs.len())?,
                AdmissionConflict
            );
            Uuid::parse_str(&id)?
        } else {
            if let Some(limits) = &self.custody_limits {
                limits.reserve(&tx, intake.mint_id, u64::try_from(gross)?)?;
            }
            let (pending, proof_bytes): (u64, u64) = tx.query_row(
                "SELECT COUNT(*),COALESCE(SUM(length(c.proofs)),0) FROM acquisitions a JOIN acquisition_context c ON c.acquisition_id=a.id WHERE a.state!='credited'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?;
            ensure!(
                pending < MAX_PENDING_ACQUISITIONS
                    && proof_bytes
                        .checked_add(u64::try_from(admitted_proofs.len())?)
                        .is_some_and(|bytes| bytes <= MAX_PENDING_PROOF_BYTES),
                custody::AdmissionCapacity::Full
            );
            let id = Uuid::new_v4();
            tx.execute("INSERT INTO acquisitions(id,mint_id,policy,transport,source_ref,proof_count,gross) VALUES(?,?,?,?,?,?,?)",
                params![id.to_string(), intake.mint_id, intake.policy_version, intake.transport.as_str(), intake.source_ref, i64::try_from(proofs.len())?, gross])?;
            tx.execute("INSERT INTO acquisition_context(acquisition_id,proofs,source_author) VALUES(?,?,?)",
                params![id.to_string(),admitted_proofs,intake.source_author])?;
            tx.execute(
                "INSERT INTO acquisition_admission_times(acquisition_id,admitted_at) VALUES(?,?)",
                params![id.to_string(), now],
            )?;
            for (y, evidence) in proofs {
                tx.execute("INSERT INTO proof_index(mint_id,unit,y,evidence,acquisition_id) VALUES(?,'sat',?,?,?)",
                    params![intake.mint_id, y, evidence, id.to_string()])?;
            }
            id
        };
        if previous.is_none() {
            let sources: u64 = tx.query_row(
                "SELECT COUNT(*) FROM intake_sources WHERE acquisition_id=?",
                [id.to_string()],
                |r| r.get(0),
            )?;
            ensure!(
                sources < MAX_INTAKE_SOURCES_PER_ACQUISITION,
                AdmissionConflict
            );
        }
        tx.execute("INSERT OR IGNORE INTO intake_sources(transport,source_ref,acquisition_id) VALUES(?,?,?)",
            params![intake.transport.as_str(), intake.source_ref, id.to_string()])?;
        tx.commit()?;
        Ok(if is_duplicate {
            Admission::Duplicate(id)
        } else {
            Admission::New(id)
        })
    }

    /// Private recovery inputs, stored atomically with their deduplication claims.
    /// Never return these through public status or operator diagnostics.
    pub fn admitted_proofs(&self, id: Uuid) -> Result<Vec<Proof>> {
        let material: Vec<u8> = self.connection.query_row(
            "SELECT proofs FROM acquisition_context WHERE acquisition_id=?",
            [id.to_string()],
            |row| row.get(0),
        )?;
        let proofs: Vec<Proof> = serde_json::from_slice(&material)
            .map_err(|_| anyhow::anyhow!("invalid retained proof material"))?;
        ensure!(
            !proofs.is_empty() && proofs.len() <= self.max_proofs,
            "invalid retained proof count"
        );
        Ok(proofs)
    }

    pub(crate) fn require_queued_receivers(&self, keys: &crate::service_keys::Keys) -> Result<()> {
        Self::verify_queued_receivers(&self.connection, keys)
    }

    pub(crate) fn verify_queued_receivers(
        connection: &Connection,
        keys: &crate::service_keys::Keys,
    ) -> Result<()> {
        let mut rows = connection
            .prepare("SELECT c.proofs FROM acquisitions a LEFT JOIN acquisition_context c ON c.acquisition_id=a.id WHERE a.state='validated'")?;
        let pending = rows.query_map([], |row| row.get::<_, Vec<u8>>(0))?;
        for material in pending {
            let proofs: Vec<Proof> = serde_json::from_slice(&material?)
                .map_err(|_| anyhow::anyhow!("invalid retained proof material"))?;
            ensure!(
                (1..=1024).contains(&proofs.len()),
                "invalid retained proof count"
            );
            for proof in proofs {
                if let Some(lock) = crate::intake::receiver_lock(&proof)? {
                    keys.signing_key(lock)?;
                }
            }
        }
        Ok(())
    }

    /// Persist exact swap material before any state-changing mint request.
    pub fn prepare(&mut self, id: Uuid, request: &[u8]) -> Result<()> {
        ensure!(
            !request.is_empty() && request.len() <= 1_048_576,
            "invalid operation material"
        );
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let (state, previous): (String, Option<Vec<u8>>) = tx.query_row(
            "SELECT state,request FROM acquisitions WHERE id=?",
            [id.to_string()],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        if let Some(previous) = previous {
            ensure!(previous == request, "operation material conflict");
        } else {
            ensure!(state == "validated", "operation cannot be prepared");
            if let Some(limits) = &self.custody_limits {
                let (mint, gross): (String, u64) = tx.query_row(
                    "SELECT mint_id,gross FROM acquisitions WHERE id=?",
                    [id.to_string()],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )?;
                limits.check_fresh_swap(&tx, &mint, gross)?;
            }
            tx.execute(
                "UPDATE acquisitions SET request=?,state='acquisition_prepared' WHERE id=?",
                params![request, id.to_string()],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn start_acquisition(&mut self, id: Uuid) -> Result<()> {
        let changed = self.connection.execute(
            "UPDATE acquisitions SET state='acquiring' WHERE id=? AND state='acquisition_prepared'",
            [id.to_string()],
        )?;
        if changed != 1 {
            bail!("operation not prepared; reconcile original request");
        }
        Ok(())
    }

    /// Exact persisted material for recovery of the original operation only.
    /// Contains secrets; callers must never emit this value into diagnostics.
    pub fn prepared_request(&self, id: Uuid) -> Result<Option<Vec<u8>>> {
        Ok(self.connection.query_row(
            "SELECT request FROM acquisitions WHERE id=?",
            [id.to_string()],
            |r| r.get(0),
        )?)
    }

    /// Bounded private recovery queue; an acquiring state never authorizes resubmission.
    pub fn pending_operations(&self, limit: usize) -> Result<Vec<(Uuid, String)>> {
        ensure!(limit > 0 && limit <= 1024, "invalid recovery page limit");
        let mut statement = self.connection.prepare(
            "SELECT id,state FROM acquisitions WHERE state!='credited' ORDER BY rowid LIMIT ?",
        )?;
        let rows = statement.query_map([i64::try_from(limit)?], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })?;
        rows.map(|row| {
            let (id, state) = row?;
            Ok((Uuid::parse_str(&id)?, state))
        })
        .collect()
    }

    /// Bind CDK's actual swap inputs to the admitted proof index, allowing only
    /// witness/DLEQ transport changes. Inputs cannot be borrowed from another receipt.
    pub fn verify_swap_inputs(&self, id: Uuid, swap: &cdk::nuts::SwapRequest) -> Result<u64> {
        let count: i64 = self.connection.query_row(
            "SELECT proof_count FROM acquisitions WHERE id=?",
            [id.to_string()],
            |row| row.get(0),
        )?;
        ensure!(
            i64::try_from(swap.inputs().len())? == count,
            "swap input set changed"
        );
        let mut seen = std::collections::BTreeSet::new();
        for proof in swap.inputs() {
            let y = proof.y()?.to_bytes().to_vec();
            ensure!(seen.insert(y.clone()), "duplicate swap input");
            let evidence: Vec<u8> = self.connection.query_row(
                "SELECT p.evidence FROM proof_index p JOIN acquisitions a ON a.id=p.acquisition_id AND a.mint_id=p.mint_id WHERE a.id=? AND p.unit='sat' AND p.y=?",
                params![id.to_string(),y], |row|row.get(0),
            )?;
            let amount = i64::try_from(u64::from(proof.amount))?;
            let mut expected = Sha256::new();
            expected.update(proof.keyset_id.to_string().as_bytes());
            expected.update(amount.to_be_bytes());
            expected.update(proof.c.to_bytes());
            ensure!(
                evidence == expected.finalize().to_vec(),
                "swap input evidence changed"
            );
        }
        let input = swap.input_amount()?;
        let output = swap.output_amount()?;
        ensure!(
            !swap.outputs().is_empty()
                && output > cdk::Amount::ZERO
                && output <= input
                && swap
                    .outputs()
                    .iter()
                    .all(|item| item.amount > cdk::Amount::ZERO),
            "invalid swap output value"
        );
        Ok(u64::from(input) - u64::from(output))
    }

    pub fn mint_id(&self, id: Uuid) -> Result<String> {
        Ok(self.connection.query_row(
            "SELECT mint_id FROM acquisitions WHERE id=?",
            [id.to_string()],
            |row| row.get(0),
        )?)
    }

    pub(crate) fn record_derivation(
        &mut self,
        id: Uuid,
        derivation: &crate::ownership::SwapDerivation,
    ) -> Result<()> {
        for previous in self.unresolved_derivations_except(id)? {
            ensure!(
                previous.mint_url != derivation.mint_url
                    || previous.keyset_id != derivation.keyset_id
                    || derivation.counter_end <= previous.counter_start
                    || derivation.counter_start >= previous.counter_end,
                "new output range overlaps retained recovery material"
            );
        }
        let bytes = crate::transport::serialize_body(derivation, 1_048_576)
            .map_err(|_| anyhow::anyhow!("derivation material exceeds limit"))?;
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let state: String = tx.query_row(
            "SELECT state FROM acquisitions WHERE id=?",
            [id.to_string()],
            |row| row.get(0),
        )?;
        ensure!(state == "acquiring", "operation not acquiring");
        let previous: Option<Vec<u8>> = tx
            .query_row(
                "SELECT material FROM swap_derivations WHERE acquisition_id=?",
                [id.to_string()],
                |row| row.get(0),
            )
            .optional()?;
        if let Some(previous) = previous {
            ensure!(previous == bytes, "derivation binding conflict");
        } else {
            tx.execute(
                "INSERT INTO swap_derivations(acquisition_id,material) VALUES(?,?)",
                params![id.to_string(), bytes],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn derivation(&self, id: Uuid) -> Result<crate::ownership::SwapDerivation> {
        let bytes: Vec<u8> = self.connection.query_row(
            "SELECT material FROM swap_derivations WHERE acquisition_id=?",
            [id.to_string()],
            |row| row.get(0),
        )?;
        serde_json::from_slice(&bytes).map_err(|_| anyhow::anyhow!("invalid retained derivation"))
    }

    /// Persist a successful mint response before allowing CDK to process it.
    /// This is private evidence, not an acquisition or credit attestation.
    pub fn record_swap_reply(&mut self, id: Uuid, reply: &[u8]) -> Result<()> {
        ensure!(
            !reply.is_empty() && reply.len() <= 1_048_576,
            "invalid operation reply"
        );
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let state: String = tx.query_row(
            "SELECT state FROM acquisitions WHERE id=?",
            [id.to_string()],
            |r| r.get(0),
        )?;
        ensure!(state == "acquiring", "operation not acquiring");
        let existing: Option<Vec<u8>> = tx
            .query_row(
                "SELECT reply FROM operation_replies WHERE acquisition_id=?",
                [id.to_string()],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(previous) = existing {
            ensure!(previous == reply, "conflicting operation reply");
        } else {
            tx.execute(
                "INSERT INTO operation_replies(acquisition_id,reply) VALUES(?,?)",
                params![id.to_string(), reply],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn swap_reply(&self, id: Uuid) -> Result<Option<Vec<u8>>> {
        Ok(self
            .connection
            .query_row(
                "SELECT reply FROM operation_replies WHERE acquisition_id=?",
                [id.to_string()],
                |r| r.get(0),
            )
            .optional()?)
    }

    pub fn mint_has_other_unresolved_operation(&self, id: Uuid) -> Result<bool> {
        Ok(self.connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM acquisitions a JOIN acquisitions current ON a.mint_id=current.mint_id WHERE current.id=? AND a.id!=current.id AND a.state IN ('acquisition_prepared','acquiring','held_for_reconciliation'))",
            [id.to_string()], |row| row.get(0),
        )?)
    }

    pub(crate) fn unresolved_derivations_except(
        &self,
        id: Uuid,
    ) -> Result<Vec<crate::ownership::SwapDerivation>> {
        let mut statement = self.connection.prepare(
            "SELECT a.id FROM acquisitions a JOIN acquisitions current ON a.mint_id=current.mint_id WHERE current.id=? AND a.id!=current.id AND a.state IN ('acquisition_prepared','acquiring','held_for_reconciliation')",
        )?;
        let ids = statement.query_map([id.to_string()], |row| row.get::<_, String>(0))?;
        ids.map(|other| self.derivation(Uuid::parse_str(&other?)?))
            .collect()
    }

    /// Preserve the original operation and proof claims. A hold never frees funds
    /// or rewinds an exported/credited receipt whose core outcome may be unknown.
    pub fn hold(&mut self, id: Uuid, reason: HoldReason) -> Result<()> {
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let state: String = tx.query_row(
            "SELECT state FROM acquisitions WHERE id=?",
            [id.to_string()],
            |row| row.get(0),
        )?;
        if state == "held_for_reconciliation" {
            tx.commit()?;
            return Ok(());
        }
        ensure!(
            matches!(
                state.as_str(),
                "validated" | "acquisition_prepared" | "acquiring" | "acquired"
            ),
            "cannot hold an exported receipt without core reconciliation"
        );
        tx.execute(
            "INSERT INTO operation_holds(acquisition_id,previous_state,reason) VALUES(?,?,?)",
            params![id.to_string(), state, reason.as_str()],
        )?;
        tx.execute(
            "UPDATE acquisitions SET state='held_for_reconciliation' WHERE id=?",
            [id.to_string()],
        )?;
        tx.commit()?;
        Ok(())
    }

    pub fn hold_reason(&self, id: Uuid) -> Result<Option<String>> {
        Ok(self
            .connection
            .query_row(
                "SELECT reason FROM operation_holds WHERE acquisition_id=?",
                [id.to_string()],
                |row| row.get(0),
            )
            .optional()?)
    }

    pub(crate) fn has_recheckable_ownership_hold(&self, id: Uuid) -> Result<bool> {
        recheckable_ownership_hold(&self.connection, id)
    }

    pub fn state(&self, id: Uuid) -> Result<String> {
        Ok(self.connection.query_row(
            "SELECT state FROM acquisitions WHERE id=?",
            [id.to_string()],
            |r| r.get(0),
        )?)
    }
}

fn recheckable_ownership_hold(connection: &Connection, id: Uuid) -> Result<bool> {
    Ok(connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM operation_holds WHERE acquisition_id=? AND previous_state='acquiring' AND reason=?)",
        params![id.to_string(), HoldReason::MissingOwnedOutputs.as_str()],
        |row| row.get(0),
    )?)
}

/// Private wallet-to-core projection. Contains no proofs or spendable operation material.
#[derive(Clone, PartialEq, Eq)]
pub struct Receipt {
    pub schema_version: u32,
    pub wallet_epoch: Uuid,
    pub acquisition_id: Uuid,
    pub mint_id: String,
    pub policy_version: String,
    pub transport: String,
    pub source_ref: String,
    pub source_author: Option<String>,
    pub gross_sats: u64,
    pub receive_fee_sats: u64,
    pub net_sats: u64,
    pub evidence_digest: Vec<u8>,
    pub credit_pool: &'static str,
}

impl Journal {
    /// The engine calls this only after CDK verifies and durably owns its new outputs.
    /// A SPENT input-state response is insufficient evidence for this transition.
    #[cfg(test)]
    pub(crate) fn record_acquired(
        &mut self,
        id: Uuid,
        net: u64,
        fee: u64,
        evidence: &[u8],
    ) -> Result<()> {
        self.record_acquired_outputs(id, net, fee, evidence, &[], false)
    }

    pub(crate) fn record_acquired_outputs(
        &mut self,
        id: Uuid,
        net: u64,
        fee: u64,
        evidence: &[u8],
        outputs: &[(String, String, u64)],
        recheck_hold: bool,
    ) -> Result<()> {
        #[cfg(not(test))]
        ensure!(!outputs.is_empty(), "owned output attribution missing");
        ensure!(outputs.len() <= 1024, "owned output count exceeds bound");
        if !outputs.is_empty() {
            let mut total = 0u64;
            let mut seen = std::collections::BTreeSet::new();
            for (y, c, amount) in outputs {
                cdk::nuts::PublicKey::from_hex(y)?;
                cdk::nuts::PublicKey::from_hex(c)?;
                ensure!(
                    *amount > 0 && seen.insert(y),
                    "invalid owned output identity"
                );
                total = total
                    .checked_add(*amount)
                    .ok_or_else(|| anyhow::anyhow!("owned output sum overflow"))?;
            }
            ensure!(
                total == net,
                "owned output attribution does not conserve net value"
            );
        }
        ensure!(
            !evidence.is_empty() && evidence.len() <= 1_048_576,
            "invalid acquisition evidence"
        );
        let net = i64::try_from(net)?;
        let fee = i64::try_from(fee)?;
        ensure!(net > 0, "empty acquisition");
        let digest = Sha256::digest(evidence).to_vec();
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let (state, gross, previous_net, previous_fee, previous_evidence): (
            String,
            i64,
            Option<i64>,
            Option<i64>,
            Option<Vec<u8>>,
        ) = tx.query_row(
            "SELECT state,gross,net,receive_fee,acquired_evidence FROM acquisitions WHERE id=?",
            [id.to_string()],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
        )?;
        ensure!(
            net.checked_add(fee) == Some(gross),
            "acquisition conservation failure"
        );
        if recheck_hold {
            ensure!(
                !outputs.is_empty()
                    && matches!(state.as_str(), "held_for_reconciliation" | "acquired")
                    && recheckable_ownership_hold(&tx, id)?,
                "acquisition has no eligible ownership hold"
            );
        }
        if let Some(previous) = previous_evidence {
            ensure!(
                previous == digest && previous_net == Some(net) && previous_fee == Some(fee),
                "acquisition evidence conflict"
            );
        } else {
            ensure!(
                state
                    == if recheck_hold {
                        "held_for_reconciliation"
                    } else {
                        "acquiring"
                    },
                "acquisition has no open ownership transition"
            );
            tx.execute("UPDATE acquisitions SET state='acquired',net=?,receive_fee=?,acquired_evidence=? WHERE id=?",
                params![net,fee,digest,id.to_string()])?;
        }
        let mint: String = tx.query_row(
            "SELECT mint_id FROM acquisitions WHERE id=?",
            [id.to_string()],
            |r| r.get(0),
        )?;
        for (y, c, amount) in outputs {
            let treasury_owned: bool = tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM treasury_change_outputs WHERE mint_id=? AND y=?)",
                params![mint, y],
                |r| r.get(0),
            )?;
            ensure!(!treasury_owned, "owned output is already treasury change");
            let prior: Option<(String, String, u64)> = tx.query_row(
                "SELECT acquisition_id,c,amount FROM owned_output_sources WHERE mint_id=? AND y=?",
                params![mint,y], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional()?;
            if let Some((owner, old_c, old_amount)) = prior {
                ensure!(
                    owner == id.to_string() && old_c == *c && old_amount == *amount,
                    "owned output already attributed to another acquisition"
                );
            } else {
                tx.execute("INSERT INTO owned_output_sources(mint_id,y,c,amount,acquisition_id) VALUES(?,?,?,?,?)",
                    params![mint,y,c,amount,id.to_string()])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// A durable outbox entry is the same immutable acquired record on every retry.
    pub fn export_receipt(&mut self, id: Uuid) -> Result<Receipt> {
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute(
            "UPDATE acquisitions SET state='credit_export_pending' WHERE id=? AND state='acquired'",
            [id.to_string()],
        )?;
        let receipt = tx.query_row(
            "SELECT a.mint_id,a.policy,a.transport,a.source_ref,a.gross,a.receive_fee,a.net,a.acquired_evidence,e.epoch,c.source_author FROM acquisitions a JOIN acquisition_context c ON c.acquisition_id=a.id CROSS JOIN wallet_epoch e WHERE a.id=? AND a.state IN ('credit_export_pending','credited')",
            [id.to_string()], |r| Ok(Receipt {
                schema_version: 1,
                wallet_epoch: Uuid::nil(),
                acquisition_id: id,
                mint_id: r.get(0)?, policy_version: r.get(1)?, transport: r.get(2)?, source_ref: r.get(3)?, source_author: r.get(9)?,
                gross_sats: r.get(4)?, receive_fee_sats: r.get(5)?, net_sats: r.get(6)?, evidence_digest: r.get(7)?,
                credit_pool: "herd",
            }).map(|receipt| (receipt, r.get::<_, String>(8))))?;
        let (mut receipt, epoch) = receipt;
        receipt.wallet_epoch = Uuid::parse_str(&epoch?)?;
        tx.commit()?;
        Ok(receipt)
    }

    /// An acknowledgment lost after the core consumed the credit must still be safe.
    pub(crate) fn acknowledge_credit(&mut self, id: Uuid, acknowledgment: &[u8]) -> Result<()> {
        ensure!(
            !acknowledgment.is_empty() && acknowledgment.len() <= 4096,
            "invalid credit acknowledgment"
        );
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let (state, previous): (String, Option<Vec<u8>>) = tx.query_row(
            "SELECT state,core_ack FROM acquisitions WHERE id=?",
            [id.to_string()],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        ensure!(
            state == "credit_export_pending" || state == "credited",
            "receipt not exported"
        );
        if let Some(previous) = previous {
            ensure!(previous == acknowledgment, "credit acknowledgment conflict");
        } else {
            tx.execute(
                "UPDATE acquisitions SET state='credited',core_ack=? WHERE id=?",
                params![acknowledgment, id.to_string()],
            )?;
        }
        tx.commit()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cdk::{
        Amount,
        nuts::{Id, PublicKey},
        secret::Secret,
    };
    use std::str::FromStr;

    fn proof(secret: &str) -> Proof {
        Proof::new(
            Amount::from(8),
            Id::from_str("00deadbeef123456").unwrap(),
            Secret::new(secret),
            PublicKey::from_hex(
                "0279be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798",
            )
            .unwrap(),
        )
    }
    fn intake<'a>(
        proofs: &'a [Proof],
        source: &'a str,
        transport: Transport,
    ) -> ValidatedIntake<'a> {
        ValidatedIntake {
            mint_id: "fixture-mint",
            policy_version: "fixture-v1",
            transport,
            source_ref: source,
            source_author: match transport {
                Transport::Cashu => None,
                Transport::Nutzap => {
                    Some("79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798")
                }
            },
            proofs,
        }
    }
    fn new_id(admission: Admission) -> Uuid {
        match admission {
            Admission::New(id) => id,
            Admission::Duplicate(_) => panic!("expected new acquisition"),
        }
    }

    #[test]
    fn custody_is_reserved_atomically_across_mints_rails_and_restart() -> Result<()> {
        use custody::AdmissionCapacity;
        let directory = crate::test_directory()?;
        let path = directory.path().join("custody.sqlite");
        let epoch = Uuid::new_v4();
        let limits: custody::CustodyLimits = serde_json::from_value(serde_json::json!({
            "maximum_total_sats":20,"maximum_per_mint_sats":{"fixture-mint":12,"second":16}
        }))?;
        let mut journal = Journal::open(&path, epoch, 8)?;
        journal.set_custody_limits(limits.clone());
        let first_proofs = [proof("first")];
        let first = new_id(journal.admit(&intake(&first_proofs, "first", Transport::Cashu))?);
        let second_proofs = [proof("second")];
        let mut second = intake(&second_proofs, "second", Transport::Nutzap);
        assert_eq!(
            journal.admit(&second).unwrap_err().downcast_ref(),
            Some(&AdmissionCapacity::Full)
        );
        second.mint_id = "second";
        let second = new_id(journal.admit(&second)?);
        let mut small_proof = proof("third");
        small_proof.amount = 5.into();
        let third_proofs = [small_proof];
        let mut third = intake(&third_proofs, "third", Transport::Cashu);
        third.mint_id = "second";
        assert_eq!(
            journal.admit(&third).unwrap_err().downcast_ref(),
            Some(&AdmissionCapacity::Full)
        );
        let overlarge = [proof("over-one"), proof("over-two")];
        assert_eq!(
            journal
                .admit(&intake(&overlarge, "oversized", Transport::Cashu))
                .unwrap_err()
                .downcast_ref(),
            Some(&AdmissionCapacity::TooLarge)
        );
        journal.prepare(first, b"first swap")?;
        journal.start_acquisition(first)?;
        journal.record_acquired(first, 7, 1, b"first owned evidence")?;
        journal.hold(second, HoldReason::AmbiguousAcquisition)?;
        drop(journal);

        let mut journal = Journal::open(&path, epoch, 8)?;
        journal.set_custody_limits(limits);
        let third = new_id(journal.admit(&intake(&third_proofs, "third", Transport::Cashu))?);
        assert_eq!(
            journal.admit(&intake(&first_proofs, "original replay", Transport::Nutzap))?,
            Admission::Duplicate(first)
        );
        let fourth = [proof("fourth")];
        let mut fourth = intake(&fourth, "fourth", Transport::Cashu);
        fourth.mint_id = "second";
        assert_eq!(
            journal.admit(&fourth).unwrap_err().downcast_ref(),
            Some(&AdmissionCapacity::Full)
        );
        assert_eq!(journal.state(third)?, "validated");
        for table in ["acquisitions", "proof_index", "acquisition_admission_times"] {
            assert_eq!(
                journal.connection.query_row(
                    &format!("SELECT COUNT(*) FROM {table}"),
                    [],
                    |r| r.get::<_, u64>(0)
                )?,
                3
            );
        }
        Ok(())
    }

    #[test]
    fn legacy_overflow_waits_without_charging_swap_attempts_or_losing_originals() -> Result<()> {
        use custody::AdmissionCapacity;
        let directory = crate::test_directory()?;
        let path = directory.path().join("legacy.sqlite");
        let epoch = Uuid::new_v4();
        let mut journal = Journal::open(&path, epoch, 8)?;
        let large = [proof("large-1"), proof("large-2")];
        let large = new_id(journal.admit(&intake(&large, "large", Transport::Cashu))?);
        let pending = [proof("small")];
        let small = new_id(journal.admit(&intake(&pending, "small", Transport::Cashu))?);
        let underway = [proof("original")];
        let original = new_id(journal.admit(&intake(&underway, "original", Transport::Nutzap))?);
        journal.prepare(original, b"original exact request")?;
        journal.start_acquisition(original)?;
        drop(journal);

        let mut journal = Journal::open(&path, epoch, 8)?;
        journal.set_custody_limits(custody::CustodyLimits::default());
        assert_eq!(
            journal.admit(&intake(&pending, "small", Transport::Cashu))?,
            Admission::Duplicate(small)
        );
        assert!(
            journal
                .claim_acquisition_work(100, true, false, 1, 5)?
                .is_none()
        );
        let work = journal
            .claim_acquisition_work(100, true, true, 1, 5)?
            .unwrap();
        assert_eq!(work.id, original);
        journal.prepare(original, b"original exact request")?;

        journal.set_custody_limits(serde_json::from_value(serde_json::json!({
            "maximum_total_sats":15,"maximum_per_mint_sats":{"fixture-mint":15}
        }))?);
        assert!(
            journal
                .claim_acquisition_work(200, true, false, 1, 5)?
                .is_none()
        );
        assert_eq!(
            journal
                .prepare(large, b"unsubmitted")
                .unwrap_err()
                .downcast_ref(),
            Some(&AdmissionCapacity::Full)
        );
        journal.record_acquired(original, 7, 1, b"original owned outputs")?;
        let work = journal
            .claim_acquisition_work(201, true, false, 1, 5)?
            .unwrap();
        assert_eq!(work.id, small);
        journal.prepare(small, b"original small swap")?;
        assert!(journal.prepared_request(large)?.is_none());
        assert_eq!(
            journal
                .connection
                .query_row("SELECT COUNT(*) FROM acquisition_schedule", [], |r| r
                    .get::<_, u64>(0))?,
            2
        );
        Ok(())
    }

    #[test]
    fn admission_time_is_atomic_and_retries_never_rewrite_it() -> Result<()> {
        let directory = crate::test_directory()?;
        let path = directory.path().join("observed.sqlite");
        let epoch = Uuid::new_v4();
        let proofs = [proof("timed-proof")];
        let mut journal = Journal::open(&path, epoch, 8)?;
        assert!(
            journal
                .admit_at(&intake(&proofs, "timed", Transport::Cashu), -1)
                .is_err()
        );
        let first = new_id(journal.admit_at(&intake(&proofs, "timed", Transport::Cashu), 100)?);
        drop(journal);
        let mut journal = Journal::open(&path, epoch, 8)?;
        assert_eq!(
            journal.admit_at(&intake(&proofs, "later-alias", Transport::Cashu), 999)?,
            Admission::Duplicate(first)
        );
        let (count, time): (u64, i64) = journal.connection.query_row(
            "SELECT COUNT(*),MIN(admitted_at) FROM acquisition_admission_times",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        assert_eq!((count, time), (1, 100));
        Ok(())
    }

    #[test]
    fn intake_backpressure_survives_restart_and_preserves_original_retries() -> Result<()> {
        let dir = crate::test_directory()?;
        let path = dir.path().join("bounded.sqlite");
        let epoch = Uuid::new_v4();
        let mut journal = Journal::open(&path, epoch, 8)?;
        let first_proofs = [proof("bounded-0")];
        let first = new_id(journal.admit(&intake(&first_proofs, "source-0", Transport::Cashu))?);
        for n in 1..MAX_PENDING_ACQUISITIONS {
            journal.admit(&intake(
                &[proof(&format!("bounded-{n}"))],
                &format!("source-{n}"),
                Transport::Cashu,
            ))?;
        }
        // Held and exhausted work still consumes space and must count.
        journal.hold(first, HoldReason::AmbiguousAcquisition)?;
        drop(journal);
        let mut journal = Journal::open(&path, epoch, 8)?;
        let refused = [proof("bounded-overflow")];
        assert!(
            journal
                .admit(&intake(&refused, "overflow", Transport::Nutzap))
                .is_err()
        );
        assert_eq!(
            journal.admit(&intake(&first_proofs, "source-0", Transport::Cashu))?,
            Admission::Duplicate(first)
        );
        // Cross-transport replay remains the same acquisition at capacity.
        assert_eq!(
            journal.admit(&intake(&first_proofs, "relay-replay", Transport::Nutzap))?,
            Admission::Duplicate(first)
        );
        for table in ["acquisitions", "acquisition_context", "proof_index"] {
            let count: u64 = journal.connection.query_row(
                &format!("SELECT COUNT(*) FROM {table}"),
                [],
                |r| r.get(0),
            )?;
            assert_eq!(count, MAX_PENDING_ACQUISITIONS);
        }
        let overflow_sources: u64 = journal.connection.query_row(
            "SELECT COUNT(*) FROM intake_sources WHERE source_ref='overflow'",
            [],
            |r| r.get(0),
        )?;
        assert_eq!(overflow_sources, 0);
        assert!(
            journal.connection.query_row(
                "SELECT COUNT(*) FROM acquisitions WHERE state='held_for_reconciliation'",
                [],
                |r| r.get::<_, u64>(0)
            )? > 0
        );
        // Completing a different original frees one slot without removing its
        // financial history or deduplication claim.
        let completed: String = journal.connection.query_row(
            "SELECT acquisition_id FROM intake_sources WHERE source_ref='source-1'",
            [],
            |r| r.get(0),
        )?;
        let completed = Uuid::parse_str(&completed)?;
        journal.prepare(completed, b"fixture original swap")?;
        journal.start_acquisition(completed)?;
        journal.record_acquired(completed, 8, 0, b"fixture owned outputs")?;
        journal.export_receipt(completed)?;
        journal.acknowledge_credit(completed, b"fixture core acknowledgment")?;
        assert!(matches!(
            journal.admit(&intake(&refused, "overflow", Transport::Nutzap))?,
            Admission::New(_)
        ));
        assert_eq!(
            journal.admit(&intake(&[proof("bounded-1")], "source-1", Transport::Cashu))?,
            Admission::Duplicate(completed)
        );
        Ok(())
    }

    #[test]
    fn intake_proof_storage_and_replay_aliases_are_bounded() -> Result<()> {
        let dir = crate::test_directory()?;
        let mut journal = Journal::open(&dir.path().join("bytes.sqlite"), Uuid::new_v4(), 8)?;
        let large_secret = "x".repeat(900_000);
        let mut admitted = 0;
        for n in 0..32 {
            let proofs = [proof(&format!("{n}{large_secret}"))];
            if journal
                .admit(&intake(&proofs, &format!("large-{n}"), Transport::Cashu))
                .is_err()
            {
                break;
            }
            admitted += 1;
        }
        assert!(admitted > 0 && admitted < 32);
        let (count, bytes): (u64, u64) = journal.connection.query_row(
            "SELECT COUNT(*),SUM(length(proofs)) FROM acquisition_context",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        assert_eq!(count, admitted);
        assert!(bytes <= MAX_PENDING_PROOF_BYTES);

        let proofs = [proof("alias-fixture")];
        let id = new_id(journal.admit(&intake(&proofs, "alias-0", Transport::Cashu))?);
        for n in 1..MAX_INTAKE_SOURCES_PER_ACQUISITION {
            assert_eq!(
                journal.admit(&intake(&proofs, &format!("alias-{n}"), Transport::Cashu))?,
                Admission::Duplicate(id)
            );
        }
        assert!(
            journal
                .admit(&intake(&proofs, "alias-overflow", Transport::Nutzap))
                .is_err()
        );
        assert_eq!(
            journal.admit(&intake(&proofs, "alias-0", Transport::Cashu))?,
            Admission::Duplicate(id)
        );
        let count: u64 = journal.connection.query_row(
            "SELECT COUNT(*) FROM intake_sources WHERE acquisition_id=?",
            [id.to_string()],
            |r| r.get(0),
        )?;
        assert_eq!(count, MAX_INTAKE_SOURCES_PER_ACQUISITION);
        Ok(())
    }

    #[test]
    fn durable_scheduler_passes_unresolved_heads_and_never_resets_attempts_on_restart() -> Result<()>
    {
        let dir = crate::test_directory()?;
        let path = dir.path().join("schedule.sqlite");
        let epoch = Uuid::new_v4();
        let mut journal = Journal::open(&path, epoch, 8)?;
        let first = new_id(journal.admit(&intake(
            &[proof("scheduled-first")],
            "first",
            Transport::Cashu,
        ))?);
        let second = new_id(journal.admit(&intake(
            &[proof("scheduled-second")],
            "second",
            Transport::Cashu,
        ))?);
        assert!(
            journal
                .claim_acquisition_work(100, false, false, 2, 5)?
                .is_none()
        );
        assert_eq!(
            journal
                .claim_acquisition_work(100, true, false, 2, 5)?
                .unwrap()
                .id,
            first
        );
        assert_eq!(
            journal
                .claim_acquisition_work(100, true, false, 2, 5)?
                .unwrap()
                .id,
            second
        );
        assert!(
            journal
                .claim_acquisition_work(100, true, false, 2, 5)?
                .is_none()
        );
        drop(journal);
        let mut journal = Journal::open(&path, epoch, 8)?;
        assert!(
            journal
                .claim_acquisition_work(109, true, false, 2, 5)?
                .is_none()
        );
        assert_eq!(
            journal
                .claim_acquisition_work(110, true, false, 2, 5)?
                .unwrap()
                .id,
            first
        );
        assert_eq!(
            journal
                .claim_acquisition_work(110, true, false, 2, 5)?
                .unwrap()
                .id,
            second
        );
        assert!(
            journal
                .claim_acquisition_work(9999, true, true, 2, 5)?
                .is_none()
        );
        assert_eq!(journal.state(first)?, "validated");
        assert_eq!(journal.admitted_proofs(first)?.len(), 1);
        // Recovering work is gated separately and retains the original identity.
        journal.prepare(first, b"original-swap-fixture")?;
        journal.start_acquisition(first)?;
        assert!(
            journal
                .claim_acquisition_work(9999, true, false, 3, 5)?
                .is_some_and(|w| w.id == second)
        );
        let recovery = journal
            .claim_acquisition_work(9999, false, true, 3, 5)?
            .unwrap();
        assert_eq!(recovery.id, first);
        assert_eq!(recovery.state, "acquiring");
        assert!(
            journal
                .claim_acquisition_work(i64::MAX, false, true, 4, 60)
                .is_err()
        );
        assert_eq!(
            journal
                .claim_acquisition_work(20000, false, true, 4, 5)?
                .unwrap()
                .id,
            first
        );
        Ok(())
    }

    #[test]
    fn acquired_output_attribution_is_atomic_and_cannot_be_claimed_twice() -> Result<()> {
        let dir = crate::test_directory()?;
        let path = dir.path().join("owned.sqlite");
        let epoch = Uuid::new_v4();
        let mut journal = Journal::open(&path, epoch, 8)?;
        let mut ids = Vec::new();
        for name in ["first", "second"] {
            let proofs = vec![proof(name)];
            let id = new_id(journal.admit(&intake(&proofs, name, Transport::Cashu))?);
            journal.prepare(id, b"fixture prepared request")?;
            journal.start_acquisition(id)?;
            ids.push(id);
        }
        let point = "0279be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798";
        let outputs = vec![(point.to_owned(), point.to_owned(), 8)];
        journal.connection.execute_batch("CREATE TRIGGER fail_owned BEFORE INSERT ON owned_output_sources BEGIN SELECT RAISE(ABORT,'fixture'); END;")?;
        assert!(
            journal
                .record_acquired_outputs(ids[0], 8, 0, b"owned", &outputs, false)
                .is_err()
        );
        assert_eq!(journal.state(ids[0])?, "acquiring");
        journal
            .connection
            .execute_batch("DROP TRIGGER fail_owned;")?;
        journal.record_acquired_outputs(ids[0], 8, 0, b"owned", &outputs, false)?;
        drop(journal);
        let mut journal = Journal::open(&path, epoch, 8)?;
        journal.record_acquired_outputs(ids[0], 8, 0, b"owned", &outputs, false)?;
        assert!(
            journal
                .record_acquired_outputs(ids[1], 8, 0, b"other-owned", &outputs, false)
                .is_err()
        );
        assert_eq!(journal.state(ids[1])?, "acquiring");
        let count: i64 =
            journal
                .connection
                .query_row("SELECT COUNT(*) FROM owned_output_sources", [], |r| {
                    r.get(0)
                })?;
        assert_eq!(count, 1);
        Ok(())
    }

    #[test]
    fn receipt_outbox_retries_originals_fairly_and_retains_exhausted_exports() -> Result<()> {
        let dir = crate::test_directory()?;
        let path = dir.path().join("delivery.sqlite");
        let epoch = Uuid::new_v4();
        let mut journal = Journal::open(&path, epoch, 8)?;
        let mut ids = Vec::new();
        for index in 0..3 {
            let id = new_id(journal.admit(&intake(
                &[proof(&format!("delivery-{index}"))],
                &format!("http-{index}"),
                Transport::Cashu,
            ))?);
            if index != 1 {
                journal.prepare(id, b"exact-original-swap")?;
                journal.start_acquisition(id)?;
                journal.record_acquired(id, 7, 1, b"verified-owned-outputs")?;
            }
            ids.push(id);
        }
        assert!(journal.claim_receipt_delivery(100, 0, 5)?.is_none());
        assert_eq!(journal.claim_receipt_delivery(100, 2, 5)?, Some(ids[0]));
        let original = journal.export_receipt(ids[0])?;
        // A lost reply retains this export while later acquired work can run.
        assert_eq!(journal.claim_receipt_delivery(100, 2, 5)?, Some(ids[2]));
        assert!(journal.claim_receipt_delivery(100, 2, 5)?.is_none());
        drop(journal);
        let mut journal = Journal::open(&path, epoch, 8)?;
        assert!(journal.claim_receipt_delivery(109, 2, 5)?.is_none());
        assert_eq!(journal.claim_receipt_delivery(110, 2, 5)?, Some(ids[0]));
        assert!(journal.export_receipt(ids[0])? == original);
        assert_eq!(journal.claim_receipt_delivery(110, 2, 5)?, Some(ids[2]));
        assert!(journal.claim_receipt_delivery(10000, 2, 5)?.is_none());
        assert_eq!(journal.state(ids[0])?, "credit_export_pending");
        assert_eq!(journal.state(ids[1])?, "validated");
        assert_eq!(journal.state(ids[2])?, "acquired");
        assert!(journal.claim_receipt_delivery(i64::MAX, 3, 5).is_err());
        assert_eq!(journal.claim_receipt_delivery(10000, 3, 5)?, Some(ids[0]));
        Ok(())
    }

    #[test]
    fn cross_transport_replay_after_ack_and_reopen_preserves_one_receipt() -> Result<()> {
        let dir = crate::test_directory()?;
        let path = dir.path().join("intake.sqlite");
        let epoch = Uuid::new_v4();
        let proofs = vec![proof("a"), proof("b")];
        let (id, receipt) = {
            let mut journal = Journal::open(&path, epoch, 8)?;
            let id = new_id(journal.admit(&intake(&proofs, "http-request", Transport::Cashu))?);
            journal.prepare(id, b"exact prepared operation")?;
            journal.start_acquisition(id)?;
            assert!(journal.start_acquisition(id).is_err());
            assert!(journal.export_receipt(id).is_err());
            assert!(
                journal
                    .record_acquired(id, 15, 2, b"owned outputs")
                    .is_err()
            );
            journal.record_acquired(id, 15, 1, b"owned outputs")?;
            let receipt = journal.export_receipt(id)?;
            assert!(receipt == journal.export_receipt(id)?);
            journal.acknowledge_credit(id, b"original core grant")?;
            (id, receipt)
        };
        let mut journal = Journal::open(&path, epoch, 8)?;
        let reordered = vec![proofs[1].clone(), proofs[0].clone()];
        assert_eq!(
            journal.admit(&intake(&reordered, "nostr-event", Transport::Nutzap))?,
            Admission::Duplicate(id)
        );
        assert!(receipt == journal.export_receipt(id)?);
        assert_eq!(journal.state(id)?, "credited");
        journal.acknowledge_credit(id, b"original core grant")?;
        assert!(journal.acknowledge_credit(id, b"different grant").is_err());
        assert!(
            journal
                .record_acquired(id, 15, 1, b"conflicting outputs")
                .is_err()
        );
        assert!(receipt == journal.export_receipt(id)?);
        Ok(())
    }

    #[test]
    fn verified_credit_acknowledgment_is_bound_canonical_and_immutable_after_restart() -> Result<()>
    {
        use lightning_goats_cashu_receipt::acknowledgment::{Acknowledgment, ImportResult};
        let dir = crate::test_directory()?;
        let path = dir.path().join("ack.sqlite");
        let epoch = Uuid::new_v4();
        let instance = Uuid::new_v4();
        let core_epoch = Uuid::new_v4();
        let mut journal = Journal::open(&path, epoch, 8)?;
        let id = new_id(journal.admit(&intake(
            &[proof("ack-fixture")],
            "http-ack",
            Transport::Cashu,
        ))?);
        journal.prepare(id, b"original-swap")?;
        journal.start_acquisition(id)?;
        journal.record_acquired(id, 7, 1, b"verified-owned-outputs")?;
        let receipt = journal.export_receipt(id)?;
        let wire = crate::receipt_wire::from_private(instance, &receipt)?;
        let mut ack = Acknowledgment {
            version: 1,
            wallet_instance: instance,
            wallet_epoch: epoch,
            core_epoch,
            result: ImportResult {
                acquisition_id: id,
                receipt_digest: hex::encode(wire.digest()?),
                event_seq: 42,
                net_sats: 7,
                original_feed_credit_sats: 1000,
            },
        };
        ack.result.net_sats = 8;
        assert!(
            journal
                .acknowledge_verified_credit(instance, core_epoch, id, &ack.encode()?)
                .is_err()
        );
        assert_eq!(journal.state(id)?, "credit_export_pending");
        ack.result.net_sats = 7;
        assert!(
            journal
                .acknowledge_verified_credit(instance, Uuid::new_v4(), id, &ack.encode()?)
                .is_err()
        );
        journal.acknowledge_verified_credit(
            instance,
            core_epoch,
            id,
            &serde_json::to_vec_pretty(&ack)?,
        )?;
        drop(journal);
        let mut journal = Journal::open(&path, epoch, 8)?;
        journal.acknowledge_verified_credit(instance, core_epoch, id, &ack.encode()?)?;
        assert_eq!(journal.state(id)?, "credited");
        ack.result.original_feed_credit_sats = 999;
        assert!(
            journal
                .acknowledge_verified_credit(instance, core_epoch, id, &ack.encode()?)
                .is_err()
        );
        assert!(journal.export_receipt(id)? == receipt);
        Ok(())
    }

    #[test]
    fn overlap_duplicate_entries_and_conflicts_claim_no_fresh_proofs() -> Result<()> {
        let dir = crate::test_directory()?;
        let mut journal = Journal::open(&dir.path().join("intake.sqlite"), Uuid::new_v4(), 8)?;
        let a = vec![proof("a")];
        new_id(journal.admit(&intake(&a, "first", Transport::Cashu))?);
        let overlap = vec![proof("a"), proof("b")];
        assert!(
            journal
                .admit(&intake(&overlap, "overlap", Transport::Nutzap))
                .is_err()
        );
        let duplicate = vec![proof("c"), proof("c")];
        assert!(
            journal
                .admit(&intake(&duplicate, "duplicate", Transport::Cashu))
                .is_err()
        );
        let b = vec![proof("b")];
        assert!(
            journal
                .admit(&intake(&b, "first", Transport::Cashu))
                .is_err()
        );
        new_id(journal.admit(&intake(&b, "fresh", Transport::Cashu))?);
        let c = vec![proof("c")];
        new_id(journal.admit(&intake(&c, "also-fresh", Transport::Cashu))?);
        let mut conflicting = a.clone();
        conflicting[0].amount = Amount::from(16);
        assert!(
            journal
                .admit(&intake(&conflicting, "conflict", Transport::Cashu))
                .is_err()
        );
        Ok(())
    }

    #[test]
    fn request_material_and_epoch_are_frozen_across_restart() -> Result<()> {
        let dir = crate::test_directory()?;
        let path = dir.path().join("intake.sqlite");
        let epoch = Uuid::new_v4();
        let id = {
            let mut journal = Journal::open(&path, epoch, 8)?;
            let id = new_id(journal.admit(&intake(&[proof("a")], "first", Transport::Cashu))?);
            journal.prepare(id, b"original outputs and derivation counters")?;
            journal.start_acquisition(id)?;
            id
        };
        assert!(Journal::open(&path, Uuid::new_v4(), 8).is_err());
        assert!(Journal::open(&path, Uuid::nil(), 8).is_err());
        let mut journal = Journal::open(&path, epoch, 8)?;
        assert_eq!(journal.state(id)?, "acquiring");
        assert_eq!(
            journal.pending_operations(8)?,
            vec![(id, "acquiring".to_owned())]
        );
        assert_eq!(
            journal.prepared_request(id)?.as_deref(),
            Some(b"original outputs and derivation counters".as_slice())
        );
        assert!(journal.start_acquisition(id).is_err());
        journal.prepare(id, b"original outputs and derivation counters")?;
        assert!(journal.prepare(id, b"fresh replacement outputs").is_err());
        assert!(journal.export_receipt(id).is_err());
        assert_eq!(journal.state(id)?, "acquiring");
        Ok(())
    }

    #[test]
    fn admitted_proofs_and_original_author_survive_restart_and_rewrapping() -> Result<()> {
        let dir = crate::test_directory()?;
        let path = dir.path().join("journal.sqlite");
        let epoch = Uuid::new_v4();
        let proofs = vec![proof("author-bound")];
        let original = intake(&proofs, "original-event", Transport::Nutzap);
        let id = {
            let mut journal = Journal::open(&path, epoch, 8)?;
            new_id(journal.admit(&original)?)
        };
        let mut journal = Journal::open(&path, epoch, 8)?;
        assert_eq!(journal.admitted_proofs(id)?, proofs);
        assert_eq!(
            journal.admit(&intake(&proofs, "http-retry", Transport::Cashu))?,
            Admission::Duplicate(id)
        );
        journal.prepare(id, b"fixture original request")?;
        journal.start_acquisition(id)?;
        journal.record_acquired(id, 8, 0, b"fixture owned outputs")?;
        let receipt = journal.export_receipt(id)?;
        assert_eq!(receipt.source_author.as_deref(), original.source_author);
        assert_eq!(receipt.source_ref, "original-event");
        assert_eq!(receipt.transport, "nutzap");
        Ok(())
    }

    #[test]
    fn amount_overflow_and_bounds_leave_no_claims() -> Result<()> {
        let dir = crate::test_directory()?;
        let mut journal = Journal::open(&dir.path().join("intake.sqlite"), Uuid::new_v4(), 2)?;
        let mut huge = proof("huge");
        huge.amount = Amount::from(u64::MAX);
        assert!(
            journal
                .admit(&intake(&[huge.clone()], "huge", Transport::Cashu))
                .is_err()
        );
        huge.amount = Amount::from(8);
        new_id(journal.admit(&intake(&[huge], "valid", Transport::Cashu))?);
        assert!(
            journal
                .admit(&intake(&[], "empty", Transport::Cashu))
                .is_err()
        );
        assert!(
            journal
                .admit(&intake(
                    &[proof("a"), proof("b"), proof("c")],
                    "many",
                    Transport::Cashu
                ))
                .is_err()
        );
        new_id(journal.admit(&intake(&[proof("a")], "fresh", Transport::Cashu))?);
        Ok(())
    }
}

#[cfg(test)]
mod hold_tests {
    use super::*;
    use cdk::{
        Amount,
        nuts::{Id, PublicKey},
        secret::Secret,
    };
    use std::str::FromStr;
    #[test]
    fn held_operation_retains_original_material_and_cannot_export_or_restart() -> Result<()> {
        let dir = crate::test_directory()?;
        let path = dir.path().join("journal.sqlite");
        let epoch = Uuid::new_v4();
        let id = {
            let mut journal = Journal::open(&path, epoch, 8)?;
            let proof = Proof::new(
                Amount::from(1),
                Id::from_str("00deadbeef123456")?,
                Secret::new("held-fixture"),
                PublicKey::from_hex(
                    "0279be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798",
                )?,
            );
            let id = match journal.admit(&ValidatedIntake {
                mint_id: "fixture",
                policy_version: "v1",
                transport: Transport::Cashu,
                source_ref: "source",
                source_author: None,
                proofs: &[proof],
            })? {
                Admission::New(id) => id,
                _ => panic!("new expected"),
            };
            journal.prepare(id, b"original request")?;
            journal.start_acquisition(id)?;
            journal.hold(id, HoldReason::MissingOwnedOutputs)?;
            id
        };
        let mut journal = Journal::open(&path, epoch, 8)?;
        assert_eq!(journal.state(id)?, "held_for_reconciliation");
        assert_eq!(
            journal.hold_reason(id)?.as_deref(),
            Some("missing_owned_outputs")
        );
        assert_eq!(
            journal.prepared_request(id)?.as_deref(),
            Some(b"original request".as_slice())
        );
        journal.hold(id, HoldReason::AmbiguousAcquisition)?;
        assert_eq!(
            journal.hold_reason(id)?.as_deref(),
            Some("missing_owned_outputs")
        );
        assert!(journal.start_acquisition(id).is_err());
        assert!(
            journal
                .record_acquired(id, 1, 0, b"unverified claim")
                .is_err()
        );
        assert!(journal.export_receipt(id).is_err());
        Ok(())
    }
}

mod receive_status;

pub(crate) mod cdk_storage;
