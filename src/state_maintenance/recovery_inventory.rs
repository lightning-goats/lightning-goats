//! Private, complete inventory of known reconciliation identities in an accepted
//! backup. It cannot establish coverage of external effects missing from it.
use super::{Operator, Role, files, legacy::rows};
use anyhow::{Context, Result, ensure};
use futures_util::TryStreamExt as _;
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};
use sqlx::{Row as _, SqlitePool};
use std::{
    collections::BTreeMap,
    fs::OpenOptions,
    io::{BufWriter, Read as _, Write},
    os::unix::fs::OpenOptionsExt as _,
    path::Path,
};
use uuid::Uuid;

const ROW_LIMIT: usize = 16 * 1024;
struct Selection {
    role: Role,
    table: &'static str,
    kind: &'static str,
    query: &'static str,
}
const SELECTIONS: &[Selection] = &[
    Selection {
        role: Role::Core,
        table: "strike_receive_requests",
        kind: "receive_request",
        query: "SELECT json_object('kind','receive_request','receive_request_id',receive_request_id,'payment_hash',payment_hash,'amount_msat',amount_msat,'created_at',created_at) AS body FROM strike_receive_requests ORDER BY receive_request_id",
    },
    Selection {
        role: Role::Core,
        table: "strike_inbox",
        kind: "receive_notification",
        query: "SELECT json_object('kind','receive_notification','work_key',work_key,'event_id',event_id,'receive_request_id',receive_request_id,'receive_id',receive_id,'state',status,'notification_only',json('true')) AS body FROM strike_inbox ORDER BY work_key",
    },
    Selection {
        role: Role::Core,
        table: "settled_payments",
        kind: "settled_payment",
        query: "SELECT json_object('kind','settled_payment','source',source,'source_id',source_id,'payment_hash',payment_hash,'amount_msat',amount_msat,'settled_at',settled_at,'received_at',received_at) AS body FROM settled_payments ORDER BY source,source_id",
    },
    Selection {
        role: Role::Core,
        table: "feed_attempts",
        kind: "feeder_request",
        query: "SELECT json_object('kind','feeder_request','request_id',id,'state',status,'threshold_sats',threshold_sats,'created_at',created_at,'resolved_at',resolved_at) AS body FROM feed_attempts ORDER BY id",
    },
    Selection {
        role: Role::Core,
        table: "payout_execution_claims",
        kind: "core_execution_claim",
        query: "SELECT json_object('kind','core_execution_claim','claim_id',id,'reservation_id',reservation_id,'member',member,'quote_id',quote_id,'quote_intent_id',quote_intent_id,'invoice_id',invoice_id,'invoice_hash',invoice_hash,'gross_sats',gross_sats,'created_at',created_at) AS body FROM payout_execution_claims ORDER BY id",
    },
    Selection {
        role: Role::Payout,
        table: "payout_invoice_intents",
        kind: "invoice_intent",
        query: "SELECT json_object('kind','invoice_intent','intent_id',id,'grant_id',grant_id,'member',member) AS body FROM payout_invoice_intents ORDER BY id",
    },
    Selection {
        role: Role::Payout,
        table: "payout_quote_intents",
        kind: "quote_intent",
        query: "SELECT json_object('kind','quote_intent','intent_id',q.id,'idempotency_key',q.id,'invoice_id',q.invoice_id,'retired_at',q.retired_at,'provider_quote_id',r.provider_quote_id) AS body FROM payout_quote_intents q LEFT JOIN payout_quote_records r ON r.intent_id=q.id ORDER BY q.id",
    },
    Selection {
        role: Role::Payout,
        table: "payout_quote_duplicates",
        kind: "duplicate_quote",
        query: "SELECT json_object('kind','duplicate_quote','intent_id',intent_id,'provider_quote_id',provider_quote_id) AS body FROM payout_quote_duplicates ORDER BY intent_id",
    },
    Selection {
        role: Role::Payout,
        table: "payout_execute_intents",
        kind: "execute_intent",
        query: "SELECT json_object('kind','execute_intent','intent_id',intent_id,'created_at',created_at) AS body FROM payout_execute_intents ORDER BY intent_id",
    },
    Selection {
        role: Role::Payout,
        table: "payout_payment_bindings",
        kind: "outgoing_payment",
        query: "SELECT json_object('kind','outgoing_payment','payment_id',b.payment_id,'intent_id',b.intent_id,'bound_at',b.bound_at,'latest_state',(SELECT state FROM payout_payment_records r WHERE r.payment_id=b.payment_id ORDER BY seq DESC LIMIT 1)) AS body FROM payout_payment_bindings b ORDER BY b.payment_id",
    },
    Selection {
        role: Role::Payout,
        table: "payout_payment_conflicts",
        kind: "payment_conflict",
        query: "SELECT json_object('kind','payment_conflict','payment_id',payment_id,'observed_at',observed_at) AS body FROM payout_payment_conflicts ORDER BY payment_id",
    },
    Selection {
        role: Role::Payout,
        table: "payout_invoices",
        kind: "validated_invoice",
        query: "SELECT json_object('kind','validated_invoice','intent_id',intent_id,'payment_hash',payment_hash,'validated_at',validated_at) AS body FROM payout_invoices ORDER BY intent_id",
    },
    Selection {
        role: Role::Payout,
        table: "payout_fee_probe_intents",
        kind: "fee_probe_intent",
        query: "SELECT json_object('kind','fee_probe_intent','intent_id',id,'grant_id',grant_id,'member',member,'round',round,'previous_quote_id',previous_quote_id,'net_sats',net_sats,'created_at',created_at,'discovery_only',json('true')) AS body FROM payout_fee_probe_intents ORDER BY id",
    },
    Selection {
        role: Role::Payout,
        table: "payout_fee_probe_invoices",
        kind: "fee_probe_invoice",
        query: "SELECT json_object('kind','fee_probe_invoice','intent_id',intent_id,'payment_hash',payment_hash,'validated_at',validated_at,'discovery_only',json('true')) AS body FROM payout_fee_probe_invoices ORDER BY intent_id",
    },
    Selection {
        role: Role::Payout,
        table: "payout_fee_probe_quotes",
        kind: "fee_probe_quote",
        query: "SELECT json_object('kind','fee_probe_quote','intent_id',id,'idempotency_key',id,'invoice_id',invoice_id,'discovery_only',json('true')) AS body FROM payout_fee_probe_quotes ORDER BY id",
    },
    Selection {
        role: Role::Payout,
        table: "payout_fee_observations",
        kind: "fee_observation",
        query: "SELECT json_object('kind','fee_observation','provider_quote_id',provider_quote_id,'grant_id',grant_id,'member',member,'round',round,'gross_sats',gross_sats,'invoice_net_sats',invoice_net_sats,'fee_sats',fee_sats,'total_sats',total_sats,'observed_at',observed_at,'discovery_only',json('true')) AS body FROM payout_fee_observations ORDER BY provider_quote_id",
    },
    Selection {
        role: Role::Payout,
        table: "payout_fee_promotions",
        kind: "fee_promotion",
        query: "SELECT json_object('kind','fee_promotion','grant_id',grant_id,'member',member,'invoice_id',invoice_id,'quote_intent_id',quote_id,'provider_quote_id',provider_quote_id,'created_at',created_at) AS body FROM payout_fee_promotions ORDER BY grant_id,member",
    },
];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct RecoveryInventory {
    pub version: u8,
    pub id: Uuid,
    pub bundle_id: Uuid,
    pub bundle_created_at: i64,
    pub bundle_manifest_sha256: String,
    pub policy_sha256: String,
    pub generated_at: i64,
    pub records_file: String,
    pub records_sha256: String,
    pub records_bytes: u64,
    pub counts: BTreeMap<String, u64>,
    pub all_selected_rows_examined: bool,
    pub external_history_complete: bool,
    pub adoption_authorized: bool,
    pub resume_authorized: bool,
}

