//! TEST ONLY: guarded disposable-CI schema fixture and harmless old-core ELF.
//! Never packaged or installed as a production program. No provider/signing/feed
//! operation exists. The stand-in does not open configuration, state or secrets.
#![forbid(unsafe_code)]

#[path = "../tests/support/breez_schema_fixture.rs"]
mod breez_schema_fixture;

use anyhow::{Result, bail};
use sqlx::{
    Row as _,
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous},
};
use std::{os::unix::fs::MetadataExt as _, path::Path, time::Duration};

const GUARD: &str = "/run/lightning-goats-contact-fixture/ALLOW";
const ATTESTATION: &[u8] = b"LIGHTNING_GOATS_DISPOSABLE_CONTACT_UPGRADE_CI_v1\n";
const DATABASE: &str = "/var/lib/lightning-goats/lightning-goats.db";
const CURRENT_CORE_SCHEMA: usize = 40;

fn isolated_fixture() -> Result<u32> {
    let uid = std::fs::metadata("/proc/self")?.uid();
    if uid == 0 {
        bail!("non-root disposable isolated fixture required");
    }
    let path = Path::new(GUARD);
    for parent in path.ancestors().skip(1) {
        let info = std::fs::symlink_metadata(parent)?;
        if !info.is_dir() || info.uid() != 0 || info.gid() != 0 || info.mode() & 0o022 != 0 {
            bail!("protected disposable fixture guard required");
        }
    }
    let info = std::fs::symlink_metadata(path)?;
    if !info.is_file()
        || info.uid() != 0
        || info.gid() != 0
        || info.nlink() != 1
        || info.mode() & 0o022 != 0
        || info.len() > 256
    {
        bail!("invalid disposable fixture guard");
    }
    // The hardened unit hides PID 1 from its non-root process. The guarded root
    // harness proves isolation first and freezes the expected namespace here;
    // the fixture compares only its own kernel namespace with that root record.
    let namespace = std::fs::read_link("/proc/self/ns/net")?;
    let namespace = namespace
        .to_str()
        .ok_or_else(|| anyhow::anyhow!("invalid fixture namespace"))?;
    let expected = [ATTESTATION, namespace.as_bytes(), b"\n"].concat();
    if !namespace.starts_with("net:[")
        || !namespace.ends_with(']')
        || std::fs::read(path)? != expected
    {
        bail!("fixture namespace differs from the root attestation");
    }
    Ok(uid)
}

// Child-first order preserves foreign-key enforcement. This fixture accepts
// only completely unused Cashu state; it is not a financial downgrade tool.
const CASHU_FIXTURE_TABLES: &[&str] = &[
    "feed_cashu_proceeds",
    "cashu_settled_ranges",
    "cashu_treasury_inspections",
    "cashu_treasury_fee_observations",
    "cashu_sweep_jobs",
    "cashu_sweep_requests",
    "feed_financial_supersessions",
    "feed_financial_treasury",
    "feed_financial_parts",
    "feed_financial_plans",
    "cashu_treasury_work",
    "cashu_treasury_source_results",
    "cashu_treasury_completions",
    "cashu_treasury_wallet_results",
    "cashu_treasury_deposits",
    "cashu_treasury_issuances",
    "cashu_treasury_claims",
    "cashu_treasury_sources",
    "cashu_treasury_intents",
    "cashu_treasury_clock",
    "feed_source_parts",
    "feed_source_reservations",
    "cashu_receipt_inbox",
    "cashu_import_clock",
];
async fn strip_empty_cashu_fixture(
    transaction: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
) -> Result<()> {
    // Inspect the entire extension before any DDL. Even a monotonic clock row
    // means the extension was used and must retain its state.
    for table in CASHU_FIXTURE_TABLES.iter().copied().chain([
        "breez_collected_claims",
        "breez_project_invoices",
        "private_contact_entitlements",
        "private_contact_requests",
        "private_contact_tenders",
        "private_contact_cashu_receipts",
        "private_contact_delivery",
    ]) {
        let count: i64 = sqlx::query_scalar(&format!("SELECT COUNT(*) FROM {table}"))
            .fetch_one(&mut **transaction)
            .await?;
        if count != 0 {
            bail!("Cashu/financial fixture state must be entirely unused");
        }
    }
    breez_schema_fixture::strip_empty(transaction).await?;
    // Rebuild the unused delivery table with its original Lightning-fee FK,
    // indexes and triggers from the checked-in migration, inside this fixture only.
    let original = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await?;
    sqlx::raw_sql(include_str!("../migrations/0024_private_contacts.sql"))
        .execute(&original)
        .await?;
    let delivery_ddl: Vec<String> = sqlx::query_scalar(
        "SELECT sql FROM sqlite_schema WHERE tbl_name='private_contact_delivery' AND sql IS NOT NULL ORDER BY rowid",
    ).fetch_all(&original).await?;
    original.close().await;
    sqlx::raw_sql(
        "DROP VIEW private_contact_excess_receipts;
        DROP TRIGGER private_contact_lightning_entitlement;
        DROP TABLE private_contact_delivery;
        DROP TABLE private_contact_cashu_receipts;
        DROP TABLE private_contact_tenders;
        DROP TABLE private_contact_requests;
        DROP TABLE private_contact_entitlements;",
    )
    .execute(&mut **transaction)
    .await?;
    for statement in delivery_ddl {
        sqlx::query(&statement).execute(&mut **transaction).await?;
    }
    if sqlx::query("DELETE FROM _sqlx_migrations WHERE version=40")
        .execute(&mut **transaction)
        .await?
        .rows_affected()
        != 1
    {
        bail!("synthetic contact migration missing");
    }
    for table in CASHU_FIXTURE_TABLES {
        sqlx::query(&format!("DROP TABLE {table}"))
            .execute(&mut **transaction)
            .await?;
    }
    if sqlx::query("DELETE FROM _sqlx_migrations WHERE version IN (26,27,28,29)")
        .execute(&mut **transaction)
        .await?
        .rows_affected()
        != 4
    {
        bail!("synthetic Cashu migration missing");
    }
    Ok(())
}

