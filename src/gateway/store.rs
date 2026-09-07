use anyhow::{Context, Result, bail};
use sqlx::{Row, SqlitePool, sqlite::SqlitePoolOptions};
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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoredRequestStatus {
    Pending,
    Acknowledged,
}

impl StoredRequestStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Acknowledged => "acknowledged",
        }
    }
}

impl GatewayStore {
    pub async fn connect(database_url: &str) -> Result<Self> {
        if !database_url.starts_with("sqlite://") || database_url == "sqlite::memory:" {
            bail!("gateway database must use a file-backed sqlite:// URL");
        }
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect(database_url)
            .await
            .context("failed opening integration gateway SQLite database")?;
        sqlx::query("PRAGMA journal_mode=WAL")
            .execute(&pool)
            .await
            .context("failed enabling gateway SQLite WAL")?;
        sqlx::query("PRAGMA synchronous=FULL")
            .execute(&pool)
            .await
            .context("failed enabling gateway SQLite FULL synchronous mode")?;
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
        Ok(Self { pool })
    }

    pub async fn begin_request(&self, request_id: Uuid) -> Result<BeginRequestOutcome> {
        let request_id = request_id.to_string();
        let inserted = sqlx::query(
            "INSERT OR IGNORE INTO feeder_requests(request_id, status) VALUES (?, 'pending')",
        )
        .bind(&request_id)
        .execute(&self.pool)
        .await
        .context("failed persisting feeder request intent")?;
        if inserted.rows_affected() == 1 {
            return Ok(BeginRequestOutcome::New);
        }
        match self.status_str(&request_id).await? {
            Some("pending") => Ok(BeginRequestOutcome::Pending),
            Some("acknowledged") => Ok(BeginRequestOutcome::Acknowledged),
            Some(other) => bail!("gateway feeder request has invalid persisted status {other}"),
            None => bail!("gateway feeder request disappeared after insert conflict"),
        }
    }

    pub async fn mark_acknowledged(&self, request_id: Uuid) -> Result<()> {
        let result = sqlx::query(
            "UPDATE feeder_requests SET status='acknowledged', updated_at=unixepoch() WHERE request_id=? AND status='pending'",
        )
        .bind(request_id.to_string())
        .execute(&self.pool)
        .await
        .context("failed marking gateway feeder request acknowledged")?;
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
            None => Ok(None),
            Some("pending") => Ok(Some(StoredRequestStatus::Pending)),
            Some("acknowledged") => Ok(Some(StoredRequestStatus::Acknowledged)),
            Some(other) => bail!("gateway feeder request has invalid persisted status {other}"),
        }
    }

    pub async fn last_acknowledged_at(&self) -> Result<Option<i64>> {
        let row = sqlx::query(
            "SELECT MAX(updated_at) AS last_ack FROM feeder_requests WHERE status='acknowledged'",
        )
        .fetch_one(&self.pool)
        .await
        .context("failed reading last gateway feeder acknowledgement")?;
        row.try_get("last_ack")
            .context("failed decoding last gateway feeder acknowledgement")
    }

    pub async fn acknowledged_since(&self, since_unix: i64) -> Result<u64> {
        let row = sqlx::query(
            "SELECT COUNT(*) AS count FROM feeder_requests WHERE status='acknowledged' AND updated_at >= ?",
        )
        .bind(since_unix)
        .fetch_one(&self.pool)
        .await
        .context("failed counting recent gateway feeder acknowledgements")?;
        let count: i64 = row.try_get("count")?;
        u64::try_from(count).context("gateway acknowledged count is negative/out of range")
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
            store.begin_request(id).await.unwrap(),
            BeginRequestOutcome::New
        );
        assert_eq!(
            store.begin_request(id).await.unwrap(),
            BeginRequestOutcome::Pending
        );
        store.mark_acknowledged(id).await.unwrap();
        assert_eq!(
            store.begin_request(id).await.unwrap(),
            BeginRequestOutcome::Acknowledged
        );
        assert!(store.last_acknowledged_at().await.unwrap().is_some());
        assert_eq!(store.acknowledged_since(0).await.unwrap(), 1);
    }
}
