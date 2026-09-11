use super::{LedgerStore, StoredStrikeReceiveRequest};
use crate::strike::StrikeCompletedReceiveEvent;
use anyhow::{Context, Result, bail};
use sqlx::Row;
use uuid::Uuid;

pub struct StrikeInboxWork {
    pub key: String,
    pub event: StrikeCompletedReceiveEvent,
    pub attempts: u32,
}

impl LedgerStore {
    pub async fn enqueue_strike_event(&self, event: &StrikeCompletedReceiveEvent) -> Result<()> {
        self.enqueue_strike_work(&format!("webhook:{}", event.event_id), event)
            .await
    }

    pub(crate) async fn enqueue_strike_work(
        &self,
        key: &str,
        event: &StrikeCompletedReceiveEvent,
    ) -> Result<()> {
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let existing =
            sqlx::query("SELECT receive_request_id,receive_id FROM strike_inbox WHERE work_key=?")
                .bind(key)
                .fetch_optional(&mut *tx)
                .await?;
        if let Some(row) = existing {
            if row.get::<String, _>("receive_request_id") != event.receive_request_id.to_string()
                || row.get::<String, _>("receive_id") != event.receive_id.to_string()
            {
                bail!("conflicting Strike inbox identity");
            }
        } else {
            let count: i64 =
                sqlx::query_scalar("SELECT COUNT(*) FROM strike_inbox WHERE status='pending'")
                    .fetch_one(&mut *tx)
                    .await?;
            if count >= 10_000 {
                bail!("Strike inbox capacity exhausted");
            }
            sqlx::query("INSERT INTO strike_inbox(work_key,event_id,receive_request_id,receive_id) VALUES (?,?,?,?)")
                .bind(key).bind(event.event_id.to_string()).bind(event.receive_request_id.to_string()).bind(event.receive_id.to_string()).execute(&mut *tx).await?;
        }
        tx.commit().await?;
        Ok(())
    }

    pub async fn due_strike_work(&self) -> Result<Option<StrikeInboxWork>> {
        let row=sqlx::query("SELECT work_key,event_id,receive_request_id,receive_id,attempts FROM strike_inbox WHERE status IN ('pending','quarantined') AND next_attempt<=unixepoch() ORDER BY next_attempt,created_at,work_key LIMIT 1")
            .fetch_optional(&self.pool).await?;
        row.map(|row| {
            Ok(StrikeInboxWork {
                key: row.try_get("work_key")?,
                event: StrikeCompletedReceiveEvent {
                    event_id: Uuid::parse_str(row.get("event_id"))?,
                    receive_request_id: Uuid::parse_str(row.get("receive_request_id"))?,
                    receive_id: Uuid::parse_str(row.get("receive_id"))?,
                },
                attempts: u32::try_from(row.get::<i64, _>("attempts"))?,
            })
        })
        .transpose()
    }

