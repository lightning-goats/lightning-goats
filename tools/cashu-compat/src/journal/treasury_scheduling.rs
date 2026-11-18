//! Persisted stage claims and independent wallet execution admission budgets.
use super::*;
use crate::{treasury::TreasuryBinding, treasury_scheduler::Execution};
use anyhow::Context;

// Share the current stage and its schedule with the read-only operator status.
pub(crate) const SCHEDULED_TREASURY_WORK: &str = "WITH work AS (
    SELECT i.id,i.rowid AS ordering,i.state,json_extract(i.issued_json,'$.intent.mint_id') AS mint_id,
        json_extract(i.issued_json,'$.intent.maximum_fee_sats') AS maximum_fee_sats,
        CASE
          WHEN i.state='reconciled' THEN 'cleanup'
          WHEN i.state='submitted' OR EXISTS(SELECT 1 FROM treasury_melt_requests r WHERE r.intent_id=i.id) THEN 'reconcile'
          WHEN i.state='prepared' THEN 'submit'
          WHEN EXISTS(SELECT 1 FROM treasury_preparation_claims c WHERE c.intent_id=i.id)
            THEN CASE WHEN ?1 THEN 'resume_preparation' ELSE 'recover_preparation' END
          WHEN EXISTS(SELECT 1 FROM treasury_split_results r WHERE r.intent_id=i.id)
            AND NOT EXISTS(SELECT 1 FROM treasury_proof_selections p WHERE p.intent_id=i.id) THEN 'select_split'
          WHEN NOT EXISTS(SELECT 1 FROM treasury_split_results r WHERE r.intent_id=i.id)
            AND (EXISTS(SELECT 1 FROM treasury_split_execution_claims c WHERE c.intent_id=i.id)
              OR EXISTS(SELECT 1 FROM treasury_split_requests r WHERE r.intent_id=i.id)) THEN 'recover_split'
          WHEN EXISTS(SELECT 1 FROM treasury_quotes q WHERE q.intent_id=i.id)
            AND EXISTS(SELECT 1 FROM treasury_split_reservations r WHERE r.intent_id=i.id)
            AND NOT EXISTS(SELECT 1 FROM treasury_split_results r WHERE r.intent_id=i.id) THEN 'split'
          WHEN EXISTS(SELECT 1 FROM treasury_quotes q WHERE q.intent_id=i.id) THEN 'prepare'
          WHEN EXISTS(SELECT 1 FROM treasury_quote_requests q WHERE q.intent_id=i.id) THEN 'recover_quote'
          WHEN EXISTS(SELECT 1 FROM treasury_proof_selections p WHERE p.intent_id=i.id)
            OR EXISTS(SELECT 1 FROM treasury_split_reservations r WHERE r.intent_id=i.id) THEN 'quote'
          ELSE 'reserve'
        END AS action
    FROM treasury_instructions i
    WHERE i.state!='reconciled' OR NOT EXISTS(SELECT 1 FROM treasury_cdk_cleanup c WHERE c.intent_id=i.id)
), scheduled AS (
    SELECT w.*,COALESCE(s.attempts,0) AS attempts,s.next_attempt,
        (w.action='cleanup' OR (w.action='reconcile' AND w.state='submitted')
        OR (w.action='recover_split' AND EXISTS(
            SELECT 1 FROM treasury_split_submissions x WHERE x.intent_id=w.id
        ))) AS continue_recovery
    FROM work w LEFT JOIN treasury_work_schedule s ON s.intent_id=w.id AND s.action=w.action
)";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Reserve,
    Split,
    RecoverSplit,
    SelectSplit,
    Quote,
    RecoverQuote,
    Prepare,
    RecoverPreparation,
    ResumePreparation,
    Submit,
    Reconcile,
    Cleanup,
}
impl Action {
    fn parse(value: &str) -> Result<Self> {
        Ok(match value {
            "reserve" => Self::Reserve,
            "split" => Self::Split,
            "recover_split" => Self::RecoverSplit,
            "select_split" => Self::SelectSplit,
            "quote" => Self::Quote,
            "recover_quote" => Self::RecoverQuote,
            "prepare" => Self::Prepare,
            "recover_preparation" => Self::RecoverPreparation,
            "resume_preparation" => Self::ResumePreparation,
            "submit" => Self::Submit,
            "reconcile" => Self::Reconcile,
            "cleanup" => Self::Cleanup,
            _ => bail!("invalid treasury scheduling action"),
        })
    }
    pub fn executes(self) -> bool {
        matches!(
            self,
            Self::Reserve
                | Self::Split
                | Self::SelectSplit
                | Self::Quote
                | Self::Prepare
                | Self::Submit
        )
    }
}
pub struct Work {
    pub id: Uuid,
    pub mint_id: String,
    pub action: Action,
}