async fn strip_fixture_schema(uid: u32, cashu_only: bool) -> Result<()> {
    if std::env::var_os("CREDENTIALS_DIRECTORY").is_some() {
        bail!("synthetic schema fixture must not load credentials");
    }
    let path = Path::new(DATABASE);
    let info = std::fs::symlink_metadata(path)?;
    if !info.is_file() || info.uid() != uid || info.nlink() != 1 || info.mode() & 0o077 != 0 {
        bail!("private existing fixture state required");
    }
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(
            SqliteConnectOptions::new()
                .filename(path)
                .create_if_missing(false)
                .journal_mode(SqliteJournalMode::Wal)
                .synchronous(SqliteSynchronous::Full)
                .foreign_keys(true)
                .pragma("trusted_schema", "OFF")
                .pragma("hard_heap_limit", "268435456")
                .busy_timeout(Duration::from_secs(5)),
        )
        .await?;
    let engine: String = sqlx::query_scalar("SELECT sqlite_version()")
        .fetch_one(&pool)
        .await?;
    if engine != "3.54.1" {
        bail!("wrong fixture engine");
    }
    let migrations = sqlx::migrate!("./migrations");
    let rows = sqlx::query(
        "SELECT version,success,checksum FROM _sqlx_migrations ORDER BY version LIMIT ?",
    )
    .bind(i64::try_from(CURRENT_CORE_SCHEMA + 1)?)
    .fetch_all(&pool)
    .await?;
    if rows.len() != CURRENT_CORE_SCHEMA || migrations.iter().count() != CURRENT_CORE_SCHEMA {
        bail!("exact current disposable schema required");
    }
    for (row, migration) in rows.iter().zip(migrations.iter()) {
        if row.try_get::<i64, _>("version")? != migration.version
            || !row.try_get::<bool, _>("success")?
            || row.try_get::<Vec<u8>, _>("checksum")?.as_slice() != migration.checksum.as_ref()
        {
            bail!("fixture migration identity changed");
        }
    }
    let payments: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM settled_payments")
        .fetch_one(&pool)
        .await?;
    let synthetic: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM settled_payments WHERE source='strike' AND source_id='TEST-ONLY-original-receipt' AND amount_msat=2340000").fetch_one(&pool).await?;
    let signed: Vec<String> = if cashu_only {
        // Later disposable stages can retain more outbox rows. The original
        // signed witness still has to be present.
        sqlx::query_scalar("SELECT signed_event_json FROM message_outbox WHERE json_extract(signed_event_json,'$.content') LIKE 'TEST ONLY: retain exact original signed bytes;%' LIMIT 2")
            .fetch_all(&pool)
            .await?
    } else {
        sqlx::query_scalar("SELECT signed_event_json FROM message_outbox LIMIT 2")
            .fetch_all(&pool)
            .await?
    };
    if payments != 1 || synthetic != 1 || signed.len() != 1 {
        bail!("only the existing synthetic paid/signed fixture may be transformed");
    }
    let event = lightning_goats::nostr_event::VerifiedEvent::from_json(&signed[0])?;
    if !event
        .event()
        .content
        .starts_with("TEST ONLY: retain exact original signed bytes;")
    {
        bail!("not the synthetic signed fixture");
    }
    let mut transaction = pool.begin().await?;
    strip_empty_cashu_fixture(&mut transaction).await?;
    if cashu_only {
        transaction.commit().await?;
        pool.close().await;
        println!(
            "{}",
            serde_json::json!({"synthetic":true,"schema":25,
                 "only_unused_cashu_removed":true,"production_supported":false})
        );
        return Ok(());
    }
    for table in [
        "message_outbox_suppressions",
        "feeding_announcement_sources",
        "feed_credit_retirements",
    ] {
        let count: i64 = sqlx::query_scalar(&format!("SELECT COUNT(*) FROM {table}"))
            .fetch_one(&mut *transaction)
            .await?;
        if count != 0 {
            bail!("synthetic feeder-cap audit must be empty");
        }
        sqlx::query(&format!("DROP TABLE {table}"))
            .execute(&mut *transaction)
            .await?;
    }
    sqlx::query("DROP TABLE feeder_credit_policy")
        .execute(&mut *transaction)
        .await?;
    if sqlx::query("DELETE FROM _sqlx_migrations WHERE version=25")
        .execute(&mut *transaction)
        .await?
        .rows_affected()
        != 1
    {
        bail!("synthetic feeder-cap migration missing");
    }
    for table in [
        "private_contact_delivery",
        "private_contact_fees",
        "private_contact_intents",
    ] {
        let count: i64 = sqlx::query_scalar(&format!("SELECT COUNT(*) FROM {table}"))
            .fetch_one(&mut *transaction)
            .await?;
        if count != 0 {
            bail!("private fixture tables must be empty");
        }
        sqlx::query(&format!("DROP TABLE {table}"))
            .execute(&mut *transaction)
            .await?;
    }
    if sqlx::query("DELETE FROM _sqlx_migrations WHERE version=24")
        .execute(&mut *transaction)
        .await?
        .rows_affected()
        != 1
    {
        bail!("fixture source migration missing");
    }
    transaction.commit().await?;
    pool.close().await;
    println!(
        "{}",
        serde_json::json!({"synthetic":true,"schema":23,"payments":1,"signed_messages":1,"production_supported":false})
    );
    Ok(())
}

