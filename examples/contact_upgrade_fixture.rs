//! TEST ONLY: guarded disposable-CI schema fixture and harmless old-core ELF.
//! Never packaged or installed as a production program. No provider/signing/feed
//! operation exists. The stand-in does not open configuration, state or secrets.
#![forbid(unsafe_code)]

use anyhow::{Result, bail};
use sqlx::{
    Row as _,
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous},
};
use std::{os::unix::fs::MetadataExt as _, path::Path, time::Duration};

const GUARD: &str = "/run/lightning-goats-contact-fixture/ALLOW";
const ATTESTATION: &[u8] = b"LIGHTNING_GOATS_DISPOSABLE_CONTACT_UPGRADE_CI_v1\n";
const DATABASE: &str = "/var/lib/lightning-goats/lightning-goats.db";

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

async fn strip_empty_contact_schema(uid: u32) -> Result<()> {
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
        "SELECT version,success,checksum FROM _sqlx_migrations ORDER BY version LIMIT 25",
    )
    .fetch_all(&pool)
    .await?;
    if rows.len() != 24 || migrations.iter().count() != 24 {
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
    let signed: Vec<String> =
        sqlx::query_scalar("SELECT signed_event_json FROM message_outbox LIMIT 2")
            .fetch_all(&pool)
            .await?;
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

async fn run() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let uid = isolated_fixture()?;
    if args == ["--strip-empty-contact-schema"] {
        strip_empty_contact_schema(uid).await
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
    #[test]
    fn ordinary_process_cannot_enter_the_disposable_state_fixture() {
        assert!(super::isolated_fixture().is_err());
    }
}
