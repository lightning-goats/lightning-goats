use std::{str::FromStr, time::Duration};

use anyhow::{Context, Result, bail};
use serde_json::{Value, json};
use sqlx::{
    Row, SqlitePool,
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous},
};

use crate::domain::payment::SettledPayment;

mod events;
mod feed;
mod outbox;
pub use events::DurableEvent;
use events::append_event_in_transaction;
use feed::feed_credit_in_transaction;
pub use feed::{StoredFeedAttempt, StoredFeedAttemptStatus};
pub use outbox::OutboxEntry;

static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");

#[derive(Debug, Clone)]
pub struct LedgerStore {
    pool: SqlitePool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SettlementOutcome {
    Credited {
        sats: u64,
        address_user: String,
        credit_pool: String,
    },
    Duplicate,
}

impl LedgerStore {
    pub async fn connect(database_url: &str) -> Result<Self> {
        let options = SqliteConnectOptions::from_str(database_url)
            .with_context(|| format!("invalid SQLite URL: {database_url}"))?
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            .synchronous(SqliteSynchronous::Full)
            .foreign_keys(true)
            .busy_timeout(Duration::from_secs(5));

        let pool = SqlitePoolOptions::new()
            .max_connections(5)
            .connect_with(options)
            .await
            .context("failed opening Lightning Goats SQLite database")?;

        MIGRATOR
            .run(&pool)
            .await
            .context("failed applying Lightning Goats database migrations")?;

        Ok(Self { pool })
    }

