use anyhow::{Context, Result, bail};
use serde::Serialize;
use sqlx::Row;
use uuid::Uuid;

use super::{LedgerStore, StoredFeedAttempt, StoredFeedAttemptStatus, to_u64};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DurableEvent {
    pub seq: u64,
    pub event_type: String,
    pub payload_json: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OverlaySnapshotState {
    pub seq: u64,
    pub feed_credit_sats: u64,
    pub unresolved_feed_attempt: Option<StoredFeedAttempt>,
}

impl LedgerStore {
    pub async fn append_event<T: Serialize>(&self, event_type: &str, payload: &T) -> Result<u64> {
        let mut transaction = self.pool.begin().await?;
        let seq = append_event_in_transaction(&mut transaction, event_type, payload).await?;
        transaction.commit().await?;
        Ok(seq)
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

    pub async fn overlay_snapshot_state(&self) -> Result<OverlaySnapshotState> {
        let mut transaction = self.pool.begin().await?;
        let seq_row = sqlx::query("SELECT COALESCE(MAX(seq), 0) AS seq FROM event_log")
            .fetch_one(&mut *transaction)
            .await?;
        let credit_row =
            sqlx::query("SELECT COALESCE(SUM(delta_sats), 0) AS credit FROM ledger_entries")
                .fetch_one(&mut *transaction)
                .await?;
        let attempt_row = sqlx::query(
            r#"
            SELECT id, status, threshold_sats
            FROM feed_attempts
            WHERE status IN ('intent_committed', 'unknown')
            LIMIT 1
            "#,
        )
        .fetch_optional(&mut *transaction)
        .await?;

        let seq: i64 = seq_row.try_get("seq")?;
        let credit: i64 = credit_row.try_get("credit")?;
        if credit < 0 {
            bail!("ledger invariant violated: feed credit is negative ({credit})");
        }

        let unresolved_feed_attempt = attempt_row
            .map(|row| {
                let id: String = row.try_get("id")?;
                let status: String = row.try_get("status")?;
                let threshold_sats: i64 = row.try_get("threshold_sats")?;
                let status = match status.as_str() {
                    "intent_committed" => StoredFeedAttemptStatus::IntentCommitted,
                    "unknown" => StoredFeedAttemptStatus::Unknown,
                    other => bail!("unexpected unresolved feed status {other}"),
                };
                Ok(StoredFeedAttempt {
                    id: Uuid::parse_str(&id).context("invalid feed attempt UUID in database")?,
                    status,
                    threshold_sats: to_u64(threshold_sats, "threshold_sats")?,
                })
            })
            .transpose()?;

        transaction.commit().await?;
        Ok(OverlaySnapshotState {
            seq: to_u64(seq, "event sequence")?,
            feed_credit_sats: to_u64(credit, "feed credit")?,
            unresolved_feed_attempt,
        })
    }
}

pub(super) async fn append_event_in_transaction<T: Serialize>(
    transaction: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    event_type: &str,
    payload: &T,
) -> Result<u64> {
    let payload_json = serde_json::to_string(payload).context("failed serializing durable event")?;
    let result = sqlx::query("INSERT INTO event_log (event_type, payload_json) VALUES (?, ?)")
        .bind(event_type)
        .bind(payload_json)
        .execute(&mut **transaction)
        .await?;
    to_u64(result.last_insert_rowid(), "event sequence")
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

    #[tokio::test]
    async fn snapshot_state_uses_current_durable_sequence_and_credit() {
        let (_directory, store) = store().await;
        store
            .append_event("test", &json!({"value": 1}))
            .await
            .unwrap();

        let snapshot = store.overlay_snapshot_state().await.unwrap();
        assert_eq!(snapshot.seq, 1);
        assert_eq!(snapshot.feed_credit_sats, 0);
        assert!(snapshot.unresolved_feed_attempt.is_none());
    }
}
