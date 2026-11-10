//! Shared durable-storage boundary for the financial ledger and physical owner ledger.

use std::{str::FromStr, time::Duration};

use anyhow::{Context, Result, bail};
use sqlx::{
    Connection, Row, SqliteConnection, SqlitePool,
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous},
};

/// Accept filesystem SQLite URLs, without a second native URI interpretation or
/// connection modes that defeat writable, persistent state. Parse with the same
/// SQLx parser used to open the database; a URL-prefix check is not sufficient.
pub(crate) fn durable_options(database_url: &str) -> Result<SqliteConnectOptions> {
    let rest = database_url
        .strip_prefix("sqlite://")
        .context("SQLite database must use a file-backed sqlite:// URL")?;
    if rest.starts_with("sqlite:") {
        bail!("SQLite database must not contain a repeated scheme");
    }
    let options = SqliteConnectOptions::from_str(database_url).context("invalid SQLite URL")?;
    let filename = options
        .get_filename()
        .to_str()
        .context("SQLite database filename must be UTF-8")?;
    if filename.is_empty() || filename == ":memory:" || filename.starts_with("file:") {
        bail!(
            "SQLite database must use a nonempty filesystem path, not memory or a native file: URI"
        );
    }
    if let Some((_, query)) = rest.split_once('?') {
        // Inspect every decoded occurrence: SQLx keeps mode=memory/read-only
        // flags even if a later duplicate mode says rw. Custom VFS and immutable
        // modes are not part of the writable filesystem storage contract.
        for (key, value) in form_urlencoded::parse(query.as_bytes()) {
            match (key.as_ref(), value.as_ref()) {
                ("mode", "rw" | "rwc")
                | ("cache", "private" | "shared")
                | ("immutable", "false" | "0") => {}
                _ => bail!("SQLite database URL has an unsupported storage option"),
            }
        }
    }
    Ok(options
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Wal)
        .synchronous(SqliteSynchronous::Full)
        .foreign_keys(true)
        .busy_timeout(Duration::from_secs(5)))
}

async fn verify_connection(connection: &mut SqliteConnection) -> Result<(), sqlx::Error> {
    let databases = sqlx::query("PRAGMA database_list")
        .fetch_all(&mut *connection)
        .await?;
    let file_backed = databases.iter().any(|row| {
        row.get::<String, _>("name") == "main" && !row.get::<String, _>("file").is_empty()
    });
    let journal: String = sqlx::query_scalar("PRAGMA main.journal_mode")
        .fetch_one(&mut *connection)
        .await?;
    let synchronous: i64 = sqlx::query_scalar("PRAGMA main.synchronous")
        .fetch_one(&mut *connection)
        .await?;
    let foreign_keys: i64 = sqlx::query_scalar("PRAGMA foreign_keys")
        .fetch_one(&mut *connection)
        .await?;
    if !file_backed || journal != "wal" || synchronous != 2 || foreign_keys != 1 {
        return Err(sqlx::Error::Protocol(
            "SQLite database must be file-backed with effective WAL, FULL synchronous and foreign keys enabled".to_owned(),
        ));
    }
    Ok(())
}

pub(crate) async fn connect_durable(
    database_url: &str,
    max_connections: u32,
) -> Result<SqlitePool> {
    connect_options(durable_options(database_url)?, max_connections).await
}

/// Recovery may open an existing store, but must not recreate one removed after
/// its protected-path checks or while SQLx opens another connection.
pub(crate) async fn connect_existing_durable(
    database_url: &str,
    max_connections: u32,
) -> Result<SqlitePool> {
    connect_options(
        durable_options(database_url)?.create_if_missing(false),
        max_connections,
    )
    .await
}

async fn connect_options(
    options: SqliteConnectOptions,
    max_connections: u32,
) -> Result<SqlitePool> {
    #[cfg(unix)]
    check_recovery_marker(options.get_filename())?;
    // Fail deterministic storage errors before the pool's after_connect retry
    // loop, and before either caller creates tables or runs migrations.
    let mut preflight = SqliteConnection::connect_with(&options)
        .await
        .context("failed opening durable SQLite database")?;
    let verified = verify_connection(&mut preflight).await;
    preflight.close().await?;
    verified.context("SQLite durability check failed")?;

    SqlitePoolOptions::new()
        .max_connections(max_connections)
        .after_connect(|connection, _| Box::pin(verify_connection(connection)))
        .connect_with(options)
        .await
        .context("failed opening durable SQLite pool")
}

#[cfg(unix)]
fn check_recovery_marker(database: &std::path::Path) -> Result<()> {
    let parent = database.parent().context("database parent is missing")?;
    match std::fs::symlink_metadata(parent.join(crate::state_maintenance::RECOVERY_MARKER)) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
        Ok(_) => bail!("restored journal requires explicit external reconciliation before startup"),
    }
}

