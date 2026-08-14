use std::{str::FromStr, time::Duration};

use anyhow::{Context, Result, bail};
use sqlx::{
    Row, SqlitePool,
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous},
};

use crate::domain::invoice::ClnAddressInvoiceLabel;

static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");

#[derive(Debug, Clone)]
pub struct LedgerStore {
    pool: SqlitePool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaidInvoice {
    pub pay_index: u64,
    pub payment_hash: String,
    pub label: Option<String>,
    pub amount_msat: u64,
    pub settled_at: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SettlementOutcome {
    Credited { sats: u64, user: String },
    Ignored,
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

    pub async fn initialize_cursor(&self, last_pay_index: u64) -> Result<()> {
        let last_pay_index = to_i64(last_pay_index, "last_pay_index")?;
        let mut transaction = self.pool.begin().await?;

        let existing = sqlx::query("SELECT last_pay_index FROM cln_cursor WHERE singleton = 1")
            .fetch_optional(&mut *transaction)
            .await?;

        if let Some(row) = existing {
            let current: i64 = row.try_get("last_pay_index")?;
            if current != last_pay_index {
                bail!(
                    "CLN cursor is already initialized at {current}; refusing to replace it with {last_pay_index}"
                );
            }
            transaction.commit().await?;
            return Ok(());
        }

        sqlx::query(
            "INSERT INTO cln_cursor (singleton, last_pay_index) VALUES (1, ?)",
        )
        .bind(last_pay_index)
        .execute(&mut *transaction)
        .await?;

        transaction.commit().await?;
        Ok(())
    }

    pub async fn last_pay_index(&self) -> Result<Option<u64>> {
        let row = sqlx::query("SELECT last_pay_index FROM cln_cursor WHERE singleton = 1")
            .fetch_optional(&self.pool)
            .await?;

        row.map(|row| {
            let value: i64 = row.try_get("last_pay_index")?;
            to_u64(value, "last_pay_index")
        })
        .transpose()
    }

    pub async fn record_settlement(
        &self,
        invoice: &PaidInvoice,
        herd_user: &str,
    ) -> Result<SettlementOutcome> {
        let pay_index = to_i64(invoice.pay_index, "pay_index")?;
        let amount_msat = to_i64(invoice.amount_msat, "amount_msat")?;
        let mut transaction = self.pool.begin().await?;

        let cursor_row = sqlx::query("SELECT last_pay_index FROM cln_cursor WHERE singleton = 1")
            .fetch_optional(&mut *transaction)
            .await?
            .context("CLN cursor is not initialized")?;
        let current_cursor: i64 = cursor_row.try_get("last_pay_index")?;

        if pay_index <= current_cursor {
            let existing = sqlx::query(
                "SELECT payment_hash FROM settled_invoices WHERE pay_index = ?",
            )
            .bind(pay_index)
            .fetch_optional(&mut *transaction)
            .await?;

            if let Some(existing) = existing {
                let existing_hash: String = existing.try_get("payment_hash")?;
                if existing_hash == invoice.payment_hash {
                    transaction.commit().await?;
                    return Ok(SettlementOutcome::Duplicate);
                }
            }

            bail!(
                "received out-of-order settlement pay_index={} while cursor={current_cursor}",
                invoice.pay_index
            );
        }

        let existing_hash = sqlx::query(
            "SELECT pay_index FROM settled_invoices WHERE payment_hash = ?",
        )
        .bind(&invoice.payment_hash)
        .fetch_optional(&mut *transaction)
        .await?;
        if let Some(existing_hash) = existing_hash {
            let existing_pay_index: i64 = existing_hash.try_get("pay_index")?;
            bail!(
                "payment_hash {} already exists at pay_index {existing_pay_index}, but was received at {}",
                invoice.payment_hash,
                invoice.pay_index
            );
        }

        let classification = classify(invoice.label.as_deref(), herd_user);
        let (classified_user, credited_sats) = match classification {
            Some(user) => (Some(user), invoice.amount_msat / 1_000),
            None => (None, 0),
        };
        let credited_sats_i64 = to_i64(credited_sats, "credited_sats")?;

        sqlx::query(
            r#"
            INSERT INTO settled_invoices
                (pay_index, payment_hash, label, amount_msat, classified_user, credited_sats, settled_at)
            VALUES (?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(pay_index)
        .bind(&invoice.payment_hash)
        .bind(invoice.label.as_deref())
        .bind(amount_msat)
        .bind(classified_user.as_deref())
        .bind(credited_sats_i64)
        .bind(invoice.settled_at)
        .execute(&mut *transaction)
        .await?;

        if credited_sats > 0 {
            let source_key = format!("cln:{}", invoice.payment_hash);
            sqlx::query(
                r#"
                INSERT INTO ledger_entries
                    (entry_type, source_key, delta_sats, payment_hash)
                VALUES ('HERD_RECEIPT', ?, ?, ?)
                "#,
            )
            .bind(source_key)
            .bind(credited_sats_i64)
            .bind(&invoice.payment_hash)
            .execute(&mut *transaction)
            .await?;
        }

        sqlx::query(
            "UPDATE cln_cursor SET last_pay_index = ?, updated_at = unixepoch() WHERE singleton = 1",
        )
        .bind(pay_index)
        .execute(&mut *transaction)
        .await?;

        transaction.commit().await?;

        match classified_user {
            Some(user) => Ok(SettlementOutcome::Credited {
                sats: credited_sats,
                user,
            }),
            None => Ok(SettlementOutcome::Ignored),
        }
    }

    pub async fn feed_credit_sats(&self) -> Result<u64> {
        let row = sqlx::query(
            "SELECT COALESCE(SUM(delta_sats), 0) AS credit FROM ledger_entries",
        )
        .fetch_one(&self.pool)
        .await?;
        let credit: i64 = row.try_get("credit")?;
        if credit < 0 {
            bail!("ledger invariant violated: feed credit is negative ({credit})");
        }
        to_u64(credit, "feed credit")
    }
}

fn classify(label: Option<&str>, herd_user: &str) -> Option<String> {
    let parsed = ClnAddressInvoiceLabel::parse(label?).ok()?;
    parsed.is_for_user(herd_user).then(|| parsed.user().to_owned())
}

fn to_i64(value: u64, field: &str) -> Result<i64> {
    i64::try_from(value).with_context(|| format!("{field} exceeds SQLite INTEGER range"))
}

fn to_u64(value: i64, field: &str) -> Result<u64> {
    u64::try_from(value).with_context(|| format!("{field} is unexpectedly negative"))
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;

    async fn store(initial_cursor: u64) -> (TempDir, LedgerStore) {
        let directory = TempDir::new().unwrap();
        let path = directory.path().join("lightning-goats.db");
        let url = format!("sqlite://{}", path.display());
        let store = LedgerStore::connect(&url).await.unwrap();
        store.initialize_cursor(initial_cursor).await.unwrap();
        (directory, store)
    }

    fn invoice(pay_index: u64, hash: &str, label: &str, sats: u64) -> PaidInvoice {
        PaidInvoice {
            pay_index,
            payment_hash: hash.to_owned(),
            label: Some(label.to_owned()),
            amount_msat: sats * 1_000,
            settled_at: Some(1_700_000_000),
        }
    }

    #[tokio::test]
    async fn herd_settlement_credits_and_advances_cursor_atomically() {
        let (_directory, store) = store(100).await;
        let paid = invoice(
            101,
            "hash-a",
            "clnaddress:v1:herd:550e8400-e29b-41d4-a716-446655440000",
            2_340,
        );

        let outcome = store.record_settlement(&paid, "herd").await.unwrap();
        assert_eq!(
            outcome,
            SettlementOutcome::Credited {
                sats: 2_340,
                user: "herd".to_owned()
            }
        );
        assert_eq!(store.feed_credit_sats().await.unwrap(), 2_340);
        assert_eq!(store.last_pay_index().await.unwrap(), Some(101));
    }

    #[tokio::test]
    async fn unrelated_address_advances_cursor_without_credit() {
        let (_directory, store) = store(100).await;
        let paid = invoice(
            101,
            "hash-b",
            "clnaddress:v1:donate:550e8400-e29b-41d4-a716-446655440000",
            5_000,
        );

        assert_eq!(
            store.record_settlement(&paid, "herd").await.unwrap(),
            SettlementOutcome::Ignored
        );
        assert_eq!(store.feed_credit_sats().await.unwrap(), 0);
        assert_eq!(store.last_pay_index().await.unwrap(), Some(101));
    }

    #[tokio::test]
    async fn duplicate_settlement_is_idempotent() {
        let (_directory, store) = store(100).await;
        let paid = invoice(
            101,
            "hash-c",
            "clnaddress:v1:herd:550e8400-e29b-41d4-a716-446655440000",
            1_000,
        );

        store.record_settlement(&paid, "herd").await.unwrap();
        assert_eq!(
            store.record_settlement(&paid, "herd").await.unwrap(),
            SettlementOutcome::Duplicate
        );
        assert_eq!(store.feed_credit_sats().await.unwrap(), 1_000);
    }

    #[tokio::test]
    async fn malformed_herd_like_label_fails_closed() {
        let (_directory, store) = store(100).await;
        let paid = invoice(101, "hash-d", "clnaddress:v1:herd:not-a-uuid", 9_000);

        assert_eq!(
            store.record_settlement(&paid, "herd").await.unwrap(),
            SettlementOutcome::Ignored
        );
        assert_eq!(store.feed_credit_sats().await.unwrap(), 0);
    }

    #[tokio::test]
    async fn out_of_order_unknown_settlement_is_rejected() {
        let (_directory, store) = store(101).await;
        let paid = invoice(
            101,
            "different-hash",
            "clnaddress:v1:herd:550e8400-e29b-41d4-a716-446655440000",
            1_000,
        );

        assert!(store.record_settlement(&paid, "herd").await.is_err());
        assert_eq!(store.feed_credit_sats().await.unwrap(), 0);
    }
}
