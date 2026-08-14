use anyhow::{Context, Result};
use serde::Serialize;
use sqlx::Row;

use super::{LedgerStore, to_u64};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DurableEvent {
    pub seq: u64,
    pub event_type: String,
    pub payload_json: String,
}

impl LedgerStore {
    pub async fn append_event<T: Serialize>(&self, event_type: &str, payload: &T) -> Result<u64> {
        let payload_json = serde_json::to_string(payload).context("failed serializing durable event")?;
        let result = sqlx::query(
            "INSERT INTO event_log (event_type, payload_json) VALUES (?, ?)",
        )
        .bind(event_type)
        .bind(payload_json)
        .execute(&self.pool)
        .await?;
        to_u64(result.last_insert_rowid(), "event sequence")
    }

    pub async fn events_after(&self, after_seq: u64, limit: u32) -> Result<Vec<DurableEvent>> {
        let after_seq = i64::try_from(after_seq).context("event sequence exceeds SQLite range")?;
        let limit = i64::from(limit.clamp(1, 1_000));
        let rows = sqlx::query(
            r#"
            SELECT seq, event_type, payload_json
            FROM event_log
            WHERE seq > ?
            ORDER BY seq ASC
            LIMIT ?
            "#,
        )
        .bind(after_seq)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;

        rows.into_iter()
            .map(|row| {
                let seq: i64 = row.try_get("seq")?;
                Ok(DurableEvent {
                    seq: to_u64(seq, "event sequence")?,
                    event_type: row.try_get("event_type")?,
                    payload_json: row.try_get("payload_json")?,
                })
            })
            .collect()
    }

    pub async fn latest_event_seq(&self) -> Result<u64> {
        let row = sqlx::query("SELECT COALESCE(MAX(seq), 0) AS seq FROM event_log")
            .fetch_one(&self.pool)
            .await?;
        let seq: i64 = row.try_get("seq")?;
        to_u64(seq, "event sequence")
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;
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
    async fn events_are_ordered_and_replayable() {
        let (_directory, store) = store().await;
        let first = store
            .append_event("payment_received", &json!({"sats": 100}))
            .await
            .unwrap();
        let second = store
            .append_event("payment_received", &json!({"sats": 200}))
            .await
            .unwrap();

        assert_eq!(first, 1);
        assert_eq!(second, 2);
        assert_eq!(store.latest_event_seq().await.unwrap(), 2);

        let events = store.events_after(1, 100).await.unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].seq, 2);
        assert_eq!(events[0].event_type, "payment_received");
    }
}
