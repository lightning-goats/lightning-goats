//! Independent whole-proof split exposure. This charge is not permission to POST.
use super::*;

#[derive(Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct AdmissionPolicy {
    pub enabled: bool,
    pub limits: Limits,
    pub operations_per_24h: u64,
    pub unlimited_operations_per_24h: bool,
    pub input_sats_per_24h: u64,
    pub maximum_in_flight_sats: u64,
}
impl AdmissionPolicy {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.operations_per_24h <= 10_000
                && (!self.unlimited_operations_per_24h || self.operations_per_24h == 0)
                && self.limits.maximum_outputs <= 1024,
            "invalid split admission count bounds"
        );
        for value in [
            self.limits.maximum_input_sats,
            self.limits.maximum_fee_sats,
            self.input_sats_per_24h,
            self.maximum_in_flight_sats,
        ] {
            ensure!(
                value <= i64::MAX as u64,
                "invalid split admission amount bound"
            );
        }
        ensure!(
            !self.enabled
                || (self.limits.maximum_input_sats > 0
                    && self.limits.maximum_outputs > 0
                    && (self.operations_per_24h > 0 || self.unlimited_operations_per_24h)
                    && self.input_sats_per_24h > 0
                    && self.maximum_in_flight_sats > 0),
            "split execution requires explicit exposure limits"
        );
        Ok(())
    }
}
impl Journal {
    /// Charge the full frozen proof inventory once, before CDK submission work.
    /// Daily charges never refund. Unresolved exposure remains in-flight across
    /// restarts and day boundaries. Recovery reads do not need this authority.
    pub fn authorize_treasury_split(
        &mut self,
        binding: &TreasuryBinding,
        id: Uuid,
        policy: &AdmissionPolicy,
        now: i64,
    ) -> Result<bool> {
        policy.validate()?;
        ensure!(
            policy.enabled && now >= 0,
            "split execution disabled or invalid time"
        );
        let issued = self.treasury_instruction(binding, id)?;
        binding.validate_new_invoice(&issued, u64::try_from(now)?)?;
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let complete: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM treasury_split_results WHERE intent_id=?)",
            [id.to_string()],
            |r| r.get(0),
        )?;
        ensure!(!complete, "completed split cannot execute again");
        let bytes:Vec<u8>=tx.query_row("SELECT reservation FROM treasury_split_reservations WHERE intent_id=? AND length(reservation)<=1048576",[id.to_string()],|r|r.get(0))?;
        let retained: Reservation = serde_json::from_slice(&bytes)?;
        retained.plan.validate_for(&issued.intent, policy.limits)?;
        let input = retained
            .inputs
            .iter()
            .try_fold(0u64, |sum, p| sum.checked_add(p.amount))
            .ok_or_else(|| anyhow::anyhow!("split exposure overflow"))?;
        ensure!(
            input <= policy.limits.maximum_input_sats,
            "whole split exposure exceeds admission"
        );
        let planned = retained
            .plan
            .outputs()
            .iter()
            .try_fold(retained.request.fee_sats, |sum, output| {
                sum.checked_add(output.amount)
            })
            .ok_or_else(|| anyhow::anyhow!("split plan exposure overflow"))?;
        ensure!(
            input == planned && input > 0,
            "split inventory and plan exposure differ"
        );
        let digest = Sha256::digest(&bytes).to_vec();
        let latest: Option<i64> = tx.query_row(
            "SELECT MAX(admitted_at) FROM treasury_split_admissions",
            [],
            |r| r.get(0),
        )?;
        ensure!(
            latest.is_none_or(|latest| now >= latest),
            "split admission clock rolled back"
        );
        let previous:Option<(u64,u64,Vec<u8>)>=tx.query_row("SELECT input_sats,fee_sats,reservation_digest FROM treasury_split_admissions WHERE intent_id=?",[id.to_string()],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional()?;
        if let Some((previous_input, previous_fee, previous_digest)) = previous {
            ensure!(
                previous_input == input
                    && previous_fee == retained.request.fee_sats
                    && previous_digest == digest,
                "split admission differs from original reservation"
            );
            tx.commit()?;
            return Ok(false);
        }
        let (count,total):(u64,u64)=tx.query_row("SELECT COUNT(*),COALESCE(SUM(input_sats),0) FROM treasury_split_admissions WHERE admitted_at>?",[now.saturating_sub(86400)],|r|Ok((r.get(0)?,r.get(1)?)))?;
        let in_flight:u64=tx.query_row("SELECT COALESCE(SUM(a.input_sats),0) FROM treasury_split_admissions a WHERE NOT EXISTS(SELECT 1 FROM treasury_split_results r WHERE r.intent_id=a.intent_id)",[],|r|r.get(0))?;
        ensure!(
            (policy.unlimited_operations_per_24h || count < policy.operations_per_24h)
                && total
                    .checked_add(input)
                    .is_some_and(|v| v <= policy.input_sats_per_24h)
                && in_flight
                    .checked_add(input)
                    .is_some_and(|v| v <= policy.maximum_in_flight_sats),
            "split exposure budget exhausted"
        );
        tx.execute("INSERT INTO treasury_split_admissions(intent_id,admitted_at,input_sats,fee_sats,reservation_digest) VALUES(?,?,?,?,?)",
            params![id.to_string(),now,input,retained.request.fee_sats,digest])?;
        tx.commit()?;
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::{Fixture, fixture, limits};
    use super::*;
    fn policy() -> AdmissionPolicy {
        AdmissionPolicy {
            enabled: true,
            limits: limits(),
            operations_per_24h: 1,
            unlimited_operations_per_24h: false,
            input_sats_per_24h: 128,
            maximum_in_flight_sats: 128,
        }
    }
    fn second(f: &mut Fixture, time: u64) -> Result<Uuid> {
        let mut issued = f.issued.clone();
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
        let acquisition = Uuid::from_u128(66);
        issued.intent.sources[0].acquisition_id = acquisition;
        issued.intent.sources[0].ledger_id = 2;
        f.journal.connection.execute("INSERT INTO acquisitions(id,mint_id,policy,transport,source_ref,proof_count,gross,state,receive_fee,net,acquired_evidence) VALUES(?,?,'receive-v1','cashu','second-fixture',2,128,'credit_export_pending',0,128,?)",
            params![acquisition.to_string(),f.binding.mint_id,vec![8u8;32]])?;
        f.journal.connection.execute(
            "INSERT INTO acquisition_context(acquisition_id,proofs) VALUES(?,?)",
            params![acquisition.to_string(), b"[]".as_slice()],
        )?;
        let receipt = f.journal.export_receipt(acquisition)?;
        issued.intent.sources[0].receipt_digest =
            lightning_goats_cashu_receipt::treasury::hex_digest(
                crate::receipt_wire::from_private(f.binding.wallet_instance, &receipt)?.digest()?,
            );
        f.journal
            .bind_treasury_instruction(&f.binding, &issued, time)?;
        f.journal
            .reserve_treasury_sources(&f.binding, issued.intent.id, time)?;
        let mut ys = Vec::new();
        for secret in [
            b"second-split-one".as_slice(),
            b"second-split-two".as_slice(),
        ] {
            let y = cdk::dhke::hash_to_curve(secret)?.to_hex();
            f.journal.connection.execute("INSERT INTO owned_output_sources(mint_id,y,c,amount,acquisition_id) VALUES(?,?,?,64,?)",params![f.binding.mint_id,y,y,acquisition.to_string()])?;
            ys.push(y);
        }
        ys.sort();
        let request = Request {
            split_id: Uuid::from_u128(57),
            input_ys: ys,
            ..f.request.clone()
        };
        f.journal
            .reserve_treasury_split(&f.binding, issued.intent.id, &request, limits(), time)?;
        Ok(issued.intent.id)
    }
    #[test]
    fn whole_proof_charge_is_default_off_exactly_once_and_survives_reopen() -> Result<()> {
        let mut f = fixture()?;
        f.journal.reserve_treasury_split(
            &f.binding,
            f.issued.intent.id,
            &f.request,
            limits(),
            1000,
        )?;
        assert!(
            f.journal
                .authorize_treasury_split(
                    &f.binding,
                    f.issued.intent.id,
                    &AdmissionPolicy::default(),
                    1002
                )
                .is_err()
        );
        let mut narrow = policy();
        narrow.limits.maximum_input_sats = 101;
        assert!(
            f.journal
                .authorize_treasury_split(&f.binding, f.issued.intent.id, &narrow, 1002)
                .is_err()
        );
        assert!(f.journal.authorize_treasury_split(
            &f.binding,
            f.issued.intent.id,
            &policy(),
            1002
        )?);
        drop(f.journal);
        f.journal = Journal::open(
            &f.directory.path().join("wallet.sqlite"),
            f.binding.wallet_epoch,
            128,
        )?;
        assert!(!f.journal.authorize_treasury_split(
            &f.binding,
            f.issued.intent.id,
            &policy(),
            1003
        )?);
        assert!(
            f.journal
                .authorize_treasury_split(&f.binding, f.issued.intent.id, &policy(), 1001)
                .is_err()
        );
        let second = second(&mut f, 1000)?;
        for constraint in 0..3 {
            let mut bound = policy();
            bound.operations_per_24h = 2;
            bound.input_sats_per_24h = 256;
            bound.maximum_in_flight_sats = 256;
            match constraint {
                0 => bound.operations_per_24h = 1,
                1 => bound.input_sats_per_24h = 255,
                _ => bound.maximum_in_flight_sats = 255,
            }
            assert!(
                f.journal
                    .authorize_treasury_split(&f.binding, second, &bound, 1003)
                    .is_err()
            );
        }
        let (count, total): (u64, u64) = f.journal.connection.query_row(
            "SELECT COUNT(*),SUM(input_sats) FROM treasury_split_admissions",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        assert_eq!((count, total), (1, 128));
        let mut no_fee = policy();
        no_fee.limits.maximum_fee_sats = 0;
        assert!(
            f.journal
                .authorize_treasury_split(&f.binding, f.issued.intent.id, &no_fee, 1003)
                .is_err()
        );
        let mut open_count = policy();
        open_count.unlimited_operations_per_24h = true;
        assert!(open_count.validate().is_err());
        open_count.operations_per_24h = 0;
        open_count.input_sats_per_24h = 256;
        open_count.maximum_in_flight_sats = 256;
        assert!(
            f.journal
                .authorize_treasury_split(&f.binding, second, &open_count, 1003)?
        );
        Ok(())
    }
    #[test]
    fn unresolved_exposure_does_not_expire_with_daily_budget() -> Result<()> {
        let mut f = fixture()?;
        f.journal.reserve_treasury_split(
            &f.binding,
            f.issued.intent.id,
            &f.request,
            limits(),
            1000,
        )?;
        f.journal
            .authorize_treasury_split(&f.binding, f.issued.intent.id, &policy(), 1002)?;
        let second = second(&mut f, 100000)?;
        // The first admission is outside the rolling day, but its unresolved
        // whole-proof exposure still occupies the in-flight limit.
        assert!(
            f.journal
                .authorize_treasury_split(&f.binding, second, &policy(), 100001)
                .is_err()
        );
        let mut more = policy();
        more.maximum_in_flight_sats = 256;
        assert!(
            f.journal
                .authorize_treasury_split(&f.binding, second, &more, 100001)?
        );
        let total: u64 = f.journal.connection.query_row(
            "SELECT SUM(input_sats) FROM treasury_split_admissions",
            [],
            |r| r.get(0),
        )?;
        assert_eq!(total, 256);
        Ok(())
    }
}
