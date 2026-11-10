//! TEST ONLY: new synthetic historical journal for a disposable CI installation.
//! Never packaged as a production binary; refuses root, existing files and paths
//! outside /run. It contains no real payment, key, food action or provider call.
#![forbid(unsafe_code)]
use anyhow::{Context, Result, bail};
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous};
use std::{fs::OpenOptions, os::unix::fs::OpenOptionsExt, path::PathBuf};

#[tokio::main]
async fn main() -> Result<()> {
    let path = PathBuf::from(std::env::args().nth(1).context("new /run fixture path")?);
    let parent = path.parent().context("fixture parent")?;
    if tokio::net::UnixStream::pair()?.0.peer_cred()?.uid() == 0
        || !path.starts_with("/run")
        || path.file_name().is_none()
        || parent.canonicalize()? != parent
    {
        bail!("fixture requires a non-root UID and a new canonical /run file");
    }
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&path)?;
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(
            SqliteConnectOptions::new()
                .filename(path)
                .journal_mode(SqliteJournalMode::Wal)
                .synchronous(SqliteSynchronous::Full)
                .foreign_keys(true),
        )
        .await?;
    let mut migrations = sqlx::migrate!("./migrations");
    migrations.migrations.to_mut().retain(|m| m.version <= 13);
    migrations.run(&pool).await?;
    let epoch = uuid::Uuid::new_v4();
    sqlx::query("INSERT INTO overlay_identity(singleton,stream_id) VALUES(1,?)")
        .bind(epoch.to_string())
        .execute(&pool)
        .await?;
    let engine: String = sqlx::query_scalar("SELECT sqlite_version()")
        .fetch_one(&pool)
        .await?;
    if engine != "3.54.1" {
        bail!("fixture used a different native engine");
    }
    pool.close().await;
    println!(
        "{}",
        serde_json::json!({"synthetic":true,"epoch":epoch,"engine":engine,"payments":0})
    );
    Ok(())
}
