use std::{str::FromStr, time::Duration};

use super::client::{FeedRefusal, RefusalReason};
use anyhow::{Context, Result, bail};
use sqlx::{
    Row, SqlitePool,
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous},
};
use uuid::Uuid;

#[derive(Clone)]
pub struct GatewayStore {
    pool: SqlitePool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BeginRequestOutcome {
    New,
    Pending,
    Acknowledged,
    NotDispatched(FeedRefusal),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoredRequestStatus {
    Pending,
    Acknowledged,
    NotDispatched(FeedRefusal),
}

impl GatewayStore {
    pub async fn connect(database_url: &str) -> Result<Self> {
        if !database_url.starts_with("sqlite://") || database_url == "sqlite::memory:" {
            bail!("gateway database must use a file-backed sqlite:// URL");
        }
        let options = SqliteConnectOptions::from_str(database_url)
            .context("invalid integration gateway SQLite URL")?
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            .synchronous(SqliteSynchronous::Full)
            .foreign_keys(true)
            .busy_timeout(Duration::from_secs(5));
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await
            .context("failed opening integration gateway SQLite database")?;
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS feeder_requests (
                request_id TEXT PRIMARY KEY,
                status TEXT NOT NULL CHECK (status IN ('pending', 'acknowledged')),
                created_at INTEGER NOT NULL DEFAULT (unixepoch()),
                updated_at INTEGER NOT NULL DEFAULT (unixepoch())
            )
            "#,
        )
        .execute(&pool)
        .await
        .context("failed creating gateway feeder request table")?;
        // SQLite accepts memory URI aliases. Verify the actual opened database,
        // not just the URL prefix, before admitting any physical request.
        let databases = sqlx::query("PRAGMA database_list").fetch_all(&pool).await?;
        let durable = databases.iter().any(|row| {
            row.get::<String, _>("name") == "main" && !row.get::<String, _>("file").is_empty()
        });
        if !durable {
            bail!("gateway database must be durable and file-backed");
        }
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS feeder_request_events (seq INTEGER PRIMARY KEY AUTOINCREMENT, request_id TEXT NOT NULL REFERENCES feeder_requests(request_id), event TEXT NOT NULL, created_at INTEGER NOT NULL DEFAULT (unixepoch()))",
        ).execute(&pool).await?;
        sqlx::query("CREATE TABLE IF NOT EXISTS feeder_refusals (request_id TEXT PRIMARY KEY, refusal_json TEXT NOT NULL, created_at INTEGER NOT NULL DEFAULT (unixepoch()))")
            .execute(&pool).await?;
        Ok(Self { pool })
    }

