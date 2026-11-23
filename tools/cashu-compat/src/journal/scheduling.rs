use super::*;

pub struct Work {
    pub id: Uuid,
    pub mint_id: String,
    pub state: String,
    pub(crate) inspection_only: bool,
    attempts: u32,
    due: i64,
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
    /// Advance the durable schedule before external work. Exhaustion never releases proofs.
    pub fn claim_acquisition_work(
        &mut self,
        now: i64,
        acquire: bool,
        reconcile: bool,
        maximum_attempts: u32,
        deadline_seconds: u64,
    ) -> Result<Option<Work>> {
        let Some(work) = self
            .ready_acquisition_work(now, acquire, reconcile, maximum_attempts, None)?
            .into_iter()
            .next()
        else {
            ensure!(
                (1..=60).contains(&deadline_seconds),
                "invalid acquisition scheduling bounds"
            );
            return Ok(None);
        };
        Ok(self
            .charge_acquisition_work(&work, now, deadline_seconds)?
            .then_some(work))
    }

    /// Select one fresh and one reconciliation candidate per eligible mint.
    /// Selection alone leaves the durable attempt budget untouched.
    pub(crate) fn ready_acquisition_work(
        &self,
        now: i64,
        acquire: bool,
        reconcile: bool,
        maximum_attempts: u32,
        configured_mints: Option<&[&str]>,
    ) -> Result<Vec<Work>> {
        ensure!(
            now >= 0 && maximum_attempts <= 10_000,
            "invalid acquisition scheduling bounds"
        );
        if (!acquire && !reconcile) || maximum_attempts == 0 {
            return Ok(Vec::new());
        }
        let swap_limits = match &self.custody_limits {
            Some(limits) if acquire => Some(serde_json::to_string(
                &limits.swap_limits(&self.connection)?,
            )?),
            _ => None,
        };
        let configured = configured_mints.map(serde_json::to_string).transpose()?;
        let mut statement = self.connection.prepare(
            "WITH eligible AS (
                 SELECT a.id,a.mint_id,a.state,COALESCE(s.attempts,0) AS attempts,
                        COALESCE(s.next_attempt,0) AS due,a.rowid AS position,
                        COALESCE(s.attempts,0)>=?3 AS inspection_only,
                        ROW_NUMBER() OVER (
                            PARTITION BY a.mint_id,a.state='validated'
                            ORDER BY COALESCE(s.next_attempt,0),a.rowid) AS rank
                 FROM acquisitions a LEFT JOIN acquisition_schedule s ON s.acquisition_id=a.id
                 WHERE (?6 IS NULL OR a.mint_id IN (SELECT value FROM json_each(?6)))
                   AND ((?1 AND a.state='validated' AND (?5 IS NULL OR a.gross <= COALESCE(
                           (SELECT value FROM json_each(?5) WHERE key=a.mint_id),0)))
                        OR (?2 AND a.state IN ('acquisition_prepared','acquiring')))
                   AND (COALESCE(s.attempts,0)<?3 OR (
                       a.state='acquiring' AND a.request IS NOT NULL
                       AND EXISTS(SELECT 1 FROM swap_derivations d WHERE d.acquisition_id=a.id)))
                   AND COALESCE(s.next_attempt,0)<=?4
             )
             SELECT id,mint_id,state,attempts,due,inspection_only
             FROM eligible WHERE rank=1 ORDER BY due,position",
        )?;
        let rows = statement.query_map(
            params![
                acquire,
                reconcile,
                maximum_attempts,
                now,
                swap_limits,
                configured
            ],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                ))
            },
        )?;
        rows.map(|row| {
            let (id, mint_id, state, attempts, due, inspection_only) = row?;
            Ok(Work {
                id: Uuid::parse_str(&id)?,
                mint_id,
                state,
                inspection_only,
                attempts,
                due,
            })
        })
        .collect()
    }

    /// Revalidate the selected row and custody when committing the attempt.
    pub(crate) fn charge_acquisition_work(
        &mut self,
        work: &Work,
        now: i64,
        deadline_seconds: u64,
    ) -> Result<bool> {
        ensure!(
            now >= 0 && (1..=60).contains(&deadline_seconds),
            "invalid acquisition scheduling bounds"
        );
        let mut backoff = (5_i64 << work.attempts.min(10)).min(3600);
        if work.inspection_only {
            backoff = backoff.max(300);
        }
        let next = now
            .checked_add(i64::try_from(deadline_seconds)?)
            .and_then(|v| v.checked_add(backoff))
            .ok_or_else(|| anyhow::anyhow!("acquisition schedule overflow"))?;
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let gross: Option<u64> = tx.query_row(
            "SELECT a.gross FROM acquisitions a LEFT JOIN acquisition_schedule s ON s.acquisition_id=a.id
             WHERE a.id=? AND a.mint_id=? AND a.state=?
               AND COALESCE(s.attempts,0)=? AND COALESCE(s.next_attempt,0)=?
               AND COALESCE(s.next_attempt,0)<=?
               AND (NOT ? OR (a.request IS NOT NULL AND EXISTS(
                   SELECT 1 FROM swap_derivations d WHERE d.acquisition_id=a.id)))",
            params![work.id.to_string(), work.mint_id, work.state, work.attempts, work.due, now, work.inspection_only],
            |row| row.get(0),
        ).optional()?;
        let Some(gross) = gross else {
            return Ok(false);
        };
        if work.state == "validated"
            && let Some(limits) = &self.custody_limits
            && gross
                > limits
                    .swap_limits(&tx)?
                    .get(work.mint_id.as_str())
                    .copied()
                    .unwrap_or(0)
        {
            return Ok(false);
        }
        tx.execute(
            "INSERT INTO acquisition_schedule(acquisition_id,attempts,next_attempt) VALUES(?,?,?) ON CONFLICT(acquisition_id) DO UPDATE SET attempts=excluded.attempts,next_attempt=excluded.next_attempt",
            params![work.id.to_string(),work.attempts+u32::from(!work.inspection_only),next],
        )?;
        tx.commit()?;
        Ok(true)
    }
}
