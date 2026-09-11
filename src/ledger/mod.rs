mod admission;
mod inbox;
pub use inbox::StrikeInboxWork;

use anyhow::{Context, Result, bail};
use serde_json::{Value, json};
use sqlx::{Row, SqlitePool};

use crate::domain::payment::SettledPayment;

mod events;
mod feed;
mod outbox;
mod overlay;
mod strike;
pub use events::DurableEvent;
use events::append_event_in_transaction;
use feed::feed_credit_in_transaction;
pub use feed::{StoredFeedAttempt, StoredFeedAttemptStatus};
pub use outbox::OutboxEntry;
pub use strike::StoredStrikeReceiveRequest;

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
        let pool = crate::sqlite::connect_durable(database_url, 5)
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
}

fn validate_source(source: &str) -> Result<()> {
    if source.is_empty() || source.len() > 32 {
        bail!("payment source must contain 1 to 32 characters");
    }
    if !source.bytes().all(|byte| {
        byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'-' | b'_')
    }) {
        bail!("payment source contains unsupported characters");
    }
    Ok(())
}

fn validate_external_id(value: &str, field: &str, max_len: usize) -> Result<()> {
    if value.is_empty() || value.len() > max_len {
        bail!("{field} must contain 1 to {max_len} characters");
    }
    if value.chars().any(char::is_control) {
        bail!("{field} must not contain control characters");
    }
    Ok(())
}

fn validate_user(value: &str, field: &str) -> Result<()> {
    crate::domain::invoice::validate_user(value).with_context(|| format!("invalid {field}"))
}

fn validate_payment_hash(value: &str) -> Result<()> {
    if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        bail!("payment_hash must be a 32-byte hex string");
    }
    Ok(())
}

pub(super) fn to_i64(value: u64, field: &str) -> Result<i64> {
    i64::try_from(value).with_context(|| format!("{field} exceeds SQLite INTEGER range"))
}

pub(super) fn to_u64(value: i64, field: &str) -> Result<u64> {
    u64::try_from(value).with_context(|| format!("{field} is unexpectedly negative"))
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;

    async fn store() -> (TempDir, LedgerStore) {
        let directory = TempDir::new().unwrap();
        let path = directory.path().join("lightning-goats.db");
        let url = format!("sqlite://{}", path.display());
        let store = LedgerStore::connect(&url).await.unwrap();
        (directory, store)
    }

    fn payment(source_id: &str, hash: &str, sats: u64) -> SettledPayment {
        SettledPayment {
            source: "strike".to_owned(),
            source_id: source_id.to_owned(),
            payment_hash: Some(hash.to_owned()),
            address_user: "dexter".to_owned(),
            credit_pool: "herd".to_owned(),
            amount_msat: sats * 1_000,
            settled_at: Some(1_700_000_000),
            context_json: Some(r#"{"receive_request_id":"request-1"}"#.to_owned()),
        }
    }

    #[tokio::test]
    async fn provider_neutral_settlement_credits_atomically() {
        let (_directory, store) = store().await;
        let payment = payment("receive-1", &"11".repeat(32), 2_340);
        assert_eq!(
            store.record_payment(&payment).await.unwrap(),
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
        assert_eq!(payload["address_user"], "dexter");
        assert_eq!(payload["credit_pool"], "herd");
    }

    #[tokio::test]
    async fn duplicate_provider_delivery_is_idempotent() {
        let (_directory, store) = store().await;
        let payment = payment("receive-2", &"22".repeat(32), 1_000);
        store.record_payment(&payment).await.unwrap();
        assert_eq!(
            store.record_payment(&payment).await.unwrap(),
            SettlementOutcome::Duplicate
        );
        assert_eq!(store.feed_credit_sats().await.unwrap(), 1_000);
        assert_eq!(store.events_after(0, 10).await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn conflicting_source_identity_fails_closed() {
        let (_directory, store) = store().await;
        let payment = payment("receive-3", &"33".repeat(32), 1_000);
        store.record_payment(&payment).await.unwrap();
        let mut conflict = payment;
        conflict.amount_msat = 2_000_000;
        assert!(store.record_payment(&conflict).await.is_err());
        assert_eq!(store.feed_credit_sats().await.unwrap(), 1_000);
    }

    #[tokio::test]
    async fn payment_hash_collision_across_source_ids_fails_closed() {
        let (_directory, store) = store().await;
        let hash = "44".repeat(32);
        store
            .record_payment(&payment("receive-4a", &hash, 1_000))
            .await
            .unwrap();
        assert!(
            store
                .record_payment(&payment("receive-4b", &hash, 1_000))
                .await
                .is_err()
        );
        assert_eq!(store.feed_credit_sats().await.unwrap(), 1_000);
    }

    #[tokio::test]
    async fn refuses_non_sat_aligned_payment() {
        let (_directory, store) = store().await;
        let mut payment = payment("receive-5", &"55".repeat(32), 1_000);
        payment.amount_msat += 1;
        assert!(store.record_payment(&payment).await.is_err());
        assert_eq!(store.feed_credit_sats().await.unwrap(), 0);
    }
}
