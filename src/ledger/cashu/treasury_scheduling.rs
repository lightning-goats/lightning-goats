//! Claims survive restarts; uncertain issuance is never retried as a new creation.
use super::super::LedgerStore;
use crate::cashu::{
    receipt_authority::ReceiptPolicy,
    treasury_worker::{Controls, Stage, Work},
};
use anyhow::{Result, ensure};
use sqlx::Row;

impl LedgerStore {
    pub(crate) async fn claim_cashu_treasury_work(
        &self,
        registration: &ReceiptPolicy,
        controls: &Controls,
        now: i64,
        deadline: u64,
    ) -> Result<Option<Work>> {
        controls.validate()?;
        ensure!(
            now >= 0 && (1..=60).contains(&deadline),
            "invalid core treasury scheduling bounds"
        );
        if !(controls.issue || controls.deliver || controls.inspect) {
            return Ok(None);
        }
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        super::treasury::require_epoch(&mut tx, registration.core_epoch).await?;
        let row = sqlx::query("WITH work AS (SELECT i.id,i.rowid AS ordering,CASE WHEN EXISTS(SELECT 1 FROM cashu_treasury_issuances s WHERE s.intent_id=i.id) THEN 'wallet' ELSE 'issue' END AS stage FROM cashu_treasury_intents i WHERE json_extract(i.intent_json,'$.core_epoch')=? AND json_extract(i.intent_json,'$.wallet_instance')=? AND json_extract(i.intent_json,'$.wallet_epoch')=? AND NOT EXISTS(SELECT 1 FROM cashu_treasury_wallet_results r WHERE r.intent_id=i.id) AND (EXISTS(SELECT 1 FROM cashu_treasury_issuances s WHERE s.intent_id=i.id) OR NOT EXISTS(SELECT 1 FROM cashu_treasury_claims c WHERE c.intent_id=i.id))) SELECT w.id,w.stage,COALESCE(s.attempts,0) AS attempts FROM work w LEFT JOIN cashu_treasury_work s ON s.intent_id=w.id AND s.stage=w.stage WHERE ((? AND w.stage='issue') OR (? AND w.stage='wallet')) AND COALESCE(s.attempts,0)<? AND COALESCE(s.next_attempt,0)<=? ORDER BY COALESCE(s.next_attempt,0),w.ordering LIMIT 1")
            .bind(registration.core_epoch.to_string()).bind(registration.wallet_instance.to_string()).bind(registration.wallet_epoch.to_string())
            .bind(controls.issue).bind(controls.deliver || controls.inspect).bind(controls.maximum_attempts).bind(now).fetch_optional(&mut *tx).await?;
        let Some(row) = row else { return Ok(None) };
        let id: String = row.try_get("id")?;
        let stage: String = row.try_get("stage")?;
        let attempts: u32 = row.try_get("attempts")?;
        let next = now
            .checked_add(i64::try_from(deadline)?)
            .and_then(|v| v.checked_add((5_i64 << attempts.min(10)).min(3600)))
            .ok_or_else(|| anyhow::anyhow!("core treasury schedule overflow"))?;
        sqlx::query("INSERT INTO cashu_treasury_work(intent_id,stage,attempts,next_attempt) VALUES(?,?,?,?) ON CONFLICT(intent_id,stage) DO UPDATE SET attempts=excluded.attempts,next_attempt=excluded.next_attempt")
            .bind(&id).bind(&stage).bind(attempts+1).bind(next).execute(&mut *tx).await?;
        let work = Work {
            id: id.parse()?,
            stage: if stage == "issue" {
                Stage::Issue
            } else {
                Stage::Wallet
            },
        };
        tx.commit().await?;
        Ok(Some(work))
    }
}
