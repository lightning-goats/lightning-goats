//! Claims survive restarts; uncertain issuance is never retried as a new creation.
use super::super::LedgerStore;
use crate::cashu::{
    receipt_authority::ReceiptPolicy,
    treasury_worker::{Controls, Stage, Work},
};
use anyhow::{Result, bail, ensure};
use sqlx::Row;

fn inspection_table(project: bool) -> &'static str {
    if project {
        "breez_treasury_inspections"
    } else {
        "cashu_treasury_inspections"
    }
}

impl LedgerStore {
    pub(crate) async fn claim_cashu_treasury_work(
        &self,
        registration: &ReceiptPolicy,
        controls: &Controls,
        project: bool,
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
        let inspections = inspection_table(project);
        let sql = format!("WITH pending AS (
                SELECT i.id,i.rowid AS ordering,(s.intent_id IS NOT NULL OR b.invoice IS NOT NULL) AS issued,
                       COALESCE(c.delivery_closed,0) AS delivery_closed
                FROM cashu_treasury_intents i
                LEFT JOIN cashu_treasury_issuances s ON s.intent_id=i.id
                LEFT JOIN breez_project_invoices b ON b.id=i.id AND json_extract(b.intent,'$.purpose')='treasury_conversion'
                LEFT JOIN {inspections} c ON c.intent_id=i.id
                WHERE (json_extract(i.intent_json,'$.destination.origin')=?)=? AND json_extract(i.intent_json,'$.core_epoch')=?
                  AND json_extract(i.intent_json,'$.wallet_instance')=?
                  AND json_extract(i.intent_json,'$.wallet_epoch')=?
                  AND NOT EXISTS(SELECT 1 FROM cashu_treasury_wallet_results r WHERE r.intent_id=i.id)
                  AND NOT EXISTS(SELECT 1 FROM breez_treasury_wallet_results r WHERE r.intent_id=i.id)
            ), work AS (
                SELECT p.id,p.ordering,s.stage,COALESCE(w.attempts,0) AS attempts,
                       COALESCE(w.next_attempt,0) AS due
                FROM pending p
                CROSS JOIN (SELECT 'issue' AS stage UNION ALL SELECT 'wallet') s
                LEFT JOIN cashu_treasury_work w ON w.intent_id=p.id AND w.stage=s.stage
                WHERE ((? AND s.stage='issue' AND NOT p.issued
                         AND NOT EXISTS(SELECT 1 FROM cashu_treasury_claims c WHERE c.intent_id=p.id))
                    OR (? AND s.stage='wallet' AND p.issued AND NOT p.delivery_closed))
                  AND COALESCE(w.attempts,0)<?
                UNION ALL
                SELECT p.id,p.ordering,'inspect',COALESCE(c.attempts,0),COALESCE(c.next_attempt,0)
                FROM pending p LEFT JOIN {inspections} c ON c.intent_id=p.id
                WHERE ? AND p.issued
            )
            SELECT id,stage,attempts FROM work WHERE due<=?
            ORDER BY due,ordering,stage='inspect' LIMIT 1");
        let row = sqlx::query(&sql)
            .bind(crate::cashu::treasury::PROJECT_MINT_ORIGIN)
            .bind(project)
            .bind(registration.core_epoch.to_string())
            .bind(registration.wallet_instance.to_string())
            .bind(registration.wallet_epoch.to_string())
            .bind(controls.issue)
            .bind(controls.deliver)
            .bind(controls.maximum_attempts)
            .bind(controls.inspect)
            .bind(now)
            .fetch_optional(&mut *tx)
            .await?;
        let Some(row) = row else { return Ok(None) };
        let id: String = row.try_get("id")?;
        let stage: String = row.try_get("stage")?;
        let attempts: u32 = row.try_get("attempts")?;
        let stage = match stage.as_str() {
            "issue" => Stage::Issue,
            "wallet" => Stage::Deliver,
            "inspect" => Stage::Inspect,
            _ => bail!("unknown treasury scheduler stage"),
        };
        let delay = if stage == Stage::Inspect {
            (5_i64 << attempts.min(6)).min(300)
        } else {
            (5_i64 << attempts.min(10)).min(3600)
        };
        let next = now
            .checked_add(i64::try_from(deadline)?)
            .and_then(|v| v.checked_add(delay))
            .ok_or_else(|| anyhow::anyhow!("core treasury schedule overflow"))?;
        if stage == Stage::Inspect {
            sqlx::query(&format!("INSERT INTO {inspections}(intent_id,attempts,next_attempt) VALUES(?,?,?) ON CONFLICT(intent_id) DO UPDATE SET attempts=excluded.attempts,next_attempt=excluded.next_attempt"))
                .bind(&id).bind((attempts+1).min(6)).bind(next).execute(&mut *tx).await?;
        } else {
            let label = if stage == Stage::Issue {
                "issue"
            } else {
                "wallet"
            };
            sqlx::query("INSERT INTO cashu_treasury_work(intent_id,stage,attempts,next_attempt) VALUES(?,?,?,?) ON CONFLICT(intent_id,stage) DO UPDATE SET attempts=excluded.attempts,next_attempt=excluded.next_attempt")
                .bind(&id).bind(label).bind(attempts+1).bind(next).execute(&mut *tx).await?;
            if stage == Stage::Deliver {
                // Wait out the first delivery claim before attempting read-only recovery.
                sqlx::query(&format!("INSERT INTO {inspections}(intent_id,next_attempt) VALUES(?,?) ON CONFLICT(intent_id) DO NOTHING"))
                    .bind(&id).bind(next).execute(&mut *tx).await?;
            }
        }
        let work = Work {
            id: id.parse()?,
            stage,
        };
        tx.commit().await?;
        Ok(Some(work))
    }

    pub(crate) async fn close_cashu_treasury_delivery(
        &self,
        id: uuid::Uuid,
        project: bool,
    ) -> Result<()> {
        let sql = format!(
            "UPDATE {} SET delivery_closed=1 WHERE intent_id=?",
            inspection_table(project)
        );
        let result = sqlx::query(&sql)
            .bind(id.to_string())
            .execute(&self.pool)
            .await?;
        ensure!(
            result.rows_affected() == 1,
            "treasury inspection schedule missing"
        );
        Ok(())
    }
}