    /// Record one provider-authoritative settled payment exactly once.
    ///
    /// Provider delivery ordering is intentionally irrelevant. Identity is the
    /// `(source, source_id)` tuple, with `payment_hash` acting as an additional
    /// collision boundary when the provider exposes one.
    pub async fn record_payment(&self, payment: &SettledPayment) -> Result<SettlementOutcome> {
        validate_source(&payment.source)?;
        validate_external_id(&payment.source_id, "payment source_id", 256)?;
        validate_user(&payment.address_user, "address_user")?;
        validate_user(&payment.credit_pool, "credit_pool")?;
        if let Some(hash) = payment.payment_hash.as_deref() {
            validate_payment_hash(hash)?;
        }
        if payment.amount_msat == 0 {
            bail!("payment amount_msat must be greater than zero");
        }
        if payment.amount_msat % 1_000 != 0 {
            bail!(
                "payment amount_msat={} is not sat-aligned; refusing lossy feed-credit conversion",
                payment.amount_msat
            );
        }
        if payment.settled_at.is_some_and(|value| value < 0) {
            bail!("payment settled_at must not be negative");
        }
        if let Some(context_json) = payment.context_json.as_deref() {
            let value: Value = serde_json::from_str(context_json)
                .context("payment context_json is not valid JSON")?;
            if !value.is_object() {
                bail!("payment context_json must be a JSON object");
            }
        }

        let amount_msat = to_i64(payment.amount_msat, "amount_msat")?;
        let credited_sats = payment.amount_msat / 1_000;
        let credited_sats_i64 = to_i64(credited_sats, "credited_sats")?;
        let mut transaction = self.pool.begin().await?;

        let existing = sqlx::query(
            r#"
            SELECT payment_hash, address_user, credit_pool, amount_msat, settled_at
            FROM settled_payments
            WHERE source = ? AND source_id = ?
            "#,
        )
        .bind(&payment.source)
        .bind(&payment.source_id)
        .fetch_optional(&mut *transaction)
        .await?;

        if let Some(row) = existing {
            let existing_hash: Option<String> = row.try_get("payment_hash")?;
            let existing_user: String = row.try_get("address_user")?;
            let existing_pool: String = row.try_get("credit_pool")?;
            let existing_amount: i64 = row.try_get("amount_msat")?;
            let existing_settled_at: Option<i64> = row.try_get("settled_at")?;

            let matches = existing_hash.as_deref() == payment.payment_hash.as_deref()
                && existing_user == payment.address_user
                && existing_pool == payment.credit_pool
                && existing_amount == amount_msat
                && existing_settled_at == payment.settled_at;

            if matches {
                transaction.commit().await?;
                return Ok(SettlementOutcome::Duplicate);
            }

            bail!(
                "payment source identity {}:{} already exists with conflicting settlement data",
                payment.source,
                payment.source_id
            );
        }

        if let Some(hash) = payment.payment_hash.as_deref() {
            let collision = sqlx::query(
                "SELECT source, source_id FROM settled_payments WHERE payment_hash = ?",
            )
            .bind(hash)
            .fetch_optional(&mut *transaction)
            .await?;
            if let Some(row) = collision {
                let existing_source: String = row.try_get("source")?;
                let existing_source_id: String = row.try_get("source_id")?;
                bail!(
                    "payment_hash {hash} already belongs to {existing_source}:{existing_source_id}; refusing conflicting settlement {}:{}",
                    payment.source,
                    payment.source_id
                );
            }
        }

        sqlx::query(
            r#"
            INSERT INTO settled_payments
                (source, source_id, payment_hash, address_user, credit_pool,
                 amount_msat, settled_at, context_json)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&payment.source)
        .bind(&payment.source_id)
        .bind(payment.payment_hash.as_deref())
        .bind(&payment.address_user)
        .bind(&payment.credit_pool)
        .bind(amount_msat)
        .bind(payment.settled_at)
        .bind(payment.context_json.as_deref())
        .execute(&mut *transaction)
        .await?;

        let source_key = format!("payment:{}:{}", payment.source, payment.source_id);
        sqlx::query(
            r#"
            INSERT INTO ledger_entries
                (entry_type, source_key, delta_sats, payment_hash,
                 payment_source, payment_source_id, address_user, credit_pool)
            VALUES ('HERD_RECEIPT', ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(source_key)
        .bind(credited_sats_i64)
        .bind(payment.payment_hash.as_deref())
        .bind(&payment.source)
        .bind(&payment.source_id)
        .bind(&payment.address_user)
        .bind(&payment.credit_pool)
        .execute(&mut *transaction)
        .await?;

        let feed_credit_sats = feed_credit_in_transaction(&mut transaction).await?;
        append_event_in_transaction(
            &mut transaction,
            "payment_received",
            &json!({
                "source": payment.source,
                "source_id": payment.source_id,
                "payment_hash": payment.payment_hash,
                "address_user": payment.address_user,
                "credit_pool": payment.credit_pool,
                "amount_sats": credited_sats,
                "feed_credit_sats": feed_credit_sats
            }),
        )
        .await?;

        transaction.commit().await?;

        Ok(SettlementOutcome::Credited {
            sats: credited_sats,
            address_user: payment.address_user.clone(),
            credit_pool: payment.credit_pool.clone(),
        })
    }

    pub async fn feed_credit_sats(&self) -> Result<u64> {
        let row = sqlx::query("SELECT COALESCE(SUM(delta_sats), 0) AS credit FROM ledger_entries")
            .fetch_one(&self.pool)
            .await?;
        let credit: i64 = row.try_get("credit")?;
        if credit < 0 {
            bail!("ledger invariant violated: feed credit is negative ({credit})");
        }
        to_u64(credit, "feed credit")
    }

    // Transitional CLN compatibility cursor. This is deliberately separate
    // from payment identity/accounting and is removed with the CLN watcher in
    // Phase 1 issue #13.
    pub async fn initialize_legacy_cln_cursor(&self, last_pay_index: u64) -> Result<()> {
        let last_pay_index = to_i64(last_pay_index, "last_pay_index")?;
        let mut transaction = self.pool.begin().await?;

        let existing = sqlx::query("SELECT last_pay_index FROM cln_cursor WHERE singleton = 1")
            .fetch_optional(&mut *transaction)
            .await?;

        if let Some(row) = existing {
            let current: i64 = row.try_get("last_pay_index")?;
            if current != last_pay_index {
                bail!(
                    "legacy CLN cursor is already initialized at {current}; refusing to replace it with {last_pay_index}"
                );
            }
            transaction.commit().await?;
            return Ok(());
        }

        sqlx::query("INSERT INTO cln_cursor (singleton, last_pay_index) VALUES (1, ?)")
            .bind(last_pay_index)
            .execute(&mut *transaction)
            .await?;
        transaction.commit().await?;
        Ok(())
    }

    pub async fn last_legacy_cln_pay_index(&self) -> Result<Option<u64>> {
        let row = sqlx::query("SELECT last_pay_index FROM cln_cursor WHERE singleton = 1")
            .fetch_optional(&self.pool)
            .await?;
        row.map(|row| {
            let value: i64 = row.try_get("last_pay_index")?;
            to_u64(value, "last_pay_index")
        })
        .transpose()
    }

    pub async fn advance_legacy_cln_cursor(&self, pay_index: u64) -> Result<()> {
        let pay_index = to_i64(pay_index, "pay_index")?;
        let mut transaction = self.pool.begin().await?;
        let row = sqlx::query("SELECT last_pay_index FROM cln_cursor WHERE singleton = 1")
            .fetch_optional(&mut *transaction)
            .await?
            .context("legacy CLN cursor is not initialized")?;
        let current: i64 = row.try_get("last_pay_index")?;
        if pay_index < current {
            bail!("legacy CLN cursor cannot move backward from {current} to {pay_index}");
        }
        if pay_index > current {
            sqlx::query(
                "UPDATE cln_cursor SET last_pay_index = ?, updated_at = unixepoch() WHERE singleton = 1",
            )
            .bind(pay_index)
            .execute(&mut *transaction)
            .await?;
        }
        transaction.commit().await?;
        Ok(())
    }
}

fn validate_source(source: &str) -> Result<()> {
    if source.is_empty() || source.len() > 32 {
        bail!("payment source must contain 1 to 32 characters");
    }
    if !source.bytes().all(|byte| {
        byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'_' | b'-' | b'.')
    }) {
        bail!("payment source must be a lowercase canonical identifier");
    }
    Ok(())
}

fn validate_user(value: &str, field: &str) -> Result<()> {
    if value.is_empty() || value.len() > 64 {
        bail!("{field} must contain 1 to 64 characters");
    }
    if !value.bytes().all(|byte| {
        byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'.' | b'_' | b'-')
    }) {
        bail!("{field} must be a canonical lowercase Lightning Address-style identifier");
    }
    Ok(())
}

fn validate_external_id(value: &str, field: &str, max_len: usize) -> Result<()> {
    if value.is_empty() || value.len() > max_len {
        bail!("{field} must contain 1 to {max_len} characters");
    }
    if !value.bytes().all(|byte| byte.is_ascii_graphic()) {
        bail!("{field} contains whitespace or non-ASCII/control characters");
    }
    Ok(())
}

fn validate_payment_hash(hash: &str) -> Result<()> {
    if hash.is_empty() || hash.len() > 128 || !hash.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        bail!("payment_hash must be non-empty hexadecimal text no longer than 128 characters");
    }
    Ok(())
}

