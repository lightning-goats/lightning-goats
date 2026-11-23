//! Explicit local financial-plan release. No custody hold or physical outcome is changed.
use super::{LedgerStore, to_u64};
use anyhow::{Context, Result, ensure};
use sqlx::Row;
use uuid::Uuid;

impl LedgerStore {
    /// Operator/maintenance callers must name the exact reviewed plan and threshold.
    /// This releases unconsumed credit only; retained treasury jobs may still settle.
    pub async fn release_financial_feed_plan(&self, id: Uuid, threshold: u64) -> Result<bool> {
        self.financial_plan_release(id, threshold, false).await
    }
    pub async fn preview_financial_feed_plan_release(
        &self,
        id: Uuid,
        threshold: u64,
    ) -> Result<bool> {
        self.financial_plan_release(id, threshold, true).await
    }
    async fn financial_plan_release(
        &self,
        id: Uuid,
        threshold: u64,
        dry_run: bool,
    ) -> Result<bool> {
        ensure!(
            !id.is_nil() && threshold > 0,
            "invalid financial release scope"
        );
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let row = sqlx::query(
            "SELECT state,threshold_sats,disposition FROM feed_financial_plans WHERE id=?",
        )
        .bind(id.to_string())
        .fetch_optional(&mut *tx)
        .await?
        .context("financial plan missing")?;
        ensure!(
            to_u64(row.try_get("threshold_sats")?, "financial threshold")? == threshold,
            "financial release threshold changed"
        );
        let state: String = row.try_get("state")?;
        let disposition: Option<String> = row.try_get("disposition")?;
        if state == "released" && disposition.as_deref() == Some("operator_release") {
            tx.commit().await?;
            return Ok(false);
        }
        ensure!(state == "pending", "financial plan is not pending");
        let physical:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM feed_attempts WHERE status IN ('intent_committed','unknown'))")
            .fetch_one(&mut *tx).await?;
        ensure!(
            !physical,
            "unresolved physical attempt prevents financial release"
        );
        if dry_run {
            tx.rollback().await?;
            return Ok(true);
        }
        ensure!(sqlx::query("UPDATE feed_financial_plans SET state='released',disposition='operator_release' WHERE id=? AND state='pending'")
            .bind(id.to_string()).execute(&mut *tx).await?.rows_affected()==1,"financial plan changed before release");
        tx.commit().await?;
        Ok(true)
    }
}
