use anyhow::{Context, Result, bail};
use sqlx::Row;

use super::{LedgerStore, to_u64};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutboxEntry {
    pub event_id: String,
    pub signed_event_json: String,
    pub source_event_seq: u64,
    pub attempts: u64,
}

impl LedgerStore {
    pub async fn message_cursor(&self) -> Result<u64> {
        let row = sqlx::query("SELECT last_event_seq FROM message_cursor WHERE singleton = 1")
            .fetch_one(&self.pool)
            .await?;
        let value: i64 = row.try_get("last_event_seq")?;
        to_u64(value, "message cursor")
    }

    pub async fn advance_message_cursor(&self, event_seq: u64) -> Result<()> {
        let event_seq = i64::try_from(event_seq).context("event sequence exceeds SQLite range")?;
        let mut transaction = self.pool.begin().await?;
        let current = message_cursor_in_transaction(&mut transaction).await?;
        if event_seq < current {
            bail!("refusing to move message cursor backward from {current} to {event_seq}");
        }
        if event_seq > current {
            sqlx::query(
                "UPDATE message_cursor SET last_event_seq = ?, updated_at = unixepoch() WHERE singleton = 1",
            )
            .bind(event_seq)
            .execute(&mut *transaction)
            .await?;
        }
        transaction.commit().await?;
        Ok(())
    }

    pub async fn enqueue_signed_message(
        &self,
        source_event_seq: u64,
        event_id: &str,
        signed_event_json: &str,
    ) -> Result<()> {
        let source_event_seq_i64 =
            i64::try_from(source_event_seq).context("event sequence exceeds SQLite range")?;
        let mut transaction = self.pool.begin().await?;
        let current = message_cursor_in_transaction(&mut transaction).await?;
        if source_event_seq_i64 <= current {
            bail!("source event {source_event_seq} is not ahead of message cursor {current}");
        }

        sqlx::query(
            r#"
            INSERT INTO message_outbox
                (event_id, signed_event_json, status, source_event_seq)
            VALUES (?, ?, 'pending', ?)
            "#,
        )
        .bind(event_id)
        .bind(signed_event_json)
        .bind(source_event_seq_i64)
        .execute(&mut *transaction)
        .await?;

        sqlx::query(
            "UPDATE message_cursor SET last_event_seq = ?, updated_at = unixepoch() WHERE singleton = 1",
        )
        .bind(source_event_seq_i64)
        .execute(&mut *transaction)
        .await?;

        transaction.commit().await?;
        Ok(())
    }

    pub async fn next_outbox_entry(&self) -> Result<Option<OutboxEntry>> {
        let row = sqlx::query(
            r#"
            SELECT event_id, signed_event_json, source_event_seq, attempts
            FROM message_outbox
            WHERE status IN ('pending', 'failed')
            ORDER BY source_event_seq ASC, created_at ASC
            LIMIT 1
            "#,
        )
        .fetch_optional(&self.pool)
        .await?;

        row.map(|row| {
            let source_event_seq: i64 = row.try_get("source_event_seq")?;
            let attempts: i64 = row.try_get("attempts")?;
            Ok(OutboxEntry {
                event_id: row.try_get("event_id")?,
                signed_event_json: row.try_get("signed_event_json")?,
                source_event_seq: to_u64(source_event_seq, "outbox source event sequence")?,
                attempts: to_u64(attempts, "outbox attempts")?,
            })
        })
        .transpose()
    }

    pub async fn mark_outbox_published(&self, event_id: &str) -> Result<()> {
        let result = sqlx::query(
            r#"
            UPDATE message_outbox
            SET status = 'published',
                attempts = attempts + 1,
                last_error = NULL,
                published_at = unixepoch()
            WHERE event_id = ? AND status IN ('pending', 'failed')
            "#,
        )
        .bind(event_id)
        .execute(&self.pool)
        .await?;
        if result.rows_affected() != 1 {
            bail!("outbox event {event_id} is not pending publication");
        }
        Ok(())
    }

    pub async fn mark_outbox_failed(&self, event_id: &str, error: &str) -> Result<()> {
        let result = sqlx::query(
            r#"
            UPDATE message_outbox
            SET status = 'failed',
                attempts = attempts + 1,
                last_error = ?
            WHERE event_id = ? AND status IN ('pending', 'failed')
            "#,
        )
        .bind(truncate_error(error))
        .bind(event_id)
        .execute(&self.pool)
        .await?;
        if result.rows_affected() != 1 {
            bail!("outbox event {event_id} is not pending publication");
        }
        Ok(())
    }
}

async fn message_cursor_in_transaction(
    transaction: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
) -> Result<i64> {
    let row = sqlx::query("SELECT last_event_seq FROM message_cursor WHERE singleton = 1")
        .fetch_one(&mut **transaction)
        .await?;
    row.try_get("last_event_seq").map_err(Into::into)
}

fn truncate_error(error: &str) -> String {
    error.chars().take(1_000).collect()
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;

    async fn store() -> (TempDir, LedgerStore) {
        let directory = TempDir::new().unwrap();
        let path = directory.path().join("lightning-goats.db");
        let store = LedgerStore::connect(&format!("sqlite://{}", path.display()))
            .await
            .unwrap();
        (directory, store)
    }

    #[tokio::test]
    async fn enqueue_advances_cursor_atomically() {
        let (_directory, store) = store().await;
        assert_eq!(store.message_cursor().await.unwrap(), 0);

        store
            .enqueue_signed_message(3, "event-a", "{\"id\":\"event-a\"}")
            .await
            .unwrap();
        assert_eq!(store.message_cursor().await.unwrap(), 3);

        let entry = store.next_outbox_entry().await.unwrap().unwrap();
        assert_eq!(entry.event_id, "event-a");
        assert_eq!(entry.source_event_seq, 3);
    }

    #[tokio::test]
    async fn failed_publish_retries_same_stored_event() {
        let (_directory, store) = store().await;
        let signed = "{\"id\":\"event-a\",\"sig\":\"same\"}";
        store
            .enqueue_signed_message(1, "event-a", signed)
            .await
            .unwrap();
        store
            .mark_outbox_failed("event-a", "relay down")
            .await
            .unwrap();

        let entry = store.next_outbox_entry().await.unwrap().unwrap();
        assert_eq!(entry.event_id, "event-a");
        assert_eq!(entry.signed_event_json, signed);
        assert_eq!(entry.attempts, 1);

        store.mark_outbox_published("event-a").await.unwrap();
        assert!(store.next_outbox_entry().await.unwrap().is_none());
    }

    #[tokio::test]
    async fn shadow_cursor_can_advance_without_outbox() {
        let (_directory, store) = store().await;
        store.advance_message_cursor(7).await.unwrap();
        assert_eq!(store.message_cursor().await.unwrap(), 7);
        assert!(store.next_outbox_entry().await.unwrap().is_none());
        assert!(store.advance_message_cursor(6).await.is_err());
    }
}