async fn diagnose_export(uid: u32) -> Result<()> {
    if std::env::var_os("CREDENTIALS_DIRECTORY").is_some() {
        bail!("diagnostic must not load credentials");
    }
    let path = Path::new(DATABASE);
    let info = std::fs::symlink_metadata(path)?;
    if !info.is_file() || info.uid() != uid || info.nlink() != 1 || info.mode() & 0o077 != 0 {
        bail!("private synthetic diagnostic state required");
    }
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(
            SqliteConnectOptions::new()
                .filename(path)
                .read_only(true)
                .create_if_missing(false)
                .pragma("query_only", "ON")
                .pragma("trusted_schema", "OFF")
                .busy_timeout(Duration::from_secs(5)),
        )
        .await?;
    let synthetic: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM settled_payments WHERE source='strike' AND source_id='TEST-ONLY-original-receipt' AND amount_msat=2340000").fetch_one(&pool).await?;
    let total: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM settled_payments")
        .fetch_one(&pool)
        .await?;
    let identities: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM overlay_identity WHERE singleton=1")
            .fetch_one(&pool)
            .await?;
    let engine: String = sqlx::query_scalar("SELECT sqlite_version()")
        .fetch_one(&pool)
        .await?;
    if synthetic != 1 || total != 1 || identities != 1 || engine != "3.54.1" {
        bail!("not the synthetic original payment diagnostic fixture");
    }
    let rows =
        sqlx::query("SELECT seq,event_type,created_at FROM event_log ORDER BY seq LIMIT 128")
            .fetch_all(&pool)
            .await?;
    let events = rows.into_iter().map(|row| -> Result<_> {
        Ok(serde_json::json!({"seq":row.try_get::<i64,_>("seq")?,
            "type":row.try_get::<String,_>("event_type")?,"created_at":row.try_get::<i64,_>("created_at")?}))
    }).collect::<Result<Vec<_>>>()?;
    pool.close().await;
    // Existing-current-schema opener performs no migration. The existing epoch
    // is required by this synthetic fixture; the export has no external I/O.
    let ledger = lightning_goats::ledger::LedgerStore::connect_existing_recovery(&format!(
        "sqlite://{DATABASE}"
    ))
    .await?;
    let exported = match ledger.core_event_batch(None, 0, 128).await {
        Ok(batch) => {
            serde_json::json!({"accepted":true,"through":batch.through,"latest":batch.latest})
        }
        Err(error) => {
            serde_json::json!({"accepted":false,"synthetic_only_error":format!("{error:#}")})
        }
    };
    println!(
        "{}",
        serde_json::json!({"synthetic_only":true,"events":events,"native_export":exported})
    );
    Ok(())
}