impl Journal {
    /// The durable stage, not the last RPC or process result, chooses the next action.
    pub(crate) fn claim_treasury_work(
        &mut self,
        now: i64,
        policy: &Execution,
        deadline: u64,
    ) -> Result<Option<Work>> {
        policy.validate()?;
        ensure!(
            now >= 0 && (1..=60).contains(&deadline),
            "invalid treasury scheduling time/deadline"
        );
        if !(policy.execute || policy.reconcile) {
            return Ok(None);
        }
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let fee_remaining = match policy.fee_sats_per_24h {
            Some(limit) if policy.execute => Some(limit.saturating_sub(daily_fees(&tx, now)?)),
            _ => None,
        };
        let row: Option<(String, String, String, u32, bool)> = tx.query_row(
            &format!(
                "{SCHEDULED_TREASURY_WORK}
                 SELECT w.id,w.mint_id,w.action,w.attempts,w.continue_recovery
                 FROM scheduled w
                 WHERE ((?1 AND w.action IN ('reserve','quote','prepare','submit','select_split'))
                     OR (?1 AND ?2 AND w.action='split')
                     OR (?3 AND w.action IN ('recover_quote','recover_preparation','resume_preparation','recover_split','reconcile','cleanup')))
                   AND (w.action IN ('recover_quote','recover_preparation','resume_preparation','recover_split','reconcile','cleanup')
                     OR EXISTS(SELECT 1 FROM treasury_execution_admissions a WHERE a.intent_id=w.id)
                     OR w.maximum_fee_sats<=?6)
                   AND (w.attempts<?4 OR w.continue_recovery) AND COALESCE(w.next_attempt,0)<=?5
                 ORDER BY COALESCE(w.next_attempt,0),w.ordering LIMIT 1"
            ),
            params![policy.execute,policy.split.enabled,policy.reconcile,policy.maximum_attempts,now,fee_remaining],
            |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?)),
        ).optional()?;
        let Some((id, mint_id, action, attempts, continue_recovery)) = row else {
            return Ok(None);
        };
        let work = Work {
            id: id.parse()?,
            mint_id,
            action: Action::parse(&action)?,
        };
        let next_attempts = attempts.saturating_add(1);
        let minimum_delay = if continue_recovery && next_attempts >= policy.maximum_attempts {
            300
        } else {
            5
        };
        let delay = (5_i64 << attempts.min(10)).clamp(minimum_delay, 3600);
        let next = now
            .checked_add(i64::try_from(deadline)?)
            .and_then(|v| v.checked_add(delay))
            .ok_or_else(|| anyhow::anyhow!("treasury schedule overflow"))?;
        tx.execute("INSERT INTO treasury_work_schedule(intent_id,action,attempts,next_attempt) VALUES(?,?,?,?) ON CONFLICT(intent_id,action) DO UPDATE SET attempts=excluded.attempts,next_attempt=excluded.next_attempt",
            params![id,action,next_attempts,next])?;
        tx.commit()?;
        Ok(Some(work))
    }

    /// Admit each immutable intent once, conservatively charging its full input.
    /// Failure/timeout never refunds admission. Reconciliation is separately allowed.
    pub(crate) fn authorize_treasury_execution(
        &mut self,
        binding: &TreasuryBinding,
        id: Uuid,
        policy: &Execution,
        now: i64,
    ) -> Result<()> {
        policy.validate()?;
        ensure!(policy.execute && now >= 0, "treasury execution disabled");
        let issued = self.treasury_instruction(binding, id)?;
        binding.validate_new_invoice(&issued, u64::try_from(now)?)?;
        ensure!(
            issued.intent.input_sats <= policy.maximum_input_sats
                && issued.intent.maximum_fee_sats <= policy.maximum_fee_sats,
            "treasury intent exceeds wallet execution limits"
        );
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let latest: i64 = tx.query_row(
            "SELECT MAX(COALESCE((SELECT MAX(admitted_at) FROM treasury_execution_admissions),0),
                        COALESCE((SELECT MAX(completed_at) FROM treasury_fee_completions),0))",
            [],
            |r| r.get(0),
        )?;
        ensure!(now >= latest, "treasury execution clock rolled back");
        let existing: Option<u64> = tx
            .query_row(
                "SELECT input_sats FROM treasury_execution_admissions WHERE intent_id=?",
                [id.to_string()],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(input) = existing {
            ensure!(
                input == issued.intent.input_sats,
                "treasury execution admission changed"
            );
            tx.commit()?;
            return Ok(());
        }
        let (count,total): (u64,u64) = tx.query_row("SELECT COUNT(*),COALESCE(SUM(input_sats),0) FROM treasury_execution_admissions WHERE admitted_at>?", [now.saturating_sub(86400)], |r| Ok((r.get(0)?,r.get(1)?)))?;
        let in_flight: u64 = tx.query_row("SELECT COALESCE(SUM(a.input_sats),0) FROM treasury_execution_admissions a JOIN treasury_instructions i ON i.id=a.intent_id WHERE i.state!='reconciled'", [], |r| r.get(0))?;
        let total_fee = daily_fees(&tx, now)?;
        let fee_limit = policy
            .fee_sats_per_24h
            .context("wallet treasury daily fee limit missing")?;
        ensure!(
            (policy.unlimited_operations_per_24h || count < policy.operations_per_24h)
                && (policy.unlimited_input_sats_per_24h
                    || total
                        .checked_add(issued.intent.input_sats)
                        .is_some_and(|v| v <= policy.input_sats_per_24h))
                && in_flight
                    .checked_add(issued.intent.input_sats)
                    .is_some_and(|v| v <= policy.maximum_in_flight_sats),
            "treasury wallet admission budget exhausted"
        );
        ensure!(
            issued.intent.maximum_fee_sats <= fee_limit.saturating_sub(total_fee),
            "wallet treasury daily fee budget exhausted"
        );
        tx.execute("INSERT INTO treasury_execution_admissions(intent_id,admitted_at,input_sats) VALUES(?,?,?)",params![id.to_string(),now,issued.intent.input_sats])?;
        tx.commit()?;
        Ok(())
    }
}

