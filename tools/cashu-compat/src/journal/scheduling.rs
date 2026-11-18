use super::*;

pub struct Work {
    pub id: Uuid,
    pub mint_id: String,
    pub state: String,
}

impl Journal {
    /// Delivery retries are independent of acquisition attempts and never re-swap funds.
    pub fn claim_receipt_delivery(
        &mut self,
        now: i64,
        maximum_attempts: u32,
        deadline_seconds: u64,
    ) -> Result<Option<Uuid>> {
        ensure!(
            now >= 0 && maximum_attempts <= 10_000 && (1..=60).contains(&deadline_seconds),
            "invalid receipt delivery scheduling bounds"
        );
        if maximum_attempts == 0 {
            return Ok(None);
        }
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let row: Option<(String,u32)> = tx.query_row(
            "SELECT a.id,COALESCE(s.attempts,0) FROM acquisitions a LEFT JOIN receipt_delivery_schedule s ON s.acquisition_id=a.id WHERE a.state IN ('acquired','credit_export_pending') AND COALESCE(s.attempts,0)<? AND COALESCE(s.next_attempt,0)<=? ORDER BY COALESCE(s.next_attempt,0),a.rowid LIMIT 1",
            params![maximum_attempts,now], |row| Ok((row.get(0)?,row.get(1)?)),
        ).optional()?;
        let Some((id, attempts)) = row else {
            return Ok(None);
        };
        let parsed = Uuid::parse_str(&id)?;
        let backoff = (5_i64 << attempts.min(10)).min(3600);
        let next = now
            .checked_add(i64::try_from(deadline_seconds)?)
            .and_then(|v| v.checked_add(backoff))
            .ok_or_else(|| anyhow::anyhow!("receipt delivery schedule overflow"))?;
        tx.execute("INSERT INTO receipt_delivery_schedule(acquisition_id,attempts,next_attempt) VALUES(?,?,?) ON CONFLICT(acquisition_id) DO UPDATE SET attempts=excluded.attempts,next_attempt=excluded.next_attempt",params![id,attempts+1,next])?;
        tx.commit()?;
        Ok(Some(parsed))
    }
    /// Charge a durable attempt before external work. Exhaustion never releases proofs.
    pub fn claim_acquisition_work(
        &mut self,
        now: i64,
        acquire: bool,
        reconcile: bool,
        maximum_attempts: u32,
        deadline_seconds: u64,
    ) -> Result<Option<Work>> {
        ensure!(
            now >= 0 && maximum_attempts <= 10_000 && (1..=60).contains(&deadline_seconds),
            "invalid acquisition scheduling bounds"
        );
        if (!acquire && !reconcile) || maximum_attempts == 0 {
            return Ok(None);
        }
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let swap_limits = match &self.custody_limits {
            Some(limits) if acquire => Some(serde_json::to_string(&limits.swap_limits(&tx)?)?),
            _ => None,
        };
        let row: Option<(String, String, String, u32)> = tx
            .query_row(
                "SELECT a.id,a.mint_id,a.state,COALESCE(s.attempts,0)
             FROM acquisitions a LEFT JOIN acquisition_schedule s ON s.acquisition_id=a.id
             WHERE ((?1 AND a.state='validated' AND (?5 IS NULL OR a.gross <= COALESCE(
                 (SELECT value FROM json_each(?5) WHERE key=a.mint_id),0)))
                 OR (?2 AND a.state IN ('acquisition_prepared','acquiring')))
             AND COALESCE(s.attempts,0)<?3 AND COALESCE(s.next_attempt,0)<=?4
             ORDER BY COALESCE(s.next_attempt,0),a.rowid LIMIT 1",
                params![acquire, reconcile, maximum_attempts, now, swap_limits],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .optional()?;
        let Some((id, mint_id, state, attempts)) = row else {
            return Ok(None);
        };
        let backoff = (5_i64 << attempts.min(10)).min(3600);
        let next = now
            .checked_add(i64::try_from(deadline_seconds)?)
            .and_then(|v| v.checked_add(backoff))
            .ok_or_else(|| anyhow::anyhow!("acquisition schedule overflow"))?;
        let work = Work {
            id: Uuid::parse_str(&id)?,
            mint_id,
            state,
        };
        tx.execute(
            "INSERT INTO acquisition_schedule(acquisition_id,attempts,next_attempt) VALUES(?,?,?) ON CONFLICT(acquisition_id) DO UPDATE SET attempts=excluded.attempts,next_attempt=excluded.next_attempt",
            params![id,attempts+1,next],
        )?;
        tx.commit()?;
        Ok(Some(work))
    }
}
