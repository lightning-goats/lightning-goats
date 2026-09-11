use super::{DurableEvent, LedgerStore, to_u64};
use anyhow::Result;
use sqlx::Row;
use uuid::Uuid;

impl LedgerStore {
    /// Offline restore must invalidate cursors from the abandoned ledger future.
    pub async fn reset_overlay_stream(&self) -> Result<Uuid> {
        let id = Uuid::new_v4();
        sqlx::query("INSERT INTO overlay_identity(singleton,stream_id) VALUES (1,?) ON CONFLICT(singleton) DO UPDATE SET stream_id=excluded.stream_id")
            .bind(id.to_string()).execute(&self.pool).await?;
        Ok(id)
    }

    pub async fn overlay_stream_id(&self) -> Result<Uuid> {
        let existing: Option<String> =
            sqlx::query_scalar("SELECT stream_id FROM overlay_identity WHERE singleton=1")
                .fetch_optional(&self.pool)
                .await?;
        if let Some(id) = existing {
            return Ok(Uuid::parse_str(&id)?);
        }
        sqlx::query("INSERT OR IGNORE INTO overlay_identity(singleton,stream_id) VALUES (1,?)")
            .bind(Uuid::new_v4().to_string())
            .execute(&self.pool)
            .await?;
        let id: String =
            sqlx::query_scalar("SELECT stream_id FROM overlay_identity WHERE singleton=1")
                .fetch_one(&self.pool)
                .await?;
        Ok(Uuid::parse_str(&id)?)
    }

    /// Overlay-specific replay limits never constrain durable publication.
    pub async fn overlay_event_window(
        &self,
        after: u64,
        through: u64,
    ) -> Result<Option<Vec<DurableEvent>>> {
        if through < after || through - after > 1000 || through > i64::MAX as u64 {
            return Ok(None);
        }
        let mut tx = self.pool.begin().await?;
        let row = sqlx::query("SELECT COUNT(*) AS count,COALESCE(SUM(LENGTH(CAST(payload_json AS BLOB))+LENGTH(CAST(event_type AS BLOB))),0) AS bytes FROM event_log WHERE seq>? AND seq<=?").bind(after as i64).bind(through as i64).fetch_one(&mut *tx).await?;
        if row.get::<i64, _>("count") != (through - after) as i64
            || row.get::<i64, _>("bytes") > 512 * 1024
        {
            return Ok(None);
        }
        let rows = sqlx::query(
            "SELECT seq,event_type,payload_json FROM event_log WHERE seq>? AND seq<=? ORDER BY seq",
        )
        .bind(after as i64)
        .bind(through as i64)
        .fetch_all(&mut *tx)
        .await?;
        let events = rows
            .into_iter()
            .map(|row| {
                Ok(DurableEvent {
                    seq: to_u64(row.try_get("seq")?, "overlay sequence")?,
                    event_type: row.try_get("event_type")?,
                    payload_json: row.try_get("payload_json")?,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        tx.commit().await?;
        Ok(Some(events))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn stream_identity_survives_reopen_and_missing_history_resets() {
        let dir = tempfile::TempDir::new().unwrap();
        let db = format!("sqlite://{}", dir.path().join("ledger.db").display());
        let first = LedgerStore::connect(&db).await.unwrap();
        let id = first.overlay_stream_id().await.unwrap();
        for _ in 0..3 {
            first
                .append_event("interface_info", &json!({}))
                .await
                .unwrap();
        }
        let second = LedgerStore::connect(&db).await.unwrap();
        assert_eq!(second.overlay_stream_id().await.unwrap(), id);
        assert_eq!(
            second
                .overlay_event_window(0, 3)
                .await
                .unwrap()
                .unwrap()
                .len(),
            3
        );
        sqlx::query("DELETE FROM event_log WHERE seq=2")
            .execute(&second.pool)
            .await
            .unwrap();
        assert!(first.overlay_event_window(0, 3).await.unwrap().is_none());
        assert!(first.overlay_event_window(0, 1001).await.unwrap().is_none());
        assert!(first.overlay_event_window(3, 2).await.unwrap().is_none());
        assert!(
            first
                .overlay_event_window(3, 3)
                .await
                .unwrap()
                .unwrap()
                .is_empty()
        );
        assert_eq!(second.events_after(0, 10).await.unwrap().len(), 2);
    }
}
