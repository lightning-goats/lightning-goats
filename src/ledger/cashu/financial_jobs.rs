//! Create source-bound treasury work from the original pending financial plan.
//! Planning and custody reservation share one transaction; no provider is called.
use super::{
    super::{LedgerStore, to_u64},
    treasury::{available_custody, require_epoch},
};
use crate::cashu::{receipt_authority::ReceiptPolicy, treasury::Policy};
use anyhow::{Context, Result, ensure};
use lightning_goats_cashu_receipt::treasury::Intent;
use sqlx::Row;
use std::collections::BTreeMap;

impl LedgerStore {
    pub(crate) async fn plan_cashu_feed_treasury(
        &self,
        registration: &ReceiptPolicy,
        policy: &Policy,
    ) -> Result<Option<Intent>> {
        policy.validate()?;
        if !policy.enabled {
            return Ok(None);
        }
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        require_epoch(&mut tx, registration.core_epoch).await?;
        let plan: Option<String> =
            sqlx::query_scalar("SELECT id FROM feed_financial_plans WHERE state='pending'")
                .fetch_optional(&mut *tx)
                .await?;
        let Some(plan) = plan else {
            return Ok(None);
        };
        let rows = sqlx::query("SELECT p.ledger_id,p.sats,c.mint_id,c.wallet_instance,c.wallet_epoch,c.core_epoch,l.remaining_sats FROM feed_financial_parts p JOIN cashu_receipt_inbox c ON c.ledger_entry_id=p.ledger_id JOIN reward_funding_lots l ON l.ledger_id=p.ledger_id WHERE p.plan_id=? ORDER BY p.ledger_id LIMIT 4097")
            .bind(&plan).fetch_all(&mut *tx).await?;
        ensure!(rows.len() <= 4096, "financial source bound exceeded");
        let mut by_mint: BTreeMap<String, Vec<(i64, u64)>> = BTreeMap::new();
        for row in rows {
            ensure!(
                row.try_get::<String, _>("wallet_instance")?
                    == registration.wallet_instance.to_string()
                    && row.try_get::<String, _>("wallet_epoch")?
                        == registration.wallet_epoch.to_string()
                    && row.try_get::<String, _>("core_epoch")?
                        == registration.core_epoch.to_string(),
                "financial source registration changed"
            );
            let ledger: i64 = row.try_get("ledger_id")?;
            let reserved = to_u64(row.try_get("sats")?, "financial source allocation")?;
            ensure!(
                reserved <= to_u64(row.try_get("remaining_sats")?, "source remaining credit")?,
                "financial source credit consumed"
            );
            let (processed, spent, consumed): (i64,i64,i64) = sqlx::query_as(
                "SELECT COALESCE(MAX(g.start_sats+g.input_sats),0),
                        COALESCE(SUM(r.input_sats-r.change_sats),0),
                        (SELECT COALESCE(SUM(sats),0) FROM feed_reward_funding_parts WHERE ledger_id=?1)
                 FROM cashu_settled_ranges g JOIN cashu_treasury_source_results r USING(intent_id,ledger_id)
                 WHERE g.ledger_id=?1",
            )
            .bind(ledger)
            .fetch_one(&mut *tx)
            .await?;
            let target = reserved
                .checked_add(to_u64(consumed, "consumed credit")?)
                .context("financial target overflow")?;
            let processed = to_u64(processed, "processed contribution credit")?;
            if processed >= target {
                continue;
            }
            let pending: i64 = sqlx::query_scalar("SELECT COALESCE(SUM(s.input_sats),0) FROM cashu_treasury_sources s WHERE s.ledger_id=? AND NOT EXISTS(SELECT 1 FROM cashu_treasury_completions c WHERE c.intent_id=s.intent_id)")
                .bind(ledger).fetch_one(&mut *tx).await?;
            let committed = to_u64(spent, "spent custody")?
                .checked_add(to_u64(pending, "pending custody")?)
                .context("committed custody overflow")?;
            let input = target
                .saturating_sub(committed)
                .min(available_custody(&mut tx, ledger).await?);
            // Batch returned change with fresh principal only if the combined input
            // reaches previously unprocessed credit.
            if input > 0 && input > processed.saturating_sub(committed) {
                by_mint
                    .entry(row.try_get("mint_id")?)
                    .or_default()
                    .push((ledger, input));
            }
        }
        let intent =
            super::treasury_planning::reserve_first(&mut tx, registration, policy, by_mint).await?;
        if let Some(intent) = &intent {
            sqlx::query("INSERT INTO feed_financial_treasury(plan_id,intent_id) VALUES(?,?)")
                .bind(&plan)
                .bind(intent.id.to_string())
                .execute(&mut *tx)
                .await?;
        }
        tx.commit().await?;
        Ok(intent)
    }
}
