use anyhow::{Result, ensure};
use rusqlite::{Connection, params};
use serde::Deserialize;
use std::collections::BTreeMap;

#[derive(Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CustodyLimits {
    maximum_total_sats: u64,
    maximum_per_mint_sats: BTreeMap<String, u64>,
}

impl CustodyLimits {
    pub(crate) fn validate(&self, mints: &[crate::mint_policy::MintPolicy]) -> Result<()> {
        ensure!(
            (1..=i64::MAX as u64).contains(&self.maximum_total_sats)
                && !self.maximum_per_mint_sats.is_empty()
                && self.maximum_per_mint_sats.iter().all(|(id, limit)| {
                    (1..=self.maximum_total_sats).contains(limit)
                        && mints.iter().any(|mint| &mint.id == id)
                }),
            "invalid wallet custody limits"
        );
        Ok(())
    }

    pub(crate) fn retains_configured_limits(&self, candidate: &Self) -> bool {
        self.maximum_total_sats == candidate.maximum_total_sats
            && self
                .maximum_per_mint_sats
                .iter()
                .all(|(id, limit)| candidate.maximum_per_mint_sats.get(id) == Some(limit))
    }

    pub(crate) fn covers(&self, mint: &str) -> bool {
        self.maximum_per_mint_sats.contains_key(mint)
    }

    pub(super) fn reserve(&self, db: &Connection, mint: &str, gross: u64) -> Result<()> {
        let limit = self
            .maximum_per_mint_sats
            .get(mint)
            .ok_or(AdmissionCapacity::Full)?;
        ensure!(
            gross <= *limit && gross <= self.maximum_total_sats,
            AdmissionCapacity::TooLarge
        );
        ensure!(
            gross <= Custody::read(db, Some(mint))?.available(*limit, true)
                && gross <= Custody::read(db, None)?.available(self.maximum_total_sats, true),
            AdmissionCapacity::Full
        );
        Ok(())
    }

    pub(super) fn check_fresh_swap(&self, db: &Connection, mint: &str, gross: u64) -> Result<()> {
        let limit = self
            .maximum_per_mint_sats
            .get(mint)
            .ok_or(AdmissionCapacity::Full)?;
        ensure!(
            gross <= Custody::read(db, Some(mint))?.available(*limit, false)
                && gross <= Custody::read(db, None)?.available(self.maximum_total_sats, false),
            AdmissionCapacity::Full
        );
        Ok(())
    }

    pub(super) fn swap_limits(&self, db: &Connection) -> Result<BTreeMap<&str, u64>> {
        if self.maximum_per_mint_sats.is_empty() {
            return Ok(BTreeMap::new());
        }
        let global = Custody::read(db, None)?.available(self.maximum_total_sats, false);
        self.maximum_per_mint_sats
            .iter()
            .map(|(mint, &limit)| {
                let available = Custody::read(db, Some(mint))?
                    .available(limit, false)
                    .min(global);
                Ok((mint.as_str(), available))
            })
            .collect()
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum AdmissionCapacity {
    Full,
    TooLarge,
}

impl std::fmt::Display for AdmissionCapacity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Full => "wallet admission capacity unavailable; retry original submission later",
            Self::TooLarge => "payment exceeds configured wallet custody limit",
        })
    }
}

impl std::error::Error for AdmissionCapacity {}

/// Read from the same journal snapshot for admission and private operation status.
pub(crate) struct Custody {
    pub unverified_input_sats: u64,
    unsubmitted_input_sats: u64,
    pub acquired_net_sats: u64,
    pub receive_fee_sats: u64,
    pub consumed_sats: u64,
    pub split_preparation_fee_sats: u64,
    pub unsettled_preparation_fee_sats: u64,
    pub retained_sats: u64,
}

impl Custody {
    pub(crate) fn read(db: &Connection, mint: Option<&str>) -> Result<Self> {
        let (unverified_input_sats, unsubmitted_input_sats, acquired_net_sats, receive_fee_sats): (u64, u64, u64, u64) = db.query_row(
            "SELECT COALESCE(SUM(CASE WHEN net IS NULL THEN gross ELSE 0 END),0),
                    COALESCE(SUM(CASE WHEN net IS NULL AND state='validated' THEN gross ELSE 0 END),0),
                    COALESCE(SUM(net),0),COALESCE(SUM(receive_fee),0)
             FROM acquisitions WHERE ?1 IS NULL OR mint_id=?1",
            [mint], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?)))?;
        let consumed_sats: u64 = db.query_row("SELECT COALESCE(SUM(r.input_sats-r.change_sats),0) FROM treasury_source_results r JOIN acquisitions a ON a.id=r.acquisition_id WHERE ?1 IS NULL OR a.mint_id=?1",[mint],|r|r.get(0))?;
        let (split_preparation_fee_sats, unsettled_preparation_fee_sats): (u64, u64) = db.query_row(
            "SELECT COALESCE(SUM(s.fee_sats),0),COALESCE(SUM(CASE WHEN r.intent_id IS NULL THEN s.fee_sats ELSE 0 END),0) FROM treasury_split_results s JOIN treasury_instructions i ON i.id=s.intent_id LEFT JOIN treasury_results r ON r.intent_id=s.intent_id WHERE ?1 IS NULL OR json_extract(i.issued_json,'$.intent.mint_id')=?1",
            params![mint], |r| Ok((r.get(0)?,r.get(1)?)))?;
        let retained_sats = acquired_net_sats
            .checked_sub(consumed_sats)
            .and_then(|v| v.checked_sub(unsettled_preparation_fee_sats))
            .ok_or_else(|| anyhow::anyhow!("journal custody conservation mismatch"))?;
        Ok(Self {
            unverified_input_sats,
            unsubmitted_input_sats,
            acquired_net_sats,
            receive_fee_sats,
            consumed_sats,
            split_preparation_fee_sats,
            unsettled_preparation_fee_sats,
            retained_sats,
        })
    }

    fn available(&self, limit: u64, include_unsubmitted: bool) -> u64 {
        let pending = if include_unsubmitted {
            self.unverified_input_sats
        } else {
            // A validated row has never been sent to the mint. An oversized
            // legacy queue must not preclude safely draining smaller entries.
            self.unverified_input_sats - self.unsubmitted_input_sats
        };
        limit
            .saturating_sub(self.retained_sats)
            .saturating_sub(pending)
    }
}