    pub async fn finish_strike_work(&self, key: &str) -> Result<()> {
        sqlx::query(
            "UPDATE strike_inbox SET status='done',completed_at=unixepoch() WHERE work_key=?",
        )
        .bind(key)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn retry_strike_work(&self, key: &str, attempts: u32) -> Result<()> {
        // Retain poison notifications without permanently consuming admission
        // capacity. Hourly retries also recover after an extended provider outage.
        let quarantined = attempts >= 7;
        sqlx::query("UPDATE strike_inbox SET attempts=MIN(attempts+1,30),status=?,next_attempt=unixepoch()+? WHERE work_key=? AND status IN ('pending','quarantined')")
            .bind(if quarantined { "quarantined" } else { "pending" })
            .bind(if quarantined { 3600 } else { retry_delay(attempts) }).bind(key).execute(&self.pool).await?;
        Ok(())
    }

    pub async fn due_strike_scan(&self) -> Result<Option<(StoredStrikeReceiveRequest, u32, u32)>> {
        let row=sqlx::query("SELECT r.receive_request_id,COALESCE(s.page_offset,0) AS page_offset,COALESCE(s.attempts,0) AS attempts FROM strike_receive_requests r LEFT JOIN strike_recovery_scan s USING(receive_request_id) WHERE COALESCE(s.next_attempt,r.created_at)<=unixepoch() ORDER BY COALESCE(s.next_attempt,r.created_at),r.created_at,r.receive_request_id LIMIT 1")
            .fetch_optional(&self.pool).await?;
        let Some(row) = row else { return Ok(None) };
        let id = Uuid::parse_str(row.get("receive_request_id"))?;
        Ok(Some((
            self.strike_receive_request(id)
                .await?
                .context("issued request disappeared")?,
            u32::try_from(row.get::<i64, _>("page_offset"))?,
            u32::try_from(row.get::<i64, _>("attempts"))?,
        )))
    }

    pub async fn record_strike_scan(
        &self,
        id: Uuid,
        offset: u32,
        attempts: u32,
        delay: i64,
    ) -> Result<()> {
        sqlx::query("INSERT INTO strike_recovery_scan(receive_request_id,page_offset,attempts,next_attempt) VALUES (?,?,?,unixepoch()+?) ON CONFLICT(receive_request_id) DO UPDATE SET page_offset=excluded.page_offset,attempts=excluded.attempts,next_attempt=excluded.next_attempt")
            .bind(id.to_string()).bind(i64::from(offset)).bind(i64::from(attempts)).bind(delay).execute(&self.pool).await?;
        Ok(())
    }
}

pub(crate) fn retry_delay(attempts: u32) -> i64 {
    (2_i64.pow(attempts.min(8))).min(300)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn poison_work_releases_admission_capacity_and_remains_retryable() {
        let directory = tempfile::TempDir::new().unwrap();
        let ledger = LedgerStore::connect(&format!(
            "sqlite://{}",
            directory.path().join("capacity.db").display()
        ))
        .await
        .unwrap();
        let event = StrikeCompletedReceiveEvent {
            event_id: Uuid::new_v4(),
            receive_request_id: Uuid::new_v4(),
            receive_id: Uuid::new_v4(),
        };
        ledger.enqueue_strike_event(&event).await.unwrap();
        let key = format!("webhook:{}", event.event_id);
        sqlx::query("WITH RECURSIVE n(x) AS (SELECT 1 UNION ALL SELECT x+1 FROM n WHERE x<9999) INSERT INTO strike_inbox(work_key,event_id,receive_request_id,receive_id) SELECT 'synthetic:'||x,?,?,? FROM n")
            .bind(event.event_id.to_string()).bind(event.receive_request_id.to_string()).bind(event.receive_id.to_string()).execute(&ledger.pool).await.unwrap();
        let fresh = StrikeCompletedReceiveEvent {
            event_id: Uuid::new_v4(),
            ..event.clone()
        };
        assert!(ledger.enqueue_strike_event(&fresh).await.is_err());
        ledger.retry_strike_work(&key, 7).await.unwrap();
        ledger.enqueue_strike_event(&fresh).await.unwrap();
        sqlx::query("UPDATE strike_inbox SET next_attempt=unixepoch()-1 WHERE work_key=?")
            .bind(&key)
            .execute(&ledger.pool)
            .await
            .unwrap();
        assert_eq!(
            ledger.due_strike_work().await.unwrap().unwrap().event,
            event
        );
        ledger.finish_strike_work(&key).await.unwrap();
        let status: String = sqlx::query_scalar("SELECT status FROM strike_inbox WHERE work_key=?")
            .bind(key)
            .fetch_one(&ledger.pool)
            .await
            .unwrap();
        assert_eq!(status, "done");
    }

    #[tokio::test]
    async fn fresh_issuance_and_notifications_do_not_starve_overdue_recovery() {
        let directory = tempfile::TempDir::new().unwrap();
        let ledger = LedgerStore::connect(&format!(
            "sqlite://{}",
            directory.path().join("fair.db").display()
        ))
        .await
        .unwrap();
        let request = |id: Uuid| StoredStrikeReceiveRequest {
            receive_request_id: id,
            address_user: "goat.name".into(),
            credit_pool: "herd".into(),
            amount_msat: 1_000,
            description_hash: "11".repeat(32),
            payment_hash: format!("{}{}", id.simple(), id.simple()),
            invoice: "synthetic-stored-fixture".into(),
            created_provider: None,
        };
        let old = Uuid::new_v4();
        ledger
            .record_strike_receive_request(&request(old))
            .await
            .unwrap();
        ledger.record_strike_scan(old, 100, 1, -10).await.unwrap();
        let old_event = StrikeCompletedReceiveEvent {
            event_id: Uuid::new_v4(),
            receive_request_id: old,
            receive_id: Uuid::new_v4(),
        };
        ledger.enqueue_strike_event(&old_event).await.unwrap();
        sqlx::query("UPDATE strike_inbox SET next_attempt=unixepoch()-10, attempts=1")
            .execute(&ledger.pool)
            .await
            .unwrap();
        for _ in 0..4 {
            let id = Uuid::new_v4();
            ledger
                .record_strike_receive_request(&request(id))
                .await
                .unwrap();
            ledger
                .enqueue_strike_event(&StrikeCompletedReceiveEvent {
                    event_id: Uuid::new_v4(),
                    receive_request_id: id,
                    receive_id: Uuid::new_v4(),
                })
                .await
                .unwrap();
        }
        let (next, offset, _) = ledger.due_strike_scan().await.unwrap().unwrap();
        assert_eq!(next.receive_request_id, old);
        assert_eq!(offset, 100);
        assert_eq!(
            ledger.due_strike_work().await.unwrap().unwrap().event,
            old_event
        );
    }
}
