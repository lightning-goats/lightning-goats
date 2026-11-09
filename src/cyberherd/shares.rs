//! Exact contribution weights and six-decimal largest-remainder allocations.
use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};

pub const MICRO_PERCENT: u64 = 1_000_000;
pub const FULL_PERCENT: u64 = 100 * MICRO_PERCENT;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SocialAwards {
    pub repost: bool,
    pub like: bool,
}

impl SocialAwards {
    #[must_use]
    pub fn points(self) -> u64 {
        3 * u64::from(self.repost) + u64::from(self.like)
    }

    pub fn include(&mut self, other: Self) {
        self.repost |= other.repost;
        self.like |= other.like;
    }
}

pub fn point_units(amount_sats: u64, awards: SocialAwards, minimum_sats: u64) -> Result<u128> {
    if amount_sats > i64::MAX as u64 || minimum_sats > i64::MAX as u64 {
        bail!("CyberHerd amount exceeds the durable integer range");
    }
    Ok(
        10 * u128::from(amount_sats)
            + u128::from(minimum_sats.max(1)) * u128::from(awards.points()),
    )
}

/// Inputs are eligible active recipients only. Stable pubkey order breaks ties.
/// No floats and no fixed percentage/slot reservation are involved.
pub fn proportional_shares(
    weights: &[(String, u128)],
    member_percent: u8,
) -> Result<BTreeMap<String, u64>> {
    if member_percent > 100 {
        bail!("member allocation must be between 0 and 100 percent");
    }
    if weights.len() > 1000 {
        bail!("CyberHerd allocation exceeds the supported roster bound");
    }
    let mut seen = BTreeSet::new();
    for (key, _) in weights {
        if !seen.insert(key) {
            bail!("duplicate member in allocation");
        }
    }
    let total = weights.iter().try_fold(0u128, |sum, (_, weight)| {
        sum.checked_add(*weight)
            .ok_or_else(|| anyhow::anyhow!("CyberHerd point total overflow"))
    })?;
    let pool = u64::from(member_percent) * MICRO_PERCENT;
    let mut shares: BTreeMap<String, u64> =
        weights.iter().map(|(key, _)| (key.clone(), 0)).collect();
    if total == 0 || pool == 0 {
        return Ok(shares);
    }
    let mut fractions = Vec::new();
    let mut allocated = 0u64;
    for (key, weight) in weights.iter().filter(|(_, weight)| *weight > 0) {
        let numerator = u128::from(pool)
            .checked_mul(*weight)
            .ok_or_else(|| anyhow::anyhow!("CyberHerd share numerator overflow"))?;
        let floor = u64::try_from(numerator / total)?;
        allocated = allocated
            .checked_add(floor)
            .ok_or_else(|| anyhow::anyhow!("CyberHerd share sum overflow"))?;
        shares.insert(key.clone(), floor);
        fractions.push((key, numerator % total));
    }
    let remainder = usize::try_from(
        pool.checked_sub(allocated)
            .ok_or_else(|| anyhow::anyhow!("CyberHerd allocation exceeds the member pool"))?,
    )?;
    if remainder > fractions.len() {
        bail!("invalid CyberHerd largest-remainder allocation");
    }
    fractions.sort_by(|(a_key, a_fraction), (b_key, b_fraction)| {
        b_fraction.cmp(a_fraction).then_with(|| a_key.cmp(b_key))
    });
    for (key, _) in fractions.into_iter().take(remainder) {
        *shares.get_mut(key).expect("allocated member exists") += 1;
    }
    Ok(shares)
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn reference_zap_repost_like_split_and_stable_ties() {
        let weights = vec![
            ("zap".into(), 100),
            ("repost".into(), 30),
            ("like".into(), 10),
        ];
        let shares = proportional_shares(&weights, 10).unwrap();
        assert_eq!(shares["zap"], 7_142_857);
        assert_eq!(shares["repost"], 2_142_857);
        assert_eq!(shares["like"], 714_286);
        let a =
            proportional_shares(&[("b".into(), 1), ("a".into(), 1), ("c".into(), 1)], 10).unwrap();
        assert_eq!(a["a"], 3_333_334);
        assert_eq!(a["b"], 3_333_333);
        assert_eq!(a["c"], 3_333_333);
    }

    #[test]
    fn points_scale_with_minimum_and_social_points_add() {
        let both = SocialAwards {
            repost: true,
            like: true,
        };
        assert_eq!(point_units(20, both, 10).unwrap(), 240);
        assert_eq!(point_units(20, both, 20).unwrap(), 280);
        assert_eq!(point_units(1, SocialAwards::default(), 0).unwrap(), 10);
        assert_eq!(point_units(0, both, 10).unwrap(), 40);
        assert!(point_units(u64::MAX, both, 10).is_err());
    }

    #[test]
    fn zero_pools_and_zero_weights_never_reserve_a_slot_share() {
        assert_eq!(proportional_shares(&[("a".into(), 1)], 0).unwrap()["a"], 0);
        assert_eq!(proportional_shares(&[("a".into(), 0)], 10).unwrap()["a"], 0);
        assert!(proportional_shares(&[("a".into(), 1), ("a".into(), 2)], 10).is_err());
        assert!(proportional_shares(&[("a".into(), u128::MAX)], 100).is_err());
    }

    proptest! {
        #[test]
        fn exact_pool_conservation_and_order_independence(
            amounts in prop::collection::vec(0u64..1_000_000_000, 1..25),
            pool in 0u8..=100,
        ) {
            let weights: Vec<_> = amounts.iter().enumerate().map(|(i, n)| (format!("{i:064x}"),u128::from(*n))).collect();
            let a = proportional_shares(&weights,pool).unwrap();
            let mut reversed = weights.clone();reversed.reverse();
            prop_assert_eq!(&a,&proportional_shares(&reversed,pool).unwrap());
            let expected = if amounts.iter().any(|n| *n>0) {u64::from(pool)*MICRO_PERCENT} else {0};
            prop_assert_eq!(a.values().sum::<u64>(),expected);
            for (key,weight) in weights {if weight==0 {prop_assert_eq!(a[&key],0);}}
        }
    }
}