pub(crate) fn daily_fees(db: &Connection, now: i64) -> Result<u64> {
    Ok(db.query_row(
        "SELECT COALESCE(SUM(COALESCE(c.fee_sats,json_extract(i.issued_json,'$.intent.maximum_fee_sats'))),0)
         FROM treasury_instructions i LEFT JOIN treasury_fee_completions c ON c.intent_id=i.id
         WHERE (c.intent_id IS NOT NULL
             OR EXISTS(SELECT 1 FROM treasury_execution_admissions a WHERE a.intent_id=i.id)
             OR EXISTS(SELECT 1 FROM treasury_melt_requests m WHERE m.intent_id=i.id)
             OR EXISTS(SELECT 1 FROM treasury_split_requests s WHERE s.intent_id=i.id))
           AND (c.intent_id IS NULL OR c.completed_at>=?)",
        [now.saturating_sub(86_400)],
        |r| r.get(0),
    )?)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn policy() -> Execution {
        Execution {
            split: Default::default(),
            execute: true,
            reconcile: true,
            maximum_attempts: 2,
            maximum_input_sats: 101,
            maximum_fee_sats: 1,
            operations_per_24h: 1,
            unlimited_operations_per_24h: false,
            input_sats_per_24h: 101,
            unlimited_input_sats_per_24h: false,
            fee_sats_per_24h: Some(2),
            maximum_in_flight_sats: 101,
        }
    }
    fn second(
        first: &lightning_goats_cashu_receipt::treasury::Issued,
        time: u64,
    ) -> lightning_goats_cashu_receipt::treasury::Issued {
        let mut issued = first.clone();
        issued.intent.id = Uuid::from_u128(55);
        issued.receive_request_id = Uuid::from_u128(56);
        issued.payment_hash = "cd".repeat(32);
        issued.invoice = crate::treasury::tests::signed_invoice_hash(
            cdk::lightning_invoice::Currency::Bitcoin,
            100_000,
            time,
            300,
            "cd",
        );
        issued
    }
    #[test]
    fn admission_budgets_survive_reopen_clock_rollback_and_unresolved_day_boundary() -> Result<()> {
        let (binding, issued) = crate::treasury::tests::fixture();
        let dir = crate::test_directory()?;
        let path = dir.path().join("budgets.sqlite");
        let mut journal = Journal::open(&path, binding.wallet_epoch, 8)?;
        journal.bind_treasury_instruction(&binding, &issued, 1000)?;
        let second = second(&issued, 1000);
        journal.bind_treasury_instruction(&binding, &second, 1000)?;
        journal.authorize_treasury_execution(&binding, issued.intent.id, &policy(), 1002)?;
        drop(journal);
        let mut journal = Journal::open(&path, binding.wallet_epoch, 8)?;
        journal.authorize_treasury_execution(&binding, issued.intent.id, &policy(), 1003)?;
        assert!(
            journal
                .authorize_treasury_execution(&binding, issued.intent.id, &policy(), 1001)
                .is_err()
        );
        for constraint in 0..4 {
            let mut limit = policy();
            limit.operations_per_24h = 2;
            limit.input_sats_per_24h = 202;
            limit.maximum_in_flight_sats = 202;
            match constraint {
                0 => limit.operations_per_24h = 1,
                1 => limit.input_sats_per_24h = 101,
                2 => limit.maximum_in_flight_sats = 101,
                _ => limit.fee_sats_per_24h = Some(1),
            }
            assert!(
                journal
                    .authorize_treasury_execution(&binding, second.intent.id, &limit, 1003)
                    .is_err()
            );
        }
        let count: u64 = journal.connection.query_row(
            "SELECT COUNT(*) FROM treasury_execution_admissions",
            [],
            |r| r.get(0),
        )?;
        assert_eq!(count, 1);
        let mut limited = policy();
        limited.maximum_input_sats = 100;
        assert!(
            journal
                .authorize_treasury_execution(&binding, issued.intent.id, &limited, 1003)
                .is_err()
        );
        limited = policy();
        limited.maximum_fee_sats = 0;
        assert!(
            journal
                .authorize_treasury_execution(&binding, issued.intent.id, &limited, 1003)
                .is_err()
        );
        // A new daily window does not release an unresolved original input reservation.
        let tomorrow = 1002 + 86400;
        let mut fresh = second.clone();
        fresh.intent.id = Uuid::from_u128(57);
        fresh.receive_request_id = Uuid::from_u128(58);
        fresh.payment_hash = "ef".repeat(32);
        fresh.invoice = crate::treasury::tests::signed_invoice_hash(
            cdk::lightning_invoice::Currency::Bitcoin,
            100_000,
            tomorrow,
            300,
            "ef",
        );
        journal.bind_treasury_instruction(&binding, &fresh, tomorrow)?;
        assert!(
            journal
                .authorize_treasury_execution(&binding, fresh.intent.id, &policy(), tomorrow as i64)
                .is_err()
        );
        let mut raised = policy();
        raised.maximum_in_flight_sats = 202;
        raised.fee_sats_per_24h = Some(1);
        assert_eq!(daily_fees(&journal.connection, tomorrow as i64 + 1)?, 1);
        assert!(
            journal
                .authorize_treasury_execution(&binding, fresh.intent.id, &raised, tomorrow as i64)
                .unwrap_err()
                .to_string()
                .contains("daily fee budget exhausted")
        );
        raised.fee_sats_per_24h = Some(2);
        journal.authorize_treasury_execution(
            &binding,
            fresh.intent.id,
            &raised,
            tomorrow as i64,
        )?;
        Ok(())
    }

    #[test]
    fn fee_pressure_leaves_fresh_treasury_work_unclaimed() -> Result<()> {
        let (binding, issued) = crate::treasury::tests::fixture();
        let dir = crate::test_directory()?;
        let mut journal = Journal::open(&dir.path().join("fees.sqlite"), binding.wallet_epoch, 8)?;
        let other = second(&issued, 1000);
        for instruction in [&issued, &other] {
            journal.bind_treasury_instruction(&binding, instruction, 1000)?;
        }
        let mut cap = policy();
        cap.operations_per_24h = 0;
        cap.unlimited_operations_per_24h = true;
        cap.input_sats_per_24h = 202;
        cap.maximum_in_flight_sats = 202;
        cap.fee_sats_per_24h = None;
        assert!(journal.claim_treasury_work(1000, &cap, 2)?.is_none());
        cap.fee_sats_per_24h = Some(1);
        journal.authorize_treasury_execution(&binding, issued.intent.id, &cap, 1000)?;
        assert_eq!(
            journal.claim_treasury_work(1000, &cap, 2)?.unwrap().id,
            issued.intent.id
        );
        assert!(journal.claim_treasury_work(1000, &cap, 2)?.is_none());
        let attempts: u64 = journal.connection.query_row(
            "SELECT COUNT(*) FROM treasury_work_schedule WHERE intent_id=?",
            [other.intent.id.to_string()],
            |r| r.get(0),
        )?;
        assert_eq!(attempts, 0);
        Ok(())
    }
    #[test]
    fn durable_claims_are_fair_bounded_and_overflow_is_atomic() -> Result<()> {
        let (binding, issued) = crate::treasury::tests::fixture();
        let dir = crate::test_directory()?;
        let path = dir.path().join("schedule.sqlite");
        let mut journal = Journal::open(&path, binding.wallet_epoch, 8)?;
        journal.bind_treasury_instruction(&binding, &issued, 1000)?;
        let second = second(&issued, 1000);
        journal.bind_treasury_instruction(&binding, &second, 1000)?;
        assert!(journal.claim_treasury_work(i64::MAX, &policy(), 2).is_err());
        assert!(
            journal
                .claim_treasury_work(1000, &Execution::default(), 2)?
                .is_none()
        );
        let work = journal.claim_treasury_work(1000, &policy(), 2)?.unwrap();
        assert_eq!(work.id, issued.intent.id);
        assert_eq!(work.action, Action::Reserve);
        assert_eq!(
            journal.claim_treasury_work(1000, &policy(), 2)?.unwrap().id,
            second.intent.id
        );
        drop(journal);
        let mut journal = Journal::open(&path, binding.wallet_epoch, 8)?;
        assert!(journal.claim_treasury_work(1006, &policy(), 2)?.is_none());
        assert_eq!(
            journal.claim_treasury_work(1007, &policy(), 2)?.unwrap().id,
            issued.intent.id
        );
        assert_eq!(
            journal.claim_treasury_work(1007, &policy(), 2)?.unwrap().id,
            second.intent.id
        );
        assert!(journal.claim_treasury_work(10000, &policy(), 2)?.is_none());
        Ok(())
    }
    #[test]
    fn quote_and_melt_claims_choose_recovery_without_fresh_execution() -> Result<()> {
        let (binding, issued) = crate::treasury::tests::fixture();
        let dir = crate::test_directory()?;
        let mut journal =
            Journal::open(&dir.path().join("stages.sqlite"), binding.wallet_epoch, 8)?;
        journal.bind_treasury_instruction(&binding, &issued, 1000)?;
        let mut paused = policy();
        paused.execute = false;
        assert!(journal.claim_treasury_work(1000, &paused, 2)?.is_none());
        journal.connection.execute(
            "INSERT INTO treasury_proof_selections(intent_id,selection) VALUES(?,?)",
            params![issued.intent.id.to_string(), b"{}".as_slice()],
        )?;
        assert_eq!(
            journal
                .claim_treasury_work(1000, &policy(), 2)?
                .unwrap()
                .action,
            Action::Quote
        );
        journal.connection.execute("INSERT INTO treasury_quote_requests(intent_id,destination,request) VALUES(?,'fixture',?)", params![issued.intent.id.to_string(), b"{}".as_slice()])?;
        assert_eq!(
            journal
                .claim_treasury_work(1000, &paused, 2)?
                .unwrap()
                .action,
            Action::RecoverQuote
        );
        journal.connection.execute("INSERT INTO treasury_quotes(intent_id,mint_id,quote_id,quote_json) VALUES(?,?,'fixture',?)", params![issued.intent.id.to_string(), binding.mint_id, b"{}".as_slice()])?;
        assert!(journal.claim_treasury_work(1000, &paused, 2)?.is_none());
        assert_eq!(
            journal
                .claim_treasury_work(1000, &policy(), 2)?
                .unwrap()
                .action,
            Action::Prepare
        );
        journal.connection.execute(
            "INSERT INTO treasury_preparation_claims(intent_id) VALUES(?)",
            [issued.intent.id.to_string()],
        )?;
        assert_eq!(
            journal
                .claim_treasury_work(1000, &paused, 2)?
                .unwrap()
                .action,
            Action::RecoverPreparation
        );
        journal.connection.execute(
            "INSERT INTO treasury_prepared(intent_id,operation_id,prepared_json) VALUES(?,?,?)",
            params![
                issued.intent.id.to_string(),
                Uuid::from_u128(900).to_string(),
                b"{}".as_slice()
            ],
        )?;
        journal.connection.execute(
            "UPDATE treasury_instructions SET state='prepared' WHERE id=?",
            [issued.intent.id.to_string()],
        )?;
        assert!(journal.claim_treasury_work(1000, &paused, 2)?.is_none());
        assert_eq!(
            journal
                .claim_treasury_work(1000, &policy(), 2)?
                .unwrap()
                .action,
            Action::Submit
        );
        journal.connection.execute(
            "INSERT INTO treasury_melt_requests(intent_id,request,derivation) VALUES(?,?,?)",
            params![
                issued.intent.id.to_string(),
                b"{}".as_slice(),
                b"{}".as_slice()
            ],
        )?;
        assert_eq!(
            journal
                .claim_treasury_work(1000, &paused, 2)?
                .unwrap()
                .action,
            Action::Reconcile
        );
        // A frozen request already prevents another submit, even before its claim committed.
        assert!(journal.claim_treasury_work(1000, &policy(), 2)?.is_none());
        assert_eq!(
            journal
                .claim_treasury_work(1007, &paused, 2)?
                .unwrap()
                .action,
            Action::Reconcile
        );
        assert!(journal.claim_treasury_work(10000, &paused, 2)?.is_none());
        journal.connection.execute(
            "UPDATE treasury_instructions SET state='submitted' WHERE id=?",
            [issued.intent.id.to_string()],
        )?;
        assert_eq!(
            journal
                .claim_treasury_work(10000, &paused, 2)?
                .unwrap()
                .action,
            Action::Reconcile
        );
        Ok(())
    }
}