/// Recovery tools never initialize/upgrade a selected database. Its complete
/// successful migration history must already match this reviewed binary.
pub(crate) async fn require_current_schema(
    pool: &SqlitePool,
    migrator: &sqlx::migrate::Migrator,
) -> Result<()> {
    let rows =
        sqlx::query("SELECT version,success,checksum FROM _sqlx_migrations ORDER BY version")
            .fetch_all(pool)
            .await?;
    let expected: Vec<_> = migrator.iter().collect();
    if rows.len() != expected.len() {
        bail!(
            "recovery requires the already-installed current schema; it cannot initialize or migrate a database"
        );
    }
    for (row, migration) in rows.iter().zip(expected) {
        if row.try_get::<i64, _>("version")? != migration.version
            || !row.try_get::<bool, _>("success")?
            || row.try_get::<Vec<u8>, _>("checksum")?.as_slice() != migration.checksum.as_ref()
        {
            bail!("recovery schema history is dirty, changed or unsupported");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn native_engine_is_the_reviewed_sqlite_release_with_the_wal_memory_fix() {
        let directory = TempDir::new().unwrap();
        let url = format!("sqlite://{}/engine.db", directory.path().display());
        let pool = connect_durable(&url, 1).await.unwrap();
        let version: String = sqlx::query_scalar("SELECT sqlite_version()")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(
            version, "3.54.1",
            "the executable must use the reviewed patched native engine, not a manifest-only or system-library substitution"
        );
        pool.close().await;
    }

    #[tokio::test]
    async fn existing_only_open_cannot_recreate_a_database_after_path_validation() {
        let directory = TempDir::new().unwrap();
        let path = directory.path().join("missing.db");
        let url = format!("sqlite://{}", path.display());
        assert!(connect_existing_durable(&url, 1).await.is_err());
        assert!(!path.exists());
        let pool = connect_durable(&url, 1).await.unwrap();
        pool.close().await;
        connect_existing_durable(&url, 1)
            .await
            .unwrap()
            .close()
            .await;
        std::fs::remove_file(&path).unwrap();
        assert!(connect_existing_durable(&url, 1).await.is_err());
        assert!(!path.exists());
    }

    #[tokio::test]
    async fn every_pool_connection_enforces_effective_durability() {
        let directory = TempDir::new().unwrap();
        let url = format!("sqlite://{}/ledger.db", directory.path().display());
        let pool = connect_durable(&url, 5).await.unwrap();
        let mut connections = Vec::new();
        for _ in 0..5 {
            let mut connection = pool.acquire().await.unwrap();
            verify_connection(&mut connection).await.unwrap();
            connections.push(connection);
        }
        drop(connections);
        pool.close().await;
    }

    #[tokio::test]
    async fn runtime_check_rejects_memory_and_weakened_pragmas() {
        for url in ["sqlite::memory:", "sqlite:///volatile?vfs=memdb"] {
            let mut connection = SqliteConnection::connect(url).await.unwrap();
            assert!(verify_connection(&mut connection).await.is_err(), "{url}");
            connection.close().await.unwrap();
        }
        for pragma in [
            "PRAGMA journal_mode=DELETE",
            "PRAGMA synchronous=NORMAL",
            "PRAGMA foreign_keys=OFF",
        ] {
            let directory = TempDir::new().unwrap();
            let url = format!("sqlite://{}/ledger.db", directory.path().display());
            let mut connection = SqliteConnection::connect_with(&durable_options(&url).unwrap())
                .await
                .unwrap();
            verify_connection(&mut connection).await.unwrap();
            sqlx::query(pragma).execute(&mut connection).await.unwrap();
            assert!(
                verify_connection(&mut connection).await.is_err(),
                "{pragma}"
            );
            connection.close().await.unwrap();
        }
    }

    #[tokio::test]
    async fn supported_disk_paths_and_options_stay_durable() {
        let directory = TempDir::new().unwrap();
        for (name, query) in [
            ("ordinary.db", ""),
            ("encoded%20space%3F%23.db", "?mode=rwc&cache=shared"),
            ("private.db", "?mode=rw&cache=private&immutable=false"),
            ("encoded-options.db", "?%6dode=rw&immutable=0"),
        ] {
            let url = format!("sqlite://{}/{name}{query}", directory.path().display());
            let pool = connect_durable(&url, 1).await.unwrap();
            pool.close().await;
        }
        assert!(directory.path().join("encoded space?#.db").is_file());
        assert_eq!(
            durable_options("sqlite://relative.db")
                .unwrap()
                .get_filename(),
            std::path::Path::new("relative.db")
        );
        for query in [
            "?mode=ro",
            "?mode=ro&mode=rwc",
            "?immutable=true",
            "?immutable=1",
            "?vfs=unix",
        ] {
            assert!(durable_options(&format!("sqlite://ledger.db{query}")).is_err());
        }
    }
}
