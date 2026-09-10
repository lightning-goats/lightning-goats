use anyhow::{Context, Result, bail};
use sqlx::Row;
use uuid::Uuid;

use super::{LedgerStore, to_i64, to_u64};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredStrikeReceiveRequest {
    pub receive_request_id: Uuid,
    pub address_user: String,
    pub credit_pool: String,
    pub amount_msat: u64,
    pub description_hash: String,
    pub payment_hash: String,
    pub invoice: String,
    pub created_provider: Option<String>,
}

impl LedgerStore {
    pub async fn record_strike_receive_request(
        &self,
        request: &StoredStrikeReceiveRequest,
    ) -> Result<()> {
        validate_hex32(&request.description_hash, "Strike description_hash")?;
        validate_hex32(&request.payment_hash, "Strike payment_hash")?;
        crate::domain::invoice::validate_user(&request.address_user)
            .context("invalid Strike address_user")?;
        crate::domain::invoice::validate_user(&request.credit_pool)
            .context("invalid Strike credit_pool")?;
        if request.amount_msat == 0 {
            bail!("Strike receive request amount_msat must be greater than zero");
        }
        if request.invoice.is_empty() || request.invoice.len() > 8_192 {
            bail!("Strike BOLT11 invoice must contain 1 to 8192 characters");
        }

        let amount_msat = to_i64(request.amount_msat, "Strike amount_msat")?;
        let id = request.receive_request_id.to_string();
        let mut transaction = self.pool.begin().await?;
        let existing = sqlx::query(
            r#"
            SELECT address_user, credit_pool, amount_msat, description_hash,
                   payment_hash, invoice, created_provider
            FROM strike_receive_requests
            WHERE receive_request_id = ?
            "#,
        )
        .bind(&id)
        .fetch_optional(&mut *transaction)
        .await?;

        if let Some(row) = existing {
            let existing_amount: i64 = row.try_get("amount_msat")?;
            let matches = row.try_get::<String, _>("address_user")? == request.address_user
                && row.try_get::<String, _>("credit_pool")? == request.credit_pool
                && existing_amount == amount_msat
                && row.try_get::<String, _>("description_hash")? == request.description_hash
                && row.try_get::<String, _>("payment_hash")? == request.payment_hash
                && row.try_get::<String, _>("invoice")? == request.invoice
                && row.try_get::<Option<String>, _>("created_provider")?
                    == request.created_provider;
            if matches {
                transaction.commit().await?;
                return Ok(());
            }
            bail!(
                "Strike receive_request_id {} already exists with conflicting request data",
                request.receive_request_id
            );
        }

        sqlx::query(
            r#"
            INSERT INTO strike_receive_requests
                (receive_request_id, address_user, credit_pool, amount_msat,
                 description_hash, payment_hash, invoice, created_provider)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&id)
        .bind(&request.address_user)
        .bind(&request.credit_pool)
        .bind(amount_msat)
        .bind(request.description_hash.to_ascii_lowercase())
        .bind(request.payment_hash.to_ascii_lowercase())
        .bind(&request.invoice)
        .bind(request.created_provider.as_deref())
        .execute(&mut *transaction)
        .await
        .context("failed persisting Strike receive request")?;

        transaction.commit().await?;
        Ok(())
    }

    pub async fn strike_receive_request(
        &self,
        receive_request_id: Uuid,
    ) -> Result<Option<StoredStrikeReceiveRequest>> {
        let row = sqlx::query(
            r#"
            SELECT address_user, credit_pool, amount_msat, description_hash,
                   payment_hash, invoice, created_provider
            FROM strike_receive_requests
            WHERE receive_request_id = ?
            "#,
        )
        .bind(receive_request_id.to_string())
        .fetch_optional(&self.pool)
        .await?;

        row.map(|row| {
            let amount_msat: i64 = row.try_get("amount_msat")?;
            Ok(StoredStrikeReceiveRequest {
                receive_request_id,
                address_user: row.try_get("address_user")?,
                credit_pool: row.try_get("credit_pool")?,
                amount_msat: to_u64(amount_msat, "Strike amount_msat")?,
                description_hash: row.try_get("description_hash")?,
                payment_hash: row.try_get("payment_hash")?,
                invoice: row.try_get("invoice")?,
                created_provider: row.try_get("created_provider")?,
            })
        })
        .transpose()
    }
}

fn validate_hex32(value: &str, field: &str) -> Result<()> {
    if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        bail!("{field} must be a 32-byte hex string");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;

    async fn store() -> (TempDir, LedgerStore) {
        let directory = TempDir::new().unwrap();
        let path = directory.path().join("strike-request.db");
        let store = LedgerStore::connect(&format!("sqlite://{}", path.display()))
            .await
            .unwrap();
        (directory, store)
    }

    fn request() -> StoredStrikeReceiveRequest {
        StoredStrikeReceiveRequest {
            receive_request_id: Uuid::parse_str("0191382f-387c-4eec-bc74-980872bfc5e5").unwrap(),
            address_user: "dexter".to_owned(),
            credit_pool: "herd".to_owned(),
            amount_msat: 2_340_000,
            description_hash: "11".repeat(32),
            payment_hash: "22".repeat(32),
            invoice: "lnbc2340n1test".to_owned(),
            created_provider: Some("2026-09-07T17:00:00Z".to_owned()),
        }
    }

    #[tokio::test]
    async fn round_trips_and_accepts_exact_duplicate() {
        let (_directory, store) = store().await;
        let request = request();
        store.record_strike_receive_request(&request).await.unwrap();
        store.record_strike_receive_request(&request).await.unwrap();
        assert_eq!(
            store
                .strike_receive_request(request.receive_request_id)
                .await
                .unwrap(),
            Some(request)
        );
    }

    #[tokio::test]
    async fn conflicting_duplicate_fails_closed() {
        let (_directory, store) = store().await;
        let request = request();
        store.record_strike_receive_request(&request).await.unwrap();
        let mut conflicting = request;
        conflicting.amount_msat += 1_000;
        assert!(
            store
                .record_strike_receive_request(&conflicting)
                .await
                .is_err()
        );
    }
}