struct Sink {
    writer: BufWriter<std::fs::File>,
    hash: Sha256,
    bytes: u64,
}
impl Sink {
    fn open(path: &Path) -> Result<Self> {
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(path)?;
        Ok(Self {
            writer: BufWriter::with_capacity(64 * 1024, file),
            hash: Sha256::new(),
            bytes: 0,
        })
    }
    fn emit(&mut self, raw: &str, kind: &str) -> Result<()> {
        ensure!(
            raw.len() <= ROW_LIMIT && !raw.contains(['\n', '\r']),
            "recovery identity row exceeds bound or framing"
        );
        let value: serde_json::Value = serde_json::from_str(raw)
            .map_err(|_| anyhow::anyhow!("invalid recovery identity encoding"))?;
        ensure!(
            value["kind"].as_str() == Some(kind),
            "recovery identity kind differs"
        );
        self.writer.write_all(raw.as_bytes())?;
        self.writer.write_all(b"\n")?;
        self.hash.update(raw.as_bytes());
        self.hash.update(b"\n");
        self.bytes = self
            .bytes
            .checked_add(u64::try_from(raw.len())? + 1)
            .context("recovery inventory size overflow")?;
        Ok(())
    }
    fn finish(mut self) -> Result<(u64, String)> {
        self.writer.flush()?;
        self.writer.get_ref().sync_all()?;
        Ok((self.bytes, hex::encode(self.hash.finalize())))
    }
}
async fn stream(
    pool: &SqlitePool,
    selection: &Selection,
    sink: &mut Sink,
    expected: u64,
) -> Result<()> {
    let mut found = 0u64;
    let mut query = sqlx::query(selection.query).fetch(pool);
    while let Some(row) = query.try_next().await? {
        let raw: String = row.try_get("body")?;
        sink.emit(&raw, selection.kind)?;
        found = found
            .checked_add(1)
            .context("recovery inventory count overflow")?;
    }
    ensure!(
        found == expected,
        "recovery inventory differs from accepted table count"
    );
    Ok(())
}

