//! Create source-bound treasury work from the original pending financial plan.
//! Planning and custody reservation share one transaction; no provider is called.
use super::{
    super::{LedgerStore, to_u64},
    treasury::{available_custody, require_epoch},
};
use crate::cashu::{receipt_authority::ReceiptPolicy, treasury::Policy};
use anyhow::{Result, ensure};
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
            // A completed transfer for this plan has already attempted its entire input.
            // Keep its change for a later batch. Dispatch still checks actual settled coverage.
            let assigned: i64 = sqlx::query_scalar(
                "SELECT MAX(0,
                    (SELECT COALESCE(SUM(r.input_sats -
                        CASE WHEN j.intent_id IS NULL THEN r.change_sats ELSE 0 END), 0)
                     FROM cashu_treasury_source_results r
                     JOIN cashu_treasury_completions c ON c.intent_id = r.intent_id
                     LEFT JOIN feed_financial_treasury j ON j.intent_id = r.intent_id AND j.plan_id = ?
                     WHERE r.ledger_id = ?)
                    - (SELECT COALESCE(SUM(sats), 0) FROM feed_reward_funding_parts WHERE ledger_id = ?))",
            )
            .bind(&plan)
            .bind(ledger)
            .bind(ledger)
            .fetch_one(&mut *tx)
            .await?;
            let pending: i64 = sqlx::query_scalar("SELECT COALESCE(SUM(s.input_sats),0) FROM cashu_treasury_sources s WHERE s.ledger_id=? AND NOT EXISTS(SELECT 1 FROM cashu_treasury_completions c WHERE c.intent_id=s.intent_id)")
                .bind(ledger).fetch_one(&mut *tx).await?;
            let needed = reserved
                .saturating_sub(to_u64(assigned, "source already assigned to settlement")?)
                .saturating_sub(to_u64(pending, "pending source reservation")?);
            let input = needed.min(available_custody(&mut tx, ledger).await?);
            if input > 0 {
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
