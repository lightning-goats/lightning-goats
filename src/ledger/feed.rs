use anyhow::{Context, Result, bail};
use sqlx::Row;
use uuid::Uuid;

use super::{LedgerStore, to_i64, to_u64};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoredFeedAttemptStatus {
    IntentCommitted,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StoredFeedAttempt {
    pub id: Uuid,
    pub status: StoredFeedAttemptStatus,
    pub threshold_sats: u64,
}

impl LedgerStore {
    pub async fn mark_interrupted_feed_intents_unknown(&self) -> Result<u64> {
        let result = sqlx::query(
            r#"
            UPDATE feed_attempts
            SET status = 'unknown',
                error = 'daemon restarted with an unresolved feed intent'
            WHERE status = 'intent_committed'
            "#,
        )
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected())
    }

    pub async fn begin_feed_attempt(&self, threshold_sats: u64) -> Result<Option<Uuid>> {
        if threshold_sats == 0 {
            bail!("feeder threshold must be greater than zero");
        }
        let threshold_i64 = to_i64(threshold_sats, "threshold_sats")?;
        let mut transaction = self.pool.begin().await?;

        if let Some(row) = sqlx::query(
            "SELECT id, status FROM feed_attempts WHERE status IN ('intent_committed', 'unknown') LIMIT 1",
        )
        .fetch_optional(&mut *transaction)
        .await?
        {
            let id: String = row.try_get("id")?;
            let status: String = row.try_get("status")?;
            bail!("unresolved feed attempt {id} is still {status}");
        }

        let credit = feed_credit_in_transaction(&mut transaction).await?;
        if credit < threshold_sats {
            transaction.commit().await?;
            return Ok(None);
        }

        let id = Uuid::new_v4();
        sqlx::query(
            r#"
            INSERT INTO feed_attempts (id, status, threshold_sats)
            VALUES (?, 'intent_committed', ?)
            "#,
        )
        .bind(id.to_string())
        .bind(threshold_i64)
        .execute(&mut *transaction)
        .await?;

        transaction.commit().await?;
        Ok(Some(id))
    }

    pub async fn confirm_feed_attempt(&self, id: Uuid) -> Result<()> {
        self.resolve_as_fed(id, "intent_committed").await
    }

    pub async fn mark_feed_unknown(&self, id: Uuid, error: &str) -> Result<()> {
        let result = sqlx::query(
            r#"
            UPDATE feed_attempts
            SET status = 'unknown', error = ?
            WHERE id = ? AND status = 'intent_committed'
            "#,
        )
        .bind(truncate_error(error))
        .bind(id.to_string())
        .execute(&self.pool)
        .await?;

        if result.rows_affected() != 1 {
            bail!("feed attempt {id} is not in intent_committed state");
        }
        Ok(())
    }

    pub async fn reconcile_unknown_as_fed(&self, id: Uuid) -> Result<()> {
        self.resolve_as_fed(id, "unknown").await
    }

    pub async fn reconcile_unknown_as_not_fed(&self, id: Uuid) -> Result<()> {
        let result = sqlx::query(
            r#"
            UPDATE feed_attempts
            SET status = 'reconciled_not_fed', resolved_at = unixepoch(), error = NULL
            WHERE id = ? AND status = 'unknown'
            "#,
        )
        .bind(id.to_string())
        .execute(&self.pool)
        .await?;

        if result.rows_affected() != 1 {
            bail!("feed attempt {id} is not in unknown state");
        }
        Ok(())
    }

    pub async fn unresolved_feed_attempt(&self) -> Result<Option<StoredFeedAttempt>> {
        let row = sqlx::query(
            r#"
            SELECT id, status, threshold_sats
            FROM feed_attempts
            WHERE status IN ('intent_committed', 'unknown')
            LIMIT 1
            "#,
        )
        .fetch_optional(&self.pool)
        .await?;

        row.map(|row| {
            let id: String = row.try_get("id")?;
            let status: String = row.try_get("status")?;
            let threshold: i64 = row.try_get("threshold_sats")?;
            let status = match status.as_str() {
                "intent_committed" => StoredFeedAttemptStatus::IntentCommitted,
                "unknown" => StoredFeedAttemptStatus::Unknown,
                other => bail!("unexpected unresolved feed status {other}"),
            };
            Ok(StoredFeedAttempt {
                id: Uuid::parse_str(&id).context("invalid feed attempt UUID in database")?,
                status,
                threshold_sats: to_u64(threshold, "threshold_sats")?,
            })
        })
        .transpose()
    }

    async fn resolve_as_fed(&self, id: Uuid, expected_status: &str) -> Result<()> {
        let mut transaction = self.pool.begin().await?;
        let row = sqlx::query(
            "SELECT threshold_sats FROM feed_attempts WHERE id = ? AND status = ?",
        )
        .bind(id.to_string())
        .bind(expected_status)
        .fetch_optional(&mut *transaction)
        .await?
        .with_context(|| format!("feed attempt {id} is not in {expected_status} state"))?;

        let threshold_i64: i64 = row.try_get("threshold_sats")?;
        let threshold_sats = to_u64(threshold_i64, "threshold_sats")?;
        let current_credit = feed_credit_in_transaction(&mut transaction).await?;
        if current_credit < threshold_sats {
            bail!(
                "ledger invariant violated: feed attempt {id} requires {threshold_sats} sats but only {current_credit} remain"
            );
        }

        let result = sqlx::query(
            r#"
            UPDATE feed_attempts
            SET status = 'confirmed', resolved_at = unixepoch(), error = NULL
            WHERE id = ? AND status = ?
            "#,
        )
        .bind(id.to_string())
        .bind(expected_status)
        .execute(&mut *transaction)
        .await?;
        if result.rows_affected() != 1 {
            bail!("feed attempt {id} changed state while being resolved");
        }

        let source_key = format!("feed:{id}");
        sqlx::query(
            r#"
            INSERT INTO ledger_entries
                (entry_type, source_key, delta_sats, feed_attempt_id)
            VALUES ('FEED_DEBIT', ?, ?, ?)
            "#,
        )
        .bind(source_key)
        .bind(-threshold_i64)
        .bind(id.to_string())
        .execute(&mut *transaction)
        .await?;

        transaction.commit().await?;
        Ok(())
    }
}