async fn strip_empty_inclusive_fee_schema(uid: u32) -> Result<()> {
    if std::env::var_os("CREDENTIALS_DIRECTORY").is_some() {
        bail!("synthetic fee fixture must not load credentials");
    }
    const PAYOUT: &str = "/var/lib/cyberherd-payout/payout.db";
    let url = format!("sqlite://{PAYOUT}");
    lightning_goats::payout::cost_recovery::existing_private_database(&url)?;
    if std::fs::symlink_metadata(PAYOUT)?.uid() != uid {
        bail!("fixture payout UID changed");
    }
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(
            SqliteConnectOptions::new()
                .filename(PAYOUT)
                .create_if_missing(false)
                .journal_mode(SqliteJournalMode::Wal)
                .synchronous(SqliteSynchronous::Full)
                .foreign_keys(true)
                .pragma("trusted_schema", "OFF")
                .pragma("hard_heap_limit", "268435456")
                .busy_timeout(Duration::from_secs(5)),
        )
        .await?;
    let engine: String = sqlx::query_scalar("SELECT sqlite_version()")
        .fetch_one(&pool)
        .await?;
    if engine != "3.54.1" {
        bail!("wrong native fee fixture engine");
    }
    let migrations = sqlx::migrate!("./payout-migrations");
    if migrations.iter().map(|m| m.version).collect::<Vec<_>>()
        != [1, 2, 3, 4, 5, 6, 8, 9, 10, 11, 12, 13, 14, 15]
    {
        bail!("unqualified fixture payout schema");
    }
    let rows = sqlx::query(
        "SELECT version,success,checksum FROM _sqlx_migrations ORDER BY version LIMIT 15",
    )
    .fetch_all(&pool)
    .await?;
    if rows.len() != migrations.iter().count() {
        bail!("exact current synthetic payout schema required");
    }
    for (row, migration) in rows.iter().zip(migrations.iter()) {
        if row.try_get::<i64, _>("version")? != migration.version
            || !row.try_get::<bool, _>("success")?
            || row.try_get::<Vec<u8>, _>("checksum")?.as_slice() != migration.checksum.as_ref()
        {
            bail!("fixture payout migration identity changed");
        }
    }
    if sqlx::query("PRAGMA foreign_key_check")
        .fetch_optional(&pool)
        .await?
        .is_some()
    {
        bail!("fixture payout journal has inconsistent history");
    }
    let mut tx = pool.begin().await?;
    let grants: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM payout_grants")
        .fetch_one(&mut *tx)
        .await?;
    if grants != 0 {
        bail!("only an empty synthetic payout journal may be transformed");
    }
    sqlx::raw_sql(
        "DROP TRIGGER bind_member_ecash_destination; DROP TRIGGER forbid_member_ecash_invoice; DROP TRIGGER forbid_member_ecash_invoice_closure; DROP TRIGGER check_operator_invoice_method; DROP TRIGGER bind_legacy_operator_method;",
    )
    .execute(&mut *tx)
    .await?;
    for table in [
        "member_ecash_ready",
        "payout_operator_ecash_status",
        "payout_operator_methods",
        "payout_operator_receipts",
        "payout_operator_invoices",
        "payout_operator_allocations",
        "payout_fee_probe_duplicates",
        "payout_fee_promotions",
        "payout_fee_probe_quotes",
        "payout_fee_probe_invoices",
        "payout_fee_probe_callbacks",
        "payout_fee_probe_intents",
        "payout_fee_observations",
    ] {
        let count: i64 = sqlx::query_scalar(&format!("SELECT COUNT(*) FROM {table}"))
            .fetch_one(&mut *tx)
            .await?;
        if count != 0 {
            bail!("fee fixture tables must be empty");
        }
        sqlx::query(&format!("DROP TABLE {table}"))
            .execute(&mut *tx)
            .await?;
    }
    if sqlx::query("DELETE FROM _sqlx_migrations WHERE version IN (9,10,11,12,13,14,15)")
        .execute(&mut *tx)
        .await?
        .rows_affected()
        != 7
    {
        bail!("fixture payout migration prefix changed");
    }
    tx.commit().await?;
    pool.close().await;
    println!(
        "{}",
        serde_json::json!({"synthetic":true,"payout_schema":8,"grants":0,"production_supported":false})
    );
    Ok(())
}