fn to_i64(value: u64, field: &str) -> Result<i64> {
    i64::try_from(value).with_context(|| format!("{field} exceeds SQLite INTEGER range"))
}

fn to_u64(value: i64, field: &str) -> Result<u64> {
    u64::try_from(value).with_context(|| format!("{field} is unexpectedly negative"))
}

#[cfg(test)]
mod tests {
    use serde_json::Value;
    use tempfile::TempDir;

    use super::*;

    async fn store() -> (TempDir, LedgerStore) {
        let directory = TempDir::new().unwrap();
        let path = directory.path().join("lightning-goats.db");
        let url = format!("sqlite://{}", path.display());
        let store = LedgerStore::connect(&url).await.unwrap();
        (directory, store)
    }

    fn payment(source_id: &str, hash: Option<&str>, user: &str, sats: u64) -> SettledPayment {
        SettledPayment {
            source: "strike".to_owned(),
            source_id: source_id.to_owned(),
            payment_hash: hash.map(str::to_owned),
            address_user: user.to_owned(),
            credit_pool: "herd".to_owned(),
            amount_msat: sats * 1_000,
            settled_at: Some(1_700_000_000),
            context_json: None,
        }
    }

    #[tokio::test]
    async fn provider_neutral_settlement_credits_and_emits_metadata_atomically() {
        let (_directory, store) = store().await;
        let paid = payment("receive-101", Some("aabbcc"), "dexter", 2_340);

        let outcome = store.record_payment(&paid).await.unwrap();
        assert_eq!(
            outcome,
            SettlementOutcome::Credited {
                sats: 2_340,
                address_user: "dexter".to_owned(),
                credit_pool: "herd".to_owned(),
            }
        );
        assert_eq!(store.feed_credit_sats().await.unwrap(), 2_340);
        let events = store.events_after(0, 10).await.unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_type, "payment_received");
        let payload: Value = serde_json::from_str(&events[0].payload_json).unwrap();
        assert_eq!(payload["source"], "strike");
        assert_eq!(payload["address_user"], "dexter");
        assert_eq!(payload["credit_pool"], "herd");
        assert_eq!(payload["feed_credit_sats"], 2_340);
    }

    #[tokio::test]
    async fn exact_duplicate_is_idempotent() {
        let (_directory, store) = store().await;
        let paid = payment("receive-102", Some("ddeeff"), "herd", 1_000);

        store.record_payment(&paid).await.unwrap();
        assert_eq!(
            store.record_payment(&paid).await.unwrap(),
            SettlementOutcome::Duplicate
        );
        assert_eq!(store.feed_credit_sats().await.unwrap(), 1_000);
        assert_eq!(store.events_after(0, 10).await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn conflicting_source_identity_fails_closed() {
        let (_directory, store) = store().await;
        let original = payment("receive-103", Some("0011aa"), "rowan", 1_000);
        store.record_payment(&original).await.unwrap();
        let mut conflicting = original.clone();
        conflicting.amount_msat = 2_000_000;

        assert!(store.record_payment(&conflicting).await.is_err());
        assert_eq!(store.feed_credit_sats().await.unwrap(), 1_000);
    }

    #[tokio::test]
    async fn payment_hash_collision_across_provider_identity_fails_closed() {
        let (_directory, store) = store().await;
        store
            .record_payment(&payment("receive-104", Some("cafeba"), "cosmo", 500))
            .await
            .unwrap();
        let collision = payment("receive-105", Some("cafeba"), "nova", 500);

        assert!(store.record_payment(&collision).await.is_err());
        assert_eq!(store.feed_credit_sats().await.unwrap(), 500);
    }

    #[tokio::test]
    async fn rejects_non_sat_aligned_settlement_without_truncation() {
        let (_directory, store) = store().await;
        let mut paid = payment("receive-106", Some("abcdef"), "newton", 1);
        paid.amount_msat = 1_001;

        assert!(store.record_payment(&paid).await.is_err());
        assert_eq!(store.feed_credit_sats().await.unwrap(), 0);
    }

    #[tokio::test]
    async fn legacy_cln_cursor_is_optional_and_separate_from_payment_identity() {
        let (_directory, store) = store().await;
        assert_eq!(store.last_legacy_cln_pay_index().await.unwrap(), None);

        store.initialize_legacy_cln_cursor(100).await.unwrap();
        store.advance_legacy_cln_cursor(101).await.unwrap();
        assert_eq!(store.last_legacy_cln_pay_index().await.unwrap(), Some(101));
        assert!(store.advance_legacy_cln_cursor(99).await.is_err());
    }
}