    /// Serialize admission across every connection/process sharing this file.
    /// The write lock is released before any OpenHAB I/O. A pending reservation
    /// never expires: only authoritative reconciliation can release it.
    pub async fn begin_request(
        &self,
        request_id: Uuid,
        min_interval: Duration,
        max_per_hour: u64,
        safety_refusal: bool,
    ) -> Result<BeginRequestOutcome> {
        let interval = i64::try_from(min_interval.as_secs())?;
        let cap = i64::try_from(max_per_hour)?;
        if interval < 5 || cap < 1 {
            bail!("invalid gateway safety limits");
        }
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let id = request_id.to_string();
        let existing: Option<String> =
            sqlx::query_scalar("SELECT status FROM feeder_requests WHERE request_id=?")
                .bind(&id)
                .fetch_optional(&mut *tx)
                .await?;
        let refusal: Option<String> =
            sqlx::query_scalar("SELECT refusal_json FROM feeder_refusals WHERE request_id=?")
                .bind(&id)
                .fetch_optional(&mut *tx)
                .await?;
        let outcome = match existing.as_deref() {
            Some("pending") => BeginRequestOutcome::Pending,
            Some("acknowledged") => BeginRequestOutcome::Acknowledged,
            Some(_) => bail!("invalid persisted gateway status"),
            None if refusal.is_some() => BeginRequestOutcome::NotDispatched(serde_json::from_str(
                refusal.as_deref().unwrap(),
            )?),
            None => {
                let pending: i64 = sqlx::query_scalar(
                    "SELECT EXISTS(SELECT 1 FROM feeder_requests WHERE status='pending')",
                )
                .fetch_one(&mut *tx)
                .await?;
                let delay: i64 = sqlx::query_scalar(
                    "SELECT MAX(0, COALESCE((SELECT MAX(updated_at)+?-unixepoch() FROM feeder_requests),0), CASE WHEN (SELECT COUNT(*) FROM feeder_requests WHERE updated_at >= unixepoch()-3600) >= ? THEN COALESCE((SELECT MIN(updated_at)+3601-unixepoch() FROM feeder_requests WHERE updated_at >= unixepoch()-3600),0) ELSE 0 END)",
                ).bind(interval).bind(cap).fetch_one(&mut *tx).await?;
                let refusal = if safety_refusal {
                    Some(FeedRefusal {
                        reason: RefusalReason::Safety,
                        retry_after_seconds: 5,
                    })
                } else if pending != 0 {
                    Some(FeedRefusal {
                        reason: RefusalReason::Unresolved,
                        retry_after_seconds: 5,
                    })
                } else if delay > 0 {
                    Some(FeedRefusal {
                        reason: RefusalReason::Capacity,
                        retry_after_seconds: u64::try_from(delay)?.min(86_400),
                    })
                } else {
                    None
                };
                if let Some(refusal) = refusal {
                    // Terminal tombstone: no later replay of this UUID can dispatch,
                    // even if concurrent callers observed different safety states.
                    sqlx::query(
                        "INSERT INTO feeder_refusals(request_id,refusal_json) VALUES (?,?)",
                    )
                    .bind(&id)
                    .bind(serde_json::to_string(&refusal)?)
                    .execute(&mut *tx)
                    .await?;
                    BeginRequestOutcome::NotDispatched(refusal)
                } else {
                    sqlx::query(
                        "INSERT INTO feeder_requests(request_id,status) VALUES (?, 'pending')",
                    )
                    .bind(&id)
                    .execute(&mut *tx)
                    .await?;
                    sqlx::query("INSERT INTO feeder_request_events(request_id,event) VALUES (?, 'reserved')")
                        .bind(&id).execute(&mut *tx).await?;
                    BeginRequestOutcome::New
                }
            }
        };
        tx.commit()
            .await
            .context("failed committing gateway admission")?;
        Ok(outcome)
    }