async fn run() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let uid = isolated_fixture()?;
    if args == ["--strip-empty-contact-schema"] {
        strip_fixture_schema(uid, false).await
    } else if args == ["--strip-empty-cashu-schema"] {
        strip_fixture_schema(uid, true).await
    } else if args == ["--strip-empty-inclusive-fee-schema"] {
        strip_empty_inclusive_fee_schema(uid).await
    } else if args == ["--diagnose-installed-export"] {
        diagnose_export(uid).await
    } else if args == ["--config", "/etc/lightning-goats/config.receive.toml"] {
        // Harmless old-core stand-in. Accept the future native core's exact unit
        // argv without opening its config, journal, credential, IPC or network.
        loop {
            std::thread::park_timeout(Duration::from_secs(60));
        }
    } else {
        bail!("unknown disposable fixture operation");
    }
}

#[tokio::main]
async fn main() {
    if run().await.is_err() {
        eprintln!("Test-only contact fixture refused; no production operation supported.");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    async fn current_fixture() -> anyhow::Result<sqlx::SqlitePool> {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(
                sqlx::sqlite::SqliteConnectOptions::new()
                    .in_memory(true)
                    .foreign_keys(true)
                    .pragma("trusted_schema", "OFF"),
            )
            .await?;
        sqlx::migrate!("./migrations").run(&pool).await?;
        Ok(pool)
    }
    #[tokio::test]
    async fn unused_current_cashu_extension_can_be_stripped_and_rolled_back() -> anyhow::Result<()>
    {
        let pool = current_fixture().await?;
        let mut tx = pool.begin().await?;
        super::strip_empty_cashu_fixture(&mut tx).await?;
        let extension: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM _sqlx_migrations WHERE version >= 26")
                .fetch_one(&mut *tx)
                .await?;
        let previous: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM _sqlx_migrations WHERE version=25")
                .fetch_one(&mut *tx)
                .await?;
        assert_eq!((extension, previous), (0, 1));
        tx.rollback().await?;
        let migrations: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM _sqlx_migrations")
            .fetch_one(&pool)
            .await?;
        assert_eq!(usize::try_from(migrations)?, super::CURRENT_CORE_SCHEMA);
        Ok(())
    }
    #[tokio::test]
    async fn any_used_cashu_state_refuses_before_dropping_any_schema_object() -> anyhow::Result<()>
    {
        let pool = current_fixture().await?;
        sqlx::query("INSERT INTO cashu_import_clock VALUES(1,7)")
            .execute(&pool)
            .await?;
        let mut tx = pool.begin().await?;
        let before: Vec<(String, String, Option<String>)> =
            sqlx::query_as("SELECT type,name,sql FROM sqlite_master ORDER BY type,name")
                .fetch_all(&mut *tx)
                .await?;
        assert!(super::strip_empty_cashu_fixture(&mut tx).await.is_err());
        let after: Vec<(String, String, Option<String>)> =
            sqlx::query_as("SELECT type,name,sql FROM sqlite_master ORDER BY type,name")
                .fetch_all(&mut *tx)
                .await?;
        assert_eq!(before, after);
        tx.rollback().await?;
        let time: i64 = sqlx::query_scalar("SELECT last_seen FROM cashu_import_clock")
            .fetch_one(&pool)
            .await?;
        assert_eq!(time, 7);
        Ok(())
    }
    #[test]
    fn disposable_drop_inventory_covers_every_cashu_migration_table() {
        let expected = include_str!("../migrations/0026_cashu_receipts.sql")
            .lines()
            .chain(include_str!("../migrations/0027_cashu_fee_observations.sql").lines())
            .chain(include_str!("../migrations/0028_cashu_treasury_inspections.sql").lines())
            .chain(include_str!("../migrations/0029_cashu_feed_proceeds.sql").lines())
            .filter_map(|line| line.strip_prefix("CREATE TABLE "))
            .map(|line| line.split_whitespace().next().unwrap())
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(
            expected,
            super::CASHU_FIXTURE_TABLES.iter().copied().collect()
        );
    }
    #[test]
    fn ordinary_process_cannot_enter_the_disposable_state_fixture() {
        assert!(super::isolated_fixture().is_err());
    }
}
