//! Minimal verified results for member presentation. No invoice, provider ID,
//! callback or payment capability crosses this read-only boundary.
use crate::reward_funding::Reservation;
use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};

pub const PAGE_ROWS: u16 = 16;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
pub enum Status {
    Completed { net_sats: u64, fee_sats: u64 },
    Failed,
    Unsubmitted,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Row {
    pub member: String,
    pub gross_sats: u64,
    pub verified_at: i64,
    pub result: Status,
}
impl Row {
    pub fn validate(&self) -> Result<()> {
        crate::nostr_event::validate_public_key(&self.member)?;
        if self.gross_sats == 0 || self.gross_sats > i64::MAX as u64 || self.verified_at < 0 {
            bail!("invalid verified result amount/time");
        }
        if let Status::Completed { net_sats, fee_sats } = self.result {
            if net_sats == 0
                || net_sats
                    .checked_add(fee_sats)
                    .is_none_or(|n| n > self.gross_sats)
            {
                bail!("verified result cost exceeds allocation");
            }
        }
        Ok(())
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Page {
    pub reservation: Reservation,
    pub manifest_hash: String,
    pub after: u16,
    pub through: u16,
    pub total_rows: u16,
    pub rows: Vec<Row>,
}
impl Page {
    pub fn validate(&self) -> Result<()> {
        self.reservation.validate()?;
        crate::nostr_event::validate_hex(&self.manifest_hash, 32)?;
        if self.total_rows == 0
            || self.total_rows > 1000
            || self.after > self.through
            || self.through > self.total_rows
            || self.rows.len() != usize::from(self.through - self.after)
            || self.rows.len() > usize::from(PAGE_ROWS)
            || (self.rows.is_empty() && self.after != self.total_rows)
        {
            bail!("invalid verified result page range");
        }
        let mut gross = 0u64;
        let mut previous: Option<&str> = None;
        for row in &self.rows {
            row.validate()?;
            if row.verified_at < self.reservation.created_at
                || row.member == self.reservation.owner
                || previous.is_some_and(|p| p >= row.member.as_str())
            {
                bail!("verified result order/scope changed");
            }
            previous = Some(&row.member);
            gross = gross
                .checked_add(row.gross_sats)
                .ok_or_else(|| anyhow::anyhow!("verified result gross overflow"))?;
        }
        if gross > self.reservation.reserved_sats {
            bail!("verified result page exceeds reservation");
        }
        Ok(())
    }
    pub fn complete(&self) -> bool {
        self.through == self.total_rows
    }
}

pub use crate::core_bridge::AuthenticatedRewardResults as AuthenticatedPage;
