//! Shared-store admission for public invoice creation, independent of recovery.
use super::LedgerStore;
use anyhow::Result;
use uuid::Uuid;

impl LedgerStore {
    pub(crate) async fn reserve_invoice(&self) -> Result<Option<Uuid>> {
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        sqlx::query("DELETE FROM invoice_admissions WHERE created_at<=unixepoch()-60 AND (finished=1 OR expires_at<=unixepoch())").execute(&mut *tx).await?;
        let recent: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM invoice_admissions WHERE created_at>unixepoch()-60",
        )
        .fetch_one(&mut *tx)
        .await?;
        let active: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM invoice_admissions WHERE finished=0 AND expires_at>unixepoch()",
        )
        .fetch_one(&mut *tx)
        .await?;
        let recently_issued: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM strike_receive_requests WHERE created_at>unixepoch()-86400",
        )
        .fetch_one(&mut *tx)
        .await?;
        if recent >= 30 || active >= 4 || recently_issued + active >= 10_000 {
            tx.commit().await?;
            return Ok(None);
        }
        let id = Uuid::new_v4();
        sqlx::query("INSERT INTO invoice_admissions(id) VALUES (?)")
            .bind(id.to_string())
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(Some(id))
    }

    pub(crate) async fn finish_invoice_admission(&self, id: Uuid) -> Result<()> {
        sqlx::query("UPDATE invoice_admissions SET finished=1 WHERE id=?")
            .bind(id.to_string())
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn admission_is_shared_durable_and_bounded_across_connections() {
        let directory = tempfile::TempDir::new().unwrap();
        let url = format!("sqlite://{}", directory.path().join("budget.db").display());
        let ledger = LedgerStore::connect(&url).await.unwrap();
        let other = LedgerStore::connect(&url).await.unwrap();
        let mut active = Vec::new();
        for _ in 0..4 {
            active.push(ledger.reserve_invoice().await.unwrap().unwrap());
        }
        assert!(other.reserve_invoice().await.unwrap().is_none());
        // Interrupted tasks cannot release their rows; the persisted lease expires.
        sqlx::query("UPDATE invoice_admissions SET expires_at=unixepoch()-1")
            .execute(&ledger.pool)
            .await
            .unwrap();
        let recovered = other.reserve_invoice().await.unwrap().unwrap();
        other.finish_invoice_admission(recovered).await.unwrap();
        for id in active {
            other.finish_invoice_admission(id).await.unwrap();
        }
        for _ in 5..30 {
            let id = other.reserve_invoice().await.unwrap().unwrap();
            other.finish_invoice_admission(id).await.unwrap();
        }
        let reopened = LedgerStore::connect(&url).await.unwrap();
        assert!(reopened.reserve_invoice().await.unwrap().is_none());
        sqlx::query(
            "UPDATE invoice_admissions SET created_at=unixepoch()-61,expires_at=unixepoch()-1",
        )
        .execute(&ledger.pool)
        .await
        .unwrap();
        assert!(reopened.reserve_invoice().await.unwrap().is_some());
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM invoice_admissions")
            .fetch_one(&ledger.pool)
            .await
            .unwrap();
        assert_eq!(count, 1);
    }

    #[tokio::test]
    async fn concurrent_reservations_and_daily_budget_recover_without_deleting_history() {
        let directory = tempfile::TempDir::new().unwrap();
        let url = format!(
            "sqlite://{}",
            directory.path().join("retained.db").display()
        );
        let ledger = LedgerStore::connect(&url).await.unwrap();
        let other = LedgerStore::connect(&url).await.unwrap();
        let reservations = futures_util::future::join_all((0..12).map(|i| {
            let store = if i % 2 == 0 {
                ledger.clone()
            } else {
                other.clone()
            };
            async move { store.reserve_invoice().await.unwrap() }
        }))
        .await;
        assert_eq!(reservations.iter().filter(|id| id.is_some()).count(), 4);
        for id in reservations.into_iter().flatten() {
            ledger.finish_invoice_admission(id).await.unwrap();
        }
        sqlx::query("WITH RECURSIVE n(x) AS (SELECT 1 UNION ALL SELECT x+1 FROM n WHERE x<10000) INSERT INTO strike_receive_requests(receive_request_id,address_user,credit_pool,amount_msat,description_hash,payment_hash,invoice) SELECT 'synthetic:'||x,'herd','herd',1000,printf('%064x',0),printf('%064x',x),'fixture:'||x FROM n").execute(&ledger.pool).await.unwrap();
        assert!(other.reserve_invoice().await.unwrap().is_none());
        sqlx::query("UPDATE strike_receive_requests SET created_at=unixepoch()-86401")
            .execute(&ledger.pool)
            .await
            .unwrap();
        assert!(other.reserve_invoice().await.unwrap().is_some());
        let retained: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM strike_receive_requests")
            .fetch_one(&ledger.pool)
            .await
            .unwrap();
        assert_eq!(retained, 10_000);
    }
}