    pub async fn mark_acknowledged(&self, request_id: Uuid) -> Result<()> {
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let result = sqlx::query(
            "UPDATE feeder_requests SET status='acknowledged', updated_at=unixepoch() WHERE request_id=? AND status='pending'",
        )
        .bind(request_id.to_string())
        .execute(&mut *tx)
        .await
        .context("failed marking gateway feeder request acknowledged")?;
        if result.rows_affected() == 1 {
            sqlx::query(
                "INSERT INTO feeder_request_events(request_id,event) VALUES (?, 'acknowledged')",
            )
            .bind(request_id.to_string())
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        if result.rows_affected() == 1 {
            return Ok(());
        }
        if self.status(request_id).await? == Some(StoredRequestStatus::Acknowledged) {
            return Ok(());
        }
        bail!("gateway feeder request could not be marked acknowledged")
    }

    pub async fn status(&self, request_id: Uuid) -> Result<Option<StoredRequestStatus>> {
        match self.status_str(&request_id.to_string()).await? {
            None => {
                let refusal: Option<String> = sqlx::query_scalar(
                    "SELECT refusal_json FROM feeder_refusals WHERE request_id=?",
                )
                .bind(request_id.to_string())
                .fetch_optional(&self.pool)
                .await?;
                refusal
                    .map(|v| {
                        serde_json::from_str(&v)
                            .map(StoredRequestStatus::NotDispatched)
                            .map_err(Into::into)
                    })
                    .transpose()
            }
            Some("pending") => Ok(Some(StoredRequestStatus::Pending)),
            Some("acknowledged") => Ok(Some(StoredRequestStatus::Acknowledged)),
            Some(other) => bail!("gateway feeder request has invalid persisted status {other}"),
        }
    }

    async fn status_str<'a>(&self, request_id: &'a str) -> Result<Option<&'a str>> {
        let row = sqlx::query("SELECT status FROM feeder_requests WHERE request_id=?")
            .bind(request_id)
            .fetch_optional(&self.pool)
            .await
            .context("failed reading gateway feeder request status")?;
        let Some(row) = row else {
            return Ok(None);
        };
        let status: String = row.try_get("status")?;
        match status.as_str() {
            "pending" => Ok(Some("pending")),
            "acknowledged" => Ok(Some("acknowledged")),
            _ => bail!("gateway feeder request has invalid persisted status {status}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;

    async fn store() -> (TempDir, GatewayStore) {
        let directory = TempDir::new().unwrap();
        let url = format!("sqlite://{}", directory.path().join("gateway.db").display());
        (directory, GatewayStore::connect(&url).await.unwrap())
    }

    #[tokio::test]
    async fn duplicate_uuid_never_becomes_new_twice() {
        let (_directory, store) = store().await;
        let id = Uuid::new_v4();
        assert_eq!(
            store
                .begin_request(id, Duration::from_secs(5), 60, false)
                .await
                .unwrap(),
            BeginRequestOutcome::New
        );
        assert_eq!(
            store
                .begin_request(id, Duration::from_secs(5), 60, false)
                .await
                .unwrap(),
            BeginRequestOutcome::Pending
        );
        store.mark_acknowledged(id).await.unwrap();
        assert_eq!(
            store
                .begin_request(id, Duration::from_secs(5), 60, false)
                .await
                .unwrap(),
            BeginRequestOutcome::Acknowledged
        );
    }

    #[tokio::test]
    async fn rejects_sqlite_memory_aliases() {
        for url in [
            "sqlite://:memory:",
            "sqlite://test?mode=memory",
            "sqlite://%3Amemory%3A",
        ] {
            assert!(GatewayStore::connect(url).await.is_err(), "{url}");
        }
    }

    #[tokio::test]
    async fn legacy_multiple_pending_rows_are_preserved_and_block_admission() {
        let (_directory, store) = store().await;
        let ids = [Uuid::new_v4(), Uuid::new_v4()];
        for id in ids {
            sqlx::query("INSERT INTO feeder_requests(request_id,status) VALUES (?, 'pending')")
                .bind(id.to_string())
                .execute(&store.pool)
                .await
                .unwrap();
        }
        for id in ids {
            assert_eq!(
                store
                    .begin_request(id, Duration::from_secs(5), 60, false)
                    .await
                    .unwrap(),
                BeginRequestOutcome::Pending
            );
        }
        store.mark_acknowledged(ids[0]).await.unwrap();
        assert_eq!(
            store
                .begin_request(Uuid::new_v4(), Duration::from_secs(5), 60, false)
                .await
                .unwrap(),
            BeginRequestOutcome::NotDispatched(FeedRefusal {
                reason: RefusalReason::Unresolved,
                retry_after_seconds: 5
            })
        );
        assert_eq!(
            store.status(ids[1]).await.unwrap(),
            Some(StoredRequestStatus::Pending)
        );
    }
    #[tokio::test]
    async fn same_uuid_safety_race_has_one_immutable_decision() {
        let (directory, a) = store().await;
        let b = GatewayStore::connect(&format!(
            "sqlite://{}",
            directory.path().join("gateway.db").display()
        ))
        .await
        .unwrap();
        let id = Uuid::new_v4();
        let (one, two) = tokio::join!(
            a.begin_request(id, Duration::from_secs(5), 60, true),
            b.begin_request(id, Duration::from_secs(5), 60, false)
        );
        let results = [one.unwrap(), two.unwrap()];
        if results
            .iter()
            .any(|r| matches!(r, BeginRequestOutcome::New))
        {
            assert!(
                results
                    .iter()
                    .all(|r| !matches!(r, BeginRequestOutcome::NotDispatched(_)))
            );
        } else {
            assert!(
                results
                    .iter()
                    .all(|r| matches!(r, BeginRequestOutcome::NotDispatched(_)))
            );
        }
        let replay = a
            .begin_request(id, Duration::from_secs(5), 60, false)
            .await
            .unwrap();
        assert_ne!(replay, BeginRequestOutcome::New);
    }
}
