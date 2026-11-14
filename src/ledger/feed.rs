use anyhow::{Context, Result, bail};
use serde_json::json;
use sqlx::Row;
use uuid::Uuid;

use super::{LedgerStore, events::append_event_in_transaction, to_i64, to_u64};

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
        let mut transaction = self.pool.begin().await?;
        let result = sqlx::query(
            r#"
            UPDATE feed_attempts
            SET status = 'unknown',
                error = 'daemon restarted with an unresolved feed intent'
            WHERE status = 'intent_committed'
            "#,
        )
        .execute(&mut *transaction)
        .await?;
        let affected = result.rows_affected();
        if affected > 0 {
            append_event_in_transaction(
                &mut transaction,
                "processing_error",
                &json!({
                    "message": "An automatic feed was interrupted and requires operator reconciliation."
                }),
            )
            .await?;
        }
        transaction.commit().await?;
        Ok(affected)
    }

    pub async fn begin_feed_attempt(&self, threshold_sats: u64) -> Result<Option<Uuid>> {
        if threshold_sats == 0 {
            bail!("feeder threshold must be greater than zero");
        }
        let threshold_i64 = to_i64(threshold_sats, "threshold_sats")?;
        // This read/check/write sequence must acquire its writer position before
        // observing credit or pending work. A deferred snapshot cannot safely
        // upgrade after a concurrent funding/settlement transaction writes.
        let mut transaction = self.pool.begin_with("BEGIN IMMEDIATE").await?;

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

        let cooling: i64 = sqlx::query_scalar(
            "SELECT resume_after > unixepoch() FROM feeder_cooldown WHERE singleton=1",
        )
        .fetch_one(&mut *transaction)
        .await?;
        if cooling != 0 {
            transaction.commit().await?;
            return Ok(None);
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
        let mut transaction = self.pool.begin().await?;
        let result = sqlx::query(
            r#"
            UPDATE feed_attempts
            SET status = 'unknown', error = ?
            WHERE id = ? AND status = 'intent_committed'
            "#,
        )
        .bind(truncate_error(error))
        .bind(id.to_string())
        .execute(&mut *transaction)
        .await?;

        if result.rows_affected() != 1 {
            bail!("feed attempt {id} is not in intent_committed state");
        }

        append_event_in_transaction(
            &mut transaction,
            "processing_error",
            &json!({
                "feed_attempt_id": id,
                "message": "An automatic feed could not be confirmed. Operator reconciliation is required."
            }),
        )
        .await?;
        transaction.commit().await?;
        Ok(())
    }

    pub async fn reconcile_unknown_as_fed(&self, id: Uuid) -> Result<()> {
        self.resolve_as_fed(id, "unknown").await
    }

    pub async fn reconcile_unknown_as_not_fed(&self, id: Uuid) -> Result<()> {
        let mut transaction = self.pool.begin().await?;
        let result = sqlx::query(
            r#"
            UPDATE feed_attempts
            SET status = 'reconciled_not_fed', resolved_at = unixepoch(), error = NULL
            WHERE id = ? AND status = 'unknown'
            "#,
        )
        .bind(id.to_string())
        .execute(&mut *transaction)
        .await?;

        if result.rows_affected() != 1 {
            bail!("feed attempt {id} is not in unknown state");
        }

        let feed_credit_sats = feed_credit_in_transaction(&mut transaction).await?;
        append_event_in_transaction(
            &mut transaction,
            "feed_reconciled_not_fed",
            &json!({
                "feed_attempt_id": id,
                "feed_credit_sats": feed_credit_sats
            }),
        )
        .await?;
        transaction.commit().await?;
        Ok(())
    }

    /// Only call with a correlated terminal gateway no-dispatch result.
    /// Preserve the attempt, credit and a restart-safe cooldown in one commit.
    pub async fn resolve_feed_not_dispatched(
        &self,
        id: Uuid,
        retry_after_seconds: u64,
    ) -> Result<()> {
        if !(1..=86_400).contains(&retry_after_seconds) {
            bail!("invalid feeder cooldown");
        }
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let changed = sqlx::query("UPDATE feed_attempts SET status='reconciled_not_fed', resolved_at=unixepoch(), error=NULL WHERE id=? AND status IN ('intent_committed','unknown')")
            .bind(id.to_string()).execute(&mut *tx).await?;
        if changed.rows_affected() != 1 {
            bail!("feed attempt changed before no-dispatch resolution");
        }
        sqlx::query("UPDATE feeder_cooldown SET resume_after=MAX(resume_after,unixepoch()+?) WHERE singleton=1")
            .bind(i64::try_from(retry_after_seconds)?).execute(&mut *tx).await?;
        append_event_in_transaction(
            &mut tx,
            "feeder_not_dispatched",
            &json!({"feed_attempt_id":id,"retry_after_seconds":retry_after_seconds}),
        )
        .await?;
        tx.commit().await?;
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
        let mut transaction = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let funding_enabled = super::reward_funding::prepare(&mut transaction).await?;
        let row =
            sqlx::query("SELECT threshold_sats FROM feed_attempts WHERE id = ? AND status = ?")
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
        let debit_id = sqlx::query(
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
        .await?
        .last_insert_rowid();

        let feed_credit_sats = feed_credit_in_transaction(&mut transaction).await?;
        // Message attribution only: remove the newest still-unspent credit to
        // find the receipt that completed this FIFO threshold. A later payment
        // during dispatch cannot replace the original funding contribution.
        let triggering_credit = triggering_credit(&mut transaction, debit_id, feed_credit_sats)
            .await
            .unwrap_or(None);
        let mut payload = json!({
            "feed_attempt_id": id,
            "threshold_sats": threshold_sats,
            "feed_credit_sats": feed_credit_sats
        });
        if let Some((credit_id, _)) = triggering_credit {
            let first: bool = sqlx::query_scalar("SELECT NOT EXISTS(SELECT 1 FROM feeding_announcement_sources WHERE credit_ledger_id=?)")
                .bind(credit_id).fetch_one(&mut *transaction).await?;
            payload["announce_feeding"] = first.into();
        }
        if let Some((credit_id, amount)) = triggering_credit {
            payload["triggering_credit_id"] = credit_id.into();
            payload["triggering_payment_amount_sats"] = amount.into();
        }
        // Presentation availability can never prevent a financial confirmation.
        if let Ok(Some(owner)) = sqlx::query_scalar::<_, String>(
            "SELECT owner FROM feeding_notice_policy WHERE singleton=1",
        )
        .fetch_optional(&mut *transaction)
        .await
        {
            payload["presentation_owner"] = owner.into();
        }
        let event_seq =
            append_event_in_transaction(&mut transaction, "feeder_confirmed", &payload).await?;

        if let Some((credit_id, _)) = triggering_credit {
            sqlx::query("INSERT INTO feeding_announcement_sources(credit_ledger_id,first_event_seq) VALUES(?,?) ON CONFLICT(credit_ledger_id) DO NOTHING")
                .bind(credit_id).bind(to_i64(event_seq,"first feeding announcement")?).execute(&mut *transaction).await?;
        }
        sqlx::query("INSERT INTO feed_event_sources(feed_attempt_id,event_seq) VALUES(?,?)")
            .bind(id.to_string())
            .bind(to_i64(event_seq, "feed event sequence")?)
            .execute(&mut *transaction)
            .await?;

        if funding_enabled {
            super::reward_funding::record_confirmation(
                &mut transaction,
                id,
                debit_id,
                threshold_sats,
            )
            .await?;
        }

        transaction.commit().await?;
        Ok(())
    }
}

pub(super) async fn triggering_credit(
    transaction: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    debit_id: i64,
    mut unspent_sats: u64,
) -> Result<Option<(i64, u64)>> {
    // Bounded, best-effort presentation lookup, with no schema or accounting
    // mutation. Missing history never substitutes the threshold as a payment.
    let rows = sqlx::query("SELECT id,delta_sats FROM ledger_entries WHERE id<? AND delta_sats>0 ORDER BY id DESC LIMIT 4096")
        .bind(debit_id).fetch_all(&mut **transaction).await?;
    for row in rows {
        let amount = to_u64(row.try_get("delta_sats")?, "credit sats")?;
        if unspent_sats < amount {
            return Ok(Some((row.try_get("id")?, amount)));
        }
        unspent_sats -= amount;
    }
    Ok(None)
}

pub(super) async fn feed_credit_in_transaction(
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
    use crate::{domain::payment::SettledPayment, ledger::SettlementOutcome};

    async fn credited_store(sats: u64) -> (TempDir, LedgerStore) {
        let directory = TempDir::new().unwrap();
        let path = directory.path().join("lightning-goats.db");
        let url = format!("sqlite://{}", path.display());
        let store = LedgerStore::connect(&url).await.unwrap();
        let payment = SettledPayment {
            source: "test".to_owned(),
            source_id: format!("feed-test-{sats}"),
            payment_hash: Some(format!("{sats:064x}")),
            address_user: "herd".to_owned(),
            credit_pool: "herd".to_owned(),
            amount_msat: sats * 1_000,
            settled_at: Some(1_700_000_000),
            context_json: None,
        };
        assert!(matches!(
            store.record_payment(&payment).await.unwrap(),
            SettlementOutcome::Credited { .. }
        ));
        (directory, store)
    }

    #[tokio::test]
    async fn intent_waits_for_an_existing_writer_without_losing_credit_or_reusing_a_feed() {
        use std::time::Duration;

        let (_directory, store) = credited_store(1000).await;
        // Warm all five connections so opening a new connection cannot stand
        // in for waiting at the actual financial transaction boundary.
        let mut connections = Vec::new();
        for _ in 0..5 {
            connections.push(store.pool.acquire().await.unwrap());
        }
        drop(connections);
        let mut writer = store.pool.begin_with("BEGIN IMMEDIATE").await.unwrap();
        // Force a different WAL page while keeping the cooldown expired.
        sqlx::query("UPDATE feeder_cooldown SET resume_after=1 WHERE singleton=1")
            .execute(&mut *writer)
            .await
            .unwrap();

        // Positive control: the old read-then-upgrade pattern really is busy,
        // rather than a missing row, invalid UUID or synthetic mocked failure.
        let mut deferred = store.pool.begin().await.unwrap();
        let _: i64 =
            sqlx::query_scalar("SELECT resume_after FROM feeder_cooldown WHERE singleton=1")
                .fetch_one(&mut *deferred)
                .await
                .unwrap();
        let error = sqlx::query(
            "INSERT INTO feed_attempts(id,status,threshold_sats) VALUES(?,'intent_committed',1000)",
        )
        .bind(Uuid::new_v4().to_string())
        .execute(&mut *deferred)
        .await
        .unwrap_err();
        let sqlx::Error::Database(error) = error else {
            panic!("expected native SQLite writer contention");
        };
        assert!(matches!(error.code().as_deref(), Some("5" | "517")));
        deferred.rollback().await.unwrap();

        let mut pending = Box::pin(store.begin_feed_attempt(1000));
        assert!(
            tokio::time::timeout(Duration::from_millis(100), pending.as_mut())
                .await
                .is_err(),
            "intent should wait for the existing writer, not fail a snapshot upgrade"
        );
        writer.commit().await.unwrap();
        let id = tokio::time::timeout(Duration::from_secs(5), pending)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert_eq!(store.feed_credit_sats().await.unwrap(), 1000);
        assert_eq!(
            store.unresolved_feed_attempt().await.unwrap().unwrap().id,
            id
        );
        assert!(store.begin_feed_attempt(1000).await.is_err());
        store.confirm_feed_attempt(id).await.unwrap();
        assert!(store.confirm_feed_attempt(id).await.is_err());
        assert_eq!(store.feed_credit_sats().await.unwrap(), 0);
        let debits: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM ledger_entries WHERE entry_type='FEED_DEBIT'")
                .fetch_one(&store.pool)
                .await
                .unwrap();
        assert_eq!(debits, 1);
        assert_eq!(
            store
                .events_after(0, 100)
                .await
                .unwrap()
                .iter()
                .filter(|event| event.event_type == "feeder_confirmed")
                .count(),
            1
        );
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
        for event in store
            .events_after(0, 100)
            .await
            .unwrap()
            .into_iter()
            .filter(|e| e.event_type == "feeder_confirmed")
        {
            let payload: serde_json::Value = serde_json::from_str(&event.payload_json).unwrap();
            assert_eq!(payload["triggering_payment_amount_sats"], 2340);
            assert_eq!(payload["threshold_sats"], 1000);
        }

        let events = store.events_after(0, 100).await.unwrap();
        assert_eq!(
            events
                .iter()
                .filter(|event| event.event_type == "feeder_confirmed")
                .count(),
            2
        );
    }

    #[tokio::test]
    async fn triggering_payment_amount_survives_later_payment_confirmation_and_restart() {
        let (directory, store) = credited_store(995).await;
        let paid = |id: &str, sats: u64| SettledPayment {
            source: "test".into(),
            source_id: id.into(),
            payment_hash: None,
            address_user: "herd".into(),
            credit_pool: "herd".into(),
            amount_msat: sats * 1000,
            settled_at: None,
            context_json: None,
        };
        store.record_payment(&paid("trigger", 10)).await.unwrap();
        let id = store.begin_feed_attempt(1000).await.unwrap().unwrap();
        // Received after the original intent; this must not steal attribution.
        store.record_payment(&paid("later", 200)).await.unwrap();
        store.confirm_feed_attempt(id).await.unwrap();
        assert_eq!(store.feed_credit_sats().await.unwrap(), 205);
        let event = store
            .events_after(0, 100)
            .await
            .unwrap()
            .into_iter()
            .find(|e| e.event_type == "feeder_confirmed")
            .unwrap();
        let payload: serde_json::Value = serde_json::from_str(&event.payload_json).unwrap();
        assert_eq!(payload["triggering_payment_amount_sats"], 10);
        assert_eq!(payload["threshold_sats"], 1000);
        let text = crate::presentation::MessageRenderer::embedded()
            .unwrap()
            .render(&event, 1000)
            .unwrap();
        assert!(text.nostr_content.unwrap().contains("10 sats"));
        assert!(text.overlay_message.unwrap().contains("10 sats"));
        let reopened = LedgerStore::connect(&format!(
            "sqlite://{}",
            directory.path().join("lightning-goats.db").display()
        ))
        .await
        .unwrap();
        reopened
            .record_payment(&paid("much-later", 2000))
            .await
            .unwrap();
        let persisted = reopened
            .events_after(event.seq - 1, 1)
            .await
            .unwrap()
            .remove(0);
        assert_eq!(persisted, event);
    }

    #[tokio::test]
    async fn attribution_lookup_bound_never_blocks_confirmation_or_invents_an_amount() {
        let (_directory, store) = credited_store(1000).await;
        sqlx::query("WITH RECURSIVE credits(n) AS (VALUES(1) UNION ALL SELECT n+1 FROM credits WHERE n<4097) INSERT INTO ledger_entries(entry_type,source_key,delta_sats) SELECT 'HERD_RECEIPT','synthetic-later:'||n,1 FROM credits")
            .execute(&store.pool).await.unwrap();
        let id = store.begin_feed_attempt(1000).await.unwrap().unwrap();
        store.confirm_feed_attempt(id).await.unwrap();
        assert_eq!(store.feed_credit_sats().await.unwrap(), 4097);
        let event = store
            .events_after(0, 100)
            .await
            .unwrap()
            .into_iter()
            .find(|e| e.event_type == "feeder_confirmed")
            .unwrap();
        let payload: serde_json::Value = serde_json::from_str(&event.payload_json).unwrap();
        assert!(payload.get("triggering_payment_amount_sats").is_none());
        let text = crate::presentation::MessageRenderer::embedded()
            .unwrap()
            .render(&event, 1000)
            .unwrap();
        assert!(text.nostr_content.unwrap().contains("have been fed"));
    }

    #[tokio::test]
    async fn interrupted_intent_becomes_unknown_without_debit() {
        let (_directory, store) = credited_store(1_340).await;
        let attempt = store.begin_feed_attempt(1_000).await.unwrap().unwrap();

        assert_eq!(
            store.mark_interrupted_feed_intents_unknown().await.unwrap(),
            1
        );
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
        assert!(
            store
                .events_after(0, 100)
                .await
                .unwrap()
                .iter()
                .any(|event| event.event_type == "processing_error")
        );
    }

    #[tokio::test]
    async fn unknown_reconciled_as_fed_debits_exactly_once() {
        let (_directory, store) = credited_store(1_340).await;
        let attempt = store.begin_feed_attempt(1_000).await.unwrap().unwrap();
        store
            .mark_feed_unknown(attempt, "connection reset")
            .await
            .unwrap();
        store.reconcile_unknown_as_fed(attempt).await.unwrap();

        assert_eq!(store.feed_credit_sats().await.unwrap(), 340);
        assert!(store.reconcile_unknown_as_fed(attempt).await.is_err());
        assert_eq!(store.feed_credit_sats().await.unwrap(), 340);
    }

    #[tokio::test]
    async fn unknown_reconciled_not_fed_preserves_credit() {
        let (_directory, store) = credited_store(1_340).await;
        let attempt = store.begin_feed_attempt(1_000).await.unwrap().unwrap();
        store
            .mark_feed_unknown(attempt, "connection reset")
            .await
            .unwrap();
        store.reconcile_unknown_as_not_fed(attempt).await.unwrap();

        assert_eq!(store.feed_credit_sats().await.unwrap(), 1_340);
        assert!(store.begin_feed_attempt(1_000).await.unwrap().is_some());
    }
}