async fn feed_credit_in_transaction(
    transaction: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
) -> Result<u64> {
    let row = sqlx::query("SELECT COALESCE(SUM(delta_sats), 0) AS credit FROM ledger_entries")
        .fetch_one(&mut **transaction)
        .await?;
    let credit: i64 = row.try_get("credit")?;
    if credit < 0 {
        bail!("ledger invariant violated: feed credit is negative ({credit})");
    }
    to_u64(credit, "feed credit")
}

fn truncate_error(error: &str) -> String {
    error.chars().take(1_000).collect()
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;
    use crate::ledger::{PaidInvoice, SettlementOutcome};

    async fn credited_store(sats: u64) -> (TempDir, LedgerStore) {
        let directory = TempDir::new().unwrap();
        let path = directory.path().join("lightning-goats.db");
        let url = format!("sqlite://{}", path.display());
        let store = LedgerStore::connect(&url).await.unwrap();
        store.initialize_cursor(100).await.unwrap();
        let invoice = PaidInvoice {
            pay_index: 101,
            payment_hash: "feed-test-hash".to_owned(),
            label: Some(
                "clnaddress:v1:herd:550e8400-e29b-41d4-a716-446655440000".to_owned(),
            ),
            amount_msat: sats * 1_000,
            settled_at: Some(1_700_000_000),
        };
        assert!(matches!(
            store.record_settlement(&invoice, "herd").await.unwrap(),
            SettlementOutcome::Credited { .. }
        ));
        (directory, store)
    }

    #[tokio::test]
    async fn two_confirmed_feed_attempts_leave_340_sats() {
        let (_directory, store) = credited_store(2_340).await;

        let first = store.begin_feed_attempt(1_000).await.unwrap().unwrap();
        assert_eq!(store.feed_credit_sats().await.unwrap(), 2_340);
        store.confirm_feed_attempt(first).await.unwrap();
        assert_eq!(store.feed_credit_sats().await.unwrap(), 1_340);

        let second = store.begin_feed_attempt(1_000).await.unwrap().unwrap();
        store.confirm_feed_attempt(second).await.unwrap();
        assert_eq!(store.feed_credit_sats().await.unwrap(), 340);
        assert!(store.begin_feed_attempt(1_000).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn interrupted_intent_becomes_unknown_without_debit() {
        let (_directory, store) = credited_store(1_340).await;
        let attempt = store.begin_feed_attempt(1_000).await.unwrap().unwrap();

        assert_eq!(store.mark_interrupted_feed_intents_unknown().await.unwrap(), 1);
        assert_eq!(store.feed_credit_sats().await.unwrap(), 1_340);
        let unresolved = store.unresolved_feed_attempt().await.unwrap().unwrap();
        assert_eq!(unresolved.id, attempt);
        assert_eq!(unresolved.status, StoredFeedAttemptStatus::Unknown);
    }

    #[tokio::test]
    async fn unknown_attempt_halts_new_feeds_without_debit() {
        let (_directory, store) = credited_store(2_340).await;
        let attempt = store.begin_feed_attempt(1_000).await.unwrap().unwrap();
        store.mark_feed_unknown(attempt, "timeout").await.unwrap();

        assert_eq!(store.feed_credit_sats().await.unwrap(), 2_340);
        assert!(store.begin_feed_attempt(1_000).await.is_err());
        let unresolved = store.unresolved_feed_attempt().await.unwrap().unwrap();
        assert_eq!(unresolved.id, attempt);
        assert_eq!(unresolved.status, StoredFeedAttemptStatus::Unknown);
    }

    #[tokio::test]
    async fn unknown_reconciled_as_fed_debits_exactly_once() {
        let (_directory, store) = credited_store(1_340).await;
        let attempt = store.begin_feed_attempt(1_000).await.unwrap().unwrap();
        store.mark_feed_unknown(attempt, "connection reset").await.unwrap();
        store.reconcile_unknown_as_fed(attempt).await.unwrap();

        assert_eq!(store.feed_credit_sats().await.unwrap(), 340);
        assert!(store.reconcile_unknown_as_fed(attempt).await.is_err());
        assert_eq!(store.feed_credit_sats().await.unwrap(), 340);
    }

    #[tokio::test]
    async fn unknown_reconciled_not_fed_preserves_credit() {
        let (_directory, store) = credited_store(1_340).await;
        let attempt = store.begin_feed_attempt(1_000).await.unwrap().unwrap();
        store.mark_feed_unknown(attempt, "connection reset").await.unwrap();
        store.reconcile_unknown_as_not_fed(attempt).await.unwrap();

        assert_eq!(store.feed_credit_sats().await.unwrap(), 1_340);
        assert!(store.begin_feed_attempt(1_000).await.unwrap().is_some());
    }
}