impl Operator {
    /// Export known IDs only. No live journal, external request, marker clearance
    /// or service operation; an empty inventory cannot authorize recovery.
    pub async fn inventory_recovery(
        &self,
        bundle: &Path,
        destination: &Path,
    ) -> Result<RecoveryInventory> {
        files::absolute(destination)?;
        ensure!(
            !destination.starts_with(bundle) && !bundle.starts_with(destination),
            "recovery inventory must be separate from its accepted backup"
        );
        let manifest = self.verify(bundle).await?;
        let original = files::read_private(&bundle.join("MANIFEST.json"), 65536)?;
        files::new_private_directory(destination)?;
        files::write_new(
            &destination.join("INCOMPLETE"),
            b"Recovery inventory is incomplete; no adoption or resume authority.\n",
        )?;
        let mut sink = Sink::open(&destination.join("IDENTITIES.jsonl"))?;
        let mut counts = BTreeMap::new();
        for role in [Role::Core, Role::Payout] {
            let pool = rows::open_named(bundle, &role.file()).await?;
            let result: Result<()> = async {
                for selection in SELECTIONS.iter().filter(|s| s.role == role) {
                    let expected = *manifest
                        .report
                        .rows
                        .get(&role)
                        .and_then(|tables| tables.get(selection.table))
                        .context("accepted backup lacks selected identity table")?;
                    stream(&pool, selection, &mut sink, expected).await?;
                    ensure!(
                        counts.insert(selection.kind.to_owned(), expected).is_none(),
                        "recovery identity kind repeats"
                    );
                }
                Ok(())
            }
            .await;
            pool.close().await;
            result?;
        }
        let (records_bytes, records_sha256) = sink.finish()?;
        let mut recorded = std::fs::File::open(destination.join("IDENTITIES.jsonl"))?;
        let mut hash = Sha256::new();
        let mut checked_bytes = 0u64;
        let mut buffer = [0u8; 65536];
        loop {
            let count = recorded.read(&mut buffer)?;
            if count == 0 {
                break;
            }
            checked_bytes = checked_bytes
                .checked_add(u64::try_from(count)?)
                .context("inventory verification size overflow")?;
            ensure!(
                checked_bytes <= records_bytes,
                "recovery inventory grew after writing"
            );
            hash.update(&buffer[..count]);
        }
        ensure!(
            checked_bytes == records_bytes && hex::encode(hash.finalize()) == records_sha256,
            "recovery inventory readback differs"
        );

        ensure!(
            self.verify(bundle).await? == manifest
                && files::read_private(&bundle.join("MANIFEST.json"), 65536)? == original,
            "accepted backup changed during recovery inventory"
        );
        let report = RecoveryInventory {
            version: 1,
            id: Uuid::new_v4(),
            bundle_id: manifest.id,
            bundle_created_at: manifest.created_at,
            bundle_manifest_sha256: files::hash_bytes(&original),
            policy_sha256: self.policy_hash.clone(),
            generated_at: crate::cyberherd::calendar::now()?,
            records_file: "IDENTITIES.jsonl".into(),
            records_sha256,
            records_bytes,
            counts,
            all_selected_rows_examined: true,
            external_history_complete: false,
            adoption_authorized: false,
            resume_authorized: false,
        };
        files::write_new(
            &destination.join("INVENTORY.json"),
            &serde_json::to_vec_pretty(&report)?,
        )?;
        files::sync_directory(destination)?;
        std::fs::remove_file(destination.join("INCOMPLETE"))?;
        files::sync_directory(destination)?;
        Ok(report)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn streams_every_identity_beyond_review_example_limits_with_stable_hashes() {
        let root = tempfile::tempdir().unwrap();
        let core = crate::ledger::LedgerStore::connect(&format!(
            "sqlite://{}",
            root.path().join("core.db").display()
        ))
        .await
        .unwrap();
        for number in 0..129 {
            core.record_payment(&crate::domain::payment::SettledPayment {
                source: "test".into(),
                source_id: format!("payment-{number:03}"),
                payment_hash: None,
                address_user: "herd".into(),
                credit_pool: "herd".into(),
                amount_msat: 1000,
                settled_at: None,
                context_json: None,
            })
            .await
            .unwrap();
        }
        let pool = rows::open_named(root.path(), "core.db").await.unwrap();
        let selection = SELECTIONS
            .iter()
            .find(|s| s.kind == "settled_payment")
            .unwrap();
        let mut outputs = Vec::new();
        for name in ["first.jsonl", "second.jsonl"] {
            let mut sink = Sink::open(&root.path().join(name)).unwrap();
            stream(&pool, selection, &mut sink, 129).await.unwrap();
            let (size, digest) = sink.finish().unwrap();
            let raw = std::fs::read(root.path().join(name)).unwrap();
            assert_eq!(raw.len() as u64, size);
            assert_eq!(files::hash_bytes(&raw), digest);
            let records: Vec<serde_json::Value> = std::str::from_utf8(&raw)
                .unwrap()
                .lines()
                .map(|line| serde_json::from_str(line).unwrap())
                .collect();
            assert_eq!(records.len(), 129);
            assert_eq!(records[0]["source_id"], "payment-000");
            assert_eq!(records[128]["source_id"], "payment-128");
            assert!(
                records
                    .iter()
                    .all(|record| record.get("context_json").is_none())
            );
            outputs.push(raw);
        }
        assert_eq!(outputs[0], outputs[1]);
        let mut refused = Sink::open(&root.path().join("wrong-count.jsonl")).unwrap();
        assert!(stream(&pool, selection, &mut refused, 128).await.is_err());
        pool.close().await;
    }

    #[tokio::test]
    async fn original_quote_keys_provider_ids_and_latest_payment_states_stay_distinct() {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        for statement in [
            "CREATE TABLE payout_fee_probe_quotes(id TEXT,invoice_id TEXT,document_json TEXT)",
            "CREATE TABLE payout_quote_intents(id TEXT,invoice_id TEXT,retired_at INTEGER)",
            "CREATE TABLE payout_quote_records(intent_id TEXT,provider_quote_id TEXT)",
            "CREATE TABLE payout_payment_bindings(payment_id TEXT,intent_id TEXT,bound_at INTEGER)",
            "CREATE TABLE payout_payment_records(seq INTEGER,payment_id TEXT,state TEXT)",
            "INSERT INTO payout_fee_probe_quotes VALUES('quote-intent','invoice-intent','private-invoice-context')",
            "INSERT INTO payout_quote_intents VALUES('quote-intent','invoice-intent',NULL)",
            "INSERT INTO payout_quote_records VALUES('quote-intent','provider-quote')",
            "INSERT INTO payout_payment_bindings VALUES('provider-payment','quote-intent',100)",
            "INSERT INTO payout_payment_records VALUES(1,'provider-payment','PENDING'),(2,'provider-payment','COMPLETED'),(3,'another-payment','FAILED')",
        ] {
            sqlx::query(statement).execute(&pool).await.unwrap();
        }
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("identities.jsonl");
        let mut sink = Sink::open(&path).unwrap();
        for kind in ["fee_probe_quote", "quote_intent", "outgoing_payment"] {
            let selection = SELECTIONS.iter().find(|s| s.kind == kind).unwrap();
            stream(&pool, selection, &mut sink, 1).await.unwrap();
        }
        sink.finish().unwrap();
        let raw = std::fs::read_to_string(&path).unwrap();
        let records: Vec<serde_json::Value> = raw
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(records[0]["intent_id"], "quote-intent");
        assert_eq!(records[0]["idempotency_key"], "quote-intent");
        assert_eq!(records[0]["invoice_id"], "invoice-intent");
        assert_eq!(records[0]["discovery_only"], true);
        assert_eq!(records[1]["provider_quote_id"], "provider-quote");
        assert_eq!(records[1]["idempotency_key"], "quote-intent");
        assert_eq!(records[2]["payment_id"], "provider-payment");
        assert_eq!(records[2]["intent_id"], "quote-intent");
        assert_eq!(records[2]["latest_state"], "COMPLETED");
        assert!(!raw.contains("private-invoice-context"));
        pool.close().await;
    }

    #[test]
    fn malformed_oversized_or_wrong_kind_rows_never_enter_the_inventory() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("records.jsonl");
        let mut sink = Sink::open(&path).unwrap();
        assert!(
            sink.emit("not JSON private-record", "feeder_request")
                .is_err()
        );
        assert!(
            sink.emit(&"x".repeat(ROW_LIMIT + 1), "feeder_request")
                .is_err()
        );
        assert!(
            sink.emit(r#"{"kind":"outgoing_payment"}"#, "feeder_request")
                .is_err()
        );
        assert!(
            sink.emit("{\n\"kind\":\"feeder_request\"}", "feeder_request")
                .is_err()
        );
        let (bytes, digest) = sink.finish().unwrap();
        assert_eq!(bytes, 0);
        assert_eq!(digest, files::hash_bytes(b""));
        assert!(Sink::open(&path).is_err());
    }
}
