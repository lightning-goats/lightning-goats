use super::{Report, Table};
use anyhow::{Result, bail};
use futures_util::TryStreamExt as _;
use sha2::{Digest as _, Sha256};
use sqlx::{
    Row as _, SqlitePool,
    sqlite::{SqliteConnectOptions, SqlitePoolOptions},
};
use std::{collections::BTreeMap, path::Path, time::Duration};

static CORE: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");

async fn open(directory: &Path) -> Result<SqlitePool> {
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(
            SqliteConnectOptions::new()
                .filename(directory.join("core.db"))
                .read_only(true)
                .create_if_missing(false)
                .foreign_keys(true)
                .pragma("query_only", "ON")
                .pragma("trusted_schema", "OFF")
                .pragma("hard_heap_limit", "268435456")
                .busy_timeout(Duration::from_secs(5)),
        )
        .await?;
    let result: Result<()> = async {
        let engine: String = sqlx::query_scalar("SELECT sqlite_version()")
            .fetch_one(&pool)
            .await?;
        if engine != "3.54.1" {
            bail!("legacy core verification requires qualified SQLite 3.54.1");
        }
        let integrity: Vec<String> = sqlx::query_scalar("PRAGMA integrity_check")
            .fetch_all(&pool)
            .await?;
        if integrity != ["ok"]
            || sqlx::query("PRAGMA foreign_key_check")
                .fetch_optional(&pool)
                .await?
                .is_some()
        {
            bail!("legacy core integrity or foreign keys failed");
        }
        Ok(())
    }
    .await;
    if let Err(e) = result {
        pool.close().await;
        return Err(e);
    }
    Ok(pool)
}
async fn schema(pool: &SqlitePool, current: bool) -> Result<Vec<i64>> {
    let rows=sqlx::query("SELECT version,CASE WHEN success=1 THEN 1 ELSE 0 END AS successful,checksum FROM _sqlx_migrations ORDER BY version LIMIT 1024").fetch_all(pool).await?;
    let expected: Vec<_> = CORE.iter().collect();
    let allowed = if current {
        expected.len()
    } else {
        expected.iter().take_while(|m| m.version < 16).count()
    };
    if rows.len() > allowed || rows.len() < 9 || (current && rows.len() != allowed) {
        bail!("requires a successful known legacy prefix, or the exact current comparison schema");
    }
    let mut versions = Vec::new();
    for (row, migration) in rows.iter().zip(expected) {
        let version: i64 = row.try_get("version")?;
        if version != migration.version
            || row.try_get::<i64, _>("successful")? != 1
            || row.try_get::<Vec<u8>, _>("checksum")?.as_slice() != migration.checksum.as_ref()
        {
            bail!("migration prefix is dirty, changed, skipped or unknown");
        }
        versions.push(version);
    }
    Ok(versions)
}
fn identifier(value: &str) -> Result<()> {
    if value.is_empty()
        || value.len() > 128
        || !value
            .bytes()
            .all(|v| v.is_ascii_alphanumeric() || v == b'_')
    {
        bail!("unsupported journal identifier");
    }
    Ok(())
}
async fn names(pool: &SqlitePool) -> Result<Vec<String>> {
    let names:Vec<String>=sqlx::query_scalar("SELECT name FROM sqlite_master WHERE type='table' AND (name NOT LIKE 'sqlite_%' OR name='sqlite_sequence') AND name<>'_sqlx_migrations' ORDER BY name LIMIT 257").fetch_all(pool).await?;
    if names.len() > 256 {
        bail!("too many journal tables");
    }
    for name in &names {
        identifier(name)?;
    }
    Ok(names)
}
async fn columns(pool: &SqlitePool, table: &str) -> Result<Vec<String>> {
    identifier(table)?;
    let metadata = sqlx::query(&format!("PRAGMA table_xinfo(\"{table}\")"))
        .fetch_all(pool)
        .await?;
    if metadata.is_empty() || metadata.len() > 256 {
        bail!("unsupported column count");
    }
    let mut columns = Vec::new();
    for row in metadata {
        let name: String = row.try_get("name")?;
        identifier(&name)?;
        if row.try_get::<i64, _>("hidden")? != 0 {
            bail!("unsupported generated or hidden column");
        }
        columns.push(name);
    }
    Ok(columns)
}
async fn digest(pool: &SqlitePool, table: &str, columns: &[String]) -> Result<Table> {
    identifier(table)?;
    if columns.is_empty() || columns.len() > 256 {
        bail!("invalid comparison column count");
    }
    let mut selected = vec!["rowid".to_owned()];
    for column in columns {
        identifier(column)?;
        selected.push(format!("typeof(\"{column}\"),\"{column}\""));
    }
    let query = format!(
        "SELECT {} FROM \"{table}\" ORDER BY rowid",
        selected.join(",")
    );
    let mut rows = sqlx::query(&query).fetch(pool);
    let mut count = 0u64;
    let mut hash = Sha256::new();
    hash.update(b"lg-legacy-core-table-v1\0");
    hash.update(serde_json::to_vec(columns)?);
    while let Some(row) = rows.try_next().await? {
        count = count
            .checked_add(1)
            .ok_or_else(|| anyhow::anyhow!("row count overflow"))?;
        hash.update(row.try_get::<i64, _>(0)?.to_be_bytes());
        for i in 0..columns.len() {
            let kind: String = row.try_get(i * 2 + 1)?;
            let index = i * 2 + 2;
            // typeof() establishes the actual storage class, independently of
            // declared affinity (e.g. BOOLEAN). No lossy SQL quote()/CAST hash.
            let (tag, bytes) = match kind.as_str() {
                "null" => (0, Vec::new()),
                "integer" => (
                    1,
                    row.try_get_unchecked::<i64, _>(index)?
                        .to_be_bytes()
                        .to_vec(),
                ),
                "real" => (
                    2,
                    row.try_get_unchecked::<f64, _>(index)?
                        .to_bits()
                        .to_be_bytes()
                        .to_vec(),
                ),
                "text" => (3, row.try_get_unchecked::<String, _>(index)?.into_bytes()),
                "blob" => (4, row.try_get_unchecked::<Vec<u8>, _>(index)?),
                _ => bail!("unsupported SQLite storage class"),
            };
            hash.update([tag]);
            hash.update(u64::try_from(bytes.len())?.to_be_bytes());
            hash.update(bytes);
        }
    }
    Ok(Table {
        columns: columns.to_vec(),
        rows: count,
        sha256: hex::encode(hash.finalize()),
    })
}
async fn epoch(pool: &SqlitePool) -> Result<uuid::Uuid> {
    let value: String =
        sqlx::query_scalar("SELECT stream_id FROM overlay_identity WHERE singleton=1")
            .fetch_one(pool)
            .await?;
    let value: uuid::Uuid = value.parse()?;
    if value.is_nil() {
        bail!("core epoch is not initialized");
    }
    Ok(value)
}
pub(super) async fn inspect(directory: &Path) -> Result<Report> {
    let pool = open(directory).await?;
    let result: Result<Report> = async {
        let migrations = schema(&pool, false).await?;
        let names = names(&pool).await?;
        if names.iter().any(|name| {
            name.starts_with("reward_")
                || name.starts_with("payout_")
                || name.starts_with("feed_reward_")
        }) {
            bail!("CyberHerd financial schemas require four-journal maintenance");
        }
        let epoch = epoch(&pool).await?;
        let mut tables = BTreeMap::new();
        for name in names {
            tables.insert(
                name.clone(),
                digest(&pool, &name, &columns(&pool, &name).await?).await?,
            );
        }
        Ok(Report {
            engine: "3.54.1".into(),
            epoch,
            migrations,
            tables,
        })
    }
    .await;
    pool.close().await;
    result
}
pub(super) async fn compare(directory: &Path, original: &Report) -> Result<()> {
    let pool = open(directory).await?;
    let result: Result<()> = async {
        schema(&pool, true).await?;
        if epoch(&pool).await? != original.epoch {
            bail!("migration changed the existing core epoch");
        }
        for (name, before) in &original.tables {
            if digest(&pool, name, &before.columns).await? != *before {
                bail!("migration changed retained legacy rows, IDs or signed bytes");
            }
        }
        for table in [
            "reward_funding_state",
            "reward_funding_lots",
            "feed_reward_funding",
            "feed_reward_funding_parts",
            "reward_reservations",
            "reward_outcome_manifests",
            "reward_outcome_rows",
            "reward_outcome_finals",
            "payout_execution_claims",
            "payout_failed_costs",
        ] {
            let count: i64 = sqlx::query_scalar(&format!("SELECT COUNT(*) FROM {table}"))
                .fetch_one(&pool)
                .await?;
            if count != 0 {
                bail!("financial work exists; upgrade comparison is pre-activation only");
            }
        }
        Ok(())
    }
    .await;
    pool.close().await;
    result
}
