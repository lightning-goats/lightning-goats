//! Deterministic denomination-split accounting, not permission to submit a swap.
//! Durable proof claims, original CDK recovery and cost-aware melt execution must
//! consume this plan before splitting can be enabled in the runtime.
use anyhow::{Result, ensure};
use lightning_goats_cashu_receipt::treasury::{Intent, MAX_SOURCES, hex_digest};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use uuid::Uuid;

#[derive(Clone, Copy, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Limits {
    /// Bounds the entire reshaped proof value, including unassigned remainder.
    pub maximum_input_sats: u64,
    pub maximum_fee_sats: u64,
    pub maximum_outputs: usize,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Purpose {
    Treasury,
    Remainder,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Output {
    pub acquisition_id: Uuid,
    pub amount: u64,
    pub purpose: Purpose,
}
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Plan {
    version: u8,
    intent_digest: String,
    owned_inputs: BTreeMap<Uuid, u64>,
    fee_sats: u64,
    denominations: Vec<u64>,
    outputs: Vec<Output>,
}
impl Plan {
    /// `owned_inputs` must subsequently be bound to the exact, globally held
    /// input proofs by the journal/gate. Arithmetic does not prove custody.
    pub fn build(
        intent: &Intent,
        owned_inputs: BTreeMap<Uuid, u64>,
        fee_sats: u64,
        denominations: Vec<u64>,
        limits: Limits,
    ) -> Result<Self> {
        intent.validate()?;
        ensure!(
            limits.maximum_input_sats > 0
                && limits.maximum_input_sats <= i64::MAX as u64
                && limits.maximum_fee_sats <= i64::MAX as u64
                && (1..=1024).contains(&limits.maximum_outputs),
            "invalid split limits"
        );
        ensure!(
            !owned_inputs.is_empty()
                && owned_inputs.len() <= MAX_SOURCES
                && owned_inputs
                    .iter()
                    .all(|(id, amount)| !id.is_nil() && *amount > 0),
            "invalid split source inventory"
        );
        let gross = owned_inputs
            .values()
            .try_fold(0u64, |sum, value| sum.checked_add(*value))
            .ok_or_else(|| anyhow::anyhow!("split input overflow"))?;
        ensure!(
            gross <= limits.maximum_input_sats,
            "whole split inputs exceed limit"
        );
        ensure!(
            fee_sats <= limits.maximum_fee_sats && fee_sats <= intent.maximum_fee_sats,
            "split fee exceeds original authority"
        );
        ensure!(
            !denominations.is_empty()
                && denominations.len() <= 64
                && denominations
                    .iter()
                    .all(|v| *v > 0 && *v <= i64::MAX as u64)
                && denominations.windows(2).all(|v| v[0] < v[1]),
            "invalid split denominations"
        );
        // Use the original contract's allocation order. Later melt fees extend
        // the same fee allocation; split expenses do not consume a second budget.
        let unspent = intent
            .input_sats
            .checked_sub(intent.deposit_sats)
            .and_then(|v| v.checked_sub(fee_sats))
            .ok_or_else(|| anyhow::anyhow!("unaffordable split"))?;
        let allocated = intent.allocate_result(fee_sats, unspent)?;
        let mut remaining = owned_inputs.clone();
        let mut outputs = Vec::new();
        for (source, cost) in intent.sources.iter().zip(allocated) {
            let owned = remaining
                .get_mut(&source.acquisition_id)
                .ok_or_else(|| anyhow::anyhow!("split source missing"))?;
            *owned = owned
                .checked_sub(source.input_sats)
                .ok_or_else(|| anyhow::anyhow!("split source underfunded"))?;
            append(
                &mut outputs,
                source.acquisition_id,
                source.input_sats - cost.fee_sats,
                Purpose::Treasury,
                &denominations,
                limits.maximum_outputs,
            )?;
        }
        for (owner, value) in remaining {
            append(
                &mut outputs,
                owner,
                value,
                Purpose::Remainder,
                &denominations,
                limits.maximum_outputs,
            )?;
        }
        outputs.sort_by_key(|o| (o.amount, o.acquisition_id, o.purpose));
        let output_total = outputs
            .iter()
            .try_fold(0u64, |sum, value| sum.checked_add(value.amount))
            .ok_or_else(|| anyhow::anyhow!("split output overflow"))?;
        ensure!(
            output_total.checked_add(fee_sats) == Some(gross),
            "split value not conserved"
        );
        Ok(Self {
            version: 1,
            intent_digest: hex_digest(intent.digest()?),
            owned_inputs,
            fee_sats,
            denominations,
            outputs,
        })
    }
    pub fn validate_for(&self, intent: &Intent, limits: Limits) -> Result<()> {
        ensure!(
            *self
                == Self::build(
                    intent,
                    self.owned_inputs.clone(),
                    self.fee_sats,
                    self.denominations.clone(),
                    limits
                )?,
            "split plan changed"
        );
        Ok(())
    }
    pub fn outputs(&self) -> &[Output] {
        &self.outputs
    }
    pub fn fee_sats(&self) -> u64 {
        self.fee_sats
    }
}

/// CDK 0.18.3 uses greedy largest-denomination splitting. Bound the allocation
/// before expanding counts; calling its splitter on huge single-sat amounts
/// would allocate before the application's proof-count check.
fn append(
    outputs: &mut Vec<Output>,
    owner: Uuid,
    mut amount: u64,
    purpose: Purpose,
    denominations: &[u64],
    maximum: usize,
) -> Result<()> {
    for denomination in denominations.iter().rev().copied() {
        let count = amount / denomination;
        ensure!(
            count <= u64::try_from(maximum - outputs.len())?,
            "split output count exceeds limit"
        );
        for _ in 0..count {
            outputs.push(Output {
                acquisition_id: owner,
                amount: denomination,
                purpose,
            });
        }
        amount %= denomination;
    }
    ensure!(amount == 0, "source amount cannot be represented exactly");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use lightning_goats_cashu_receipt::treasury::{Destination, PURPOSE, Source};
    fn intent() -> Intent {
        Intent {
            version: 1,
            purpose: PURPOSE.into(),
            id: Uuid::from_u128(1),
            core_epoch: Uuid::from_u128(2),
            wallet_instance: Uuid::from_u128(3),
            wallet_epoch: Uuid::from_u128(4),
            mint_id: "fixture".into(),
            treasury_policy: "v1".into(),
            destination: Destination {
                origin: "https://api.example.com/".into(),
                account_id: Uuid::from_u128(5),
                credential_fingerprint: "ab".repeat(32),
                network: "bitcoin".into(),
            },
            input_sats: 10,
            deposit_sats: 7,
            maximum_fee_sats: 3,
            invoice_expiry_seconds: 300,
            sources: vec![
                Source {
                    ledger_id: 1,
                    acquisition_id: Uuid::from_u128(11),
                    receipt_digest: "cd".repeat(32),
                    input_sats: 6,
                },
                Source {
                    ledger_id: 2,
                    acquisition_id: Uuid::from_u128(12),
                    receipt_digest: "ef".repeat(32),
                    input_sats: 4,
                },
            ],
        }
    }
    fn limits() -> Limits {
        Limits {
            maximum_input_sats: 32,
            maximum_fee_sats: 3,
            maximum_outputs: 64,
        }
    }
    fn owned() -> BTreeMap<Uuid, u64> {
        BTreeMap::from([
            (Uuid::from_u128(11), 8),
            (Uuid::from_u128(12), 8),
            (Uuid::from_u128(13), 4),
        ])
    }
    fn amounts(plan: &Plan, purpose: Purpose) -> BTreeMap<Uuid, u64> {
        let mut result = BTreeMap::new();
        for output in plan.outputs().iter().filter(|o| o.purpose == purpose) {
            *result.entry(output.acquisition_id).or_default() += output.amount;
        }
        result
    }
    #[test]
    fn fees_use_original_sources_and_unassigned_remainder_is_preserved() -> Result<()> {
        let original = intent();
        let plan = Plan::build(&original, owned(), 1, vec![1, 2, 4, 8], limits())?;
        assert_eq!(
            amounts(&plan, Purpose::Treasury),
            BTreeMap::from([(Uuid::from_u128(11), 6), (Uuid::from_u128(12), 3)])
        );
        assert_eq!(
            amounts(&plan, Purpose::Remainder),
            BTreeMap::from([
                (Uuid::from_u128(11), 2),
                (Uuid::from_u128(12), 4),
                (Uuid::from_u128(13), 4)
            ])
        );
        assert_eq!(plan.fee_sats(), 1);
        let bytes = serde_json::to_vec(&plan)?;
        serde_json::from_slice::<Plan>(&bytes)?.validate_for(&original, limits())?;
        let mut changed: serde_json::Value = serde_json::from_slice(&bytes)?;
        changed["outputs"][0]["acquisition_id"] = serde_json::json!(Uuid::from_u128(99));
        assert!(
            serde_json::from_value::<Plan>(changed)?
                .validate_for(&original, limits())
                .is_err()
        );
        Ok(())
    }
    #[test]
    fn bounds_apply_to_whole_proofs_and_rounding_never_invents_value() -> Result<()> {
        let original = intent();
        for denominations in [vec![0], vec![2, 1], vec![1, 1], vec![4, 8], vec![]] {
            assert!(Plan::build(&original, owned(), 1, denominations, limits()).is_err());
        }
        assert!(Plan::build(&original, owned(), 4, vec![1, 2, 4, 8], limits()).is_err());
        for limits in [
            Limits {
                maximum_input_sats: 19,
                ..limits()
            },
            Limits {
                maximum_outputs: 1,
                ..limits()
            },
            Limits {
                maximum_fee_sats: 0,
                ..limits()
            },
        ] {
            assert!(Plan::build(&original, owned(), 1, vec![1, 2, 4, 8], limits).is_err());
        }
        let mut huge = owned();
        huge.remove(&Uuid::from_u128(13));
        huge.insert(Uuid::from_u128(11), i64::MAX as u64 - 8);
        assert!(
            Plan::build(
                &original,
                huge,
                1,
                vec![1],
                Limits {
                    maximum_input_sats: i64::MAX as u64,
                    ..limits()
                }
            )
            .is_err()
        );
        let mut short = owned();
        short.insert(Uuid::from_u128(11), 5);
        assert!(Plan::build(&original, short, 1, vec![1, 2, 4, 8], limits()).is_err());
        Ok(())
    }
    #[test]
    fn every_allowed_fee_conserves_each_source_and_retains_melt_fee_headroom() -> Result<()> {
        let original = intent();
        for fee in 0..=original.maximum_fee_sats {
            for extra in 0..=8 {
                let inventory = BTreeMap::from([
                    (Uuid::from_u128(11), 6 + extra),
                    (Uuid::from_u128(12), 4 + extra),
                ]);
                let plan = Plan::build(
                    &original,
                    inventory.clone(),
                    fee,
                    vec![1, 2, 4, 8, 16],
                    limits(),
                )?;
                let treasury = amounts(&plan, Purpose::Treasury);
                let remainder = amounts(&plan, Purpose::Remainder);
                let allocated = original
                    .allocate_result(fee, original.input_sats - original.deposit_sats - fee)?;
                for (source, cost) in original.sources.iter().zip(allocated) {
                    assert_eq!(
                        treasury.get(&source.acquisition_id).copied().unwrap_or(0)
                            + remainder.get(&source.acquisition_id).copied().unwrap_or(0)
                            + cost.fee_sats,
                        inventory[&source.acquisition_id]
                    );
                }
                assert_eq!(treasury.values().sum::<u64>(), original.input_sats - fee);
                assert!(treasury.values().sum::<u64>() >= original.deposit_sats);
                plan.validate_for(&original, limits())?;
            }
        }
        Ok(())
    }
}
