//! Remaining melt authority after a fully verified, committed split.
use super::*;
use lightning_goats_cashu_receipt::treasury::Intent;

pub(super) struct Budget {
    pub input_sats: u64,
    pub maximum_fee_sats: u64,
    pub sources: BTreeMap<Uuid, u64>,
}

/// Read inside the caller's custody transaction where it mutates reservations.
/// An unresolved split cannot fall back to the original full-input allowance.
pub(super) fn verified(connection: &Connection, intent: &Intent) -> Result<Budget> {
    let splitting: bool = connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM treasury_split_reservations WHERE intent_id=?)",
        [intent.id.to_string()],
        |r| r.get(0),
    )?;
    if !splitting {
        return remaining(intent, 0);
    }
    let (fee,digest,frozen):(u64,Vec<u8>,Vec<u8>)=connection.query_row(
        "SELECT r.fee_sats,r.frozen_digest,q.frozen FROM treasury_split_results r JOIN treasury_split_requests q ON q.intent_id=r.intent_id WHERE r.intent_id=? AND length(q.frozen)<=2097152",
        [intent.id.to_string()],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?)))
        .map_err(|_|anyhow::anyhow!("original split must reconcile before melt proof selection"))?;
    ensure!(
        digest == Sha256::digest(&frozen).to_vec(),
        "split result no longer binds original request"
    );
    remaining(intent, fee)
}
/// Provisional quote bounds only; unresolved splits remain barred from melting.
pub(super) fn for_quote(connection: &Connection, intent: &Intent) -> Result<Budget> {
    remaining(
        intent,
        super::treasury_split::planned_fee(connection, intent)?.unwrap_or(0),
    )
}

fn remaining(intent: &Intent, fee: u64) -> Result<Budget> {
    intent.validate()?;
    let input_sats = intent
        .input_sats
        .checked_sub(fee)
        .ok_or_else(|| anyhow::anyhow!("split exhausted original input"))?;
    let maximum_fee_sats = intent
        .maximum_fee_sats
        .checked_sub(fee)
        .ok_or_else(|| anyhow::anyhow!("split exhausted original fee authority"))?;
    let change = input_sats
        .checked_sub(intent.deposit_sats)
        .ok_or_else(|| anyhow::anyhow!("split consumed deposit authority"))?;
    let costs = intent.allocate_result(fee, change)?;
    let sources = intent
        .sources
        .iter()
        .zip(costs)
        .map(|(source, cost)| (source.acquisition_id, source.input_sats - cost.fee_sats))
        .collect();
    Ok(Budget {
        input_sats,
        maximum_fee_sats,
        sources,
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preparation_fees_share_original_budget_and_can_exhaust_a_source() -> Result<()> {
        let (_, mut issued) = crate::treasury::tests::fixture();
        issued.intent.sources[0].input_sats = 100;
        let mut second = issued.intent.sources[0].clone();
        second.ledger_id = 2;
        second.acquisition_id = Uuid::from_u128(20);
        second.input_sats = 1;
        issued.intent.sources.push(second.clone());
        let original = remaining(&issued.intent, 0)?;
        assert_eq!(original.input_sats, 101);
        assert_eq!(original.maximum_fee_sats, 1);
        let after = remaining(&issued.intent, 1)?;
        assert_eq!(after.input_sats, 100);
        assert_eq!(after.maximum_fee_sats, 0);
        assert_eq!(after.sources[&second.acquisition_id], 0);
        assert_eq!(after.sources.values().sum::<u64>(), 100);
        assert!(remaining(&issued.intent, 2).is_err());
        Ok(())
    }
}
