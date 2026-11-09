//! Core-owned proof and budget limits. A reservation is a spending ceiling,
//! never a wallet balance, submitted payment or evidence of delivery.
use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RewardPolicy {
    pub owner: String,
    pub member_percent_ceiling: u8,
    pub max_sats_per_feed: u64,
    pub max_sats_per_24h: u64,
}

impl RewardPolicy {
    pub fn validate(&self) -> Result<()> {
        crate::nostr_event::validate_public_key(&self.owner)?;
        if self.member_percent_ceiling > 100
            || self.max_sats_per_feed > i64::MAX as u64
            || self.max_sats_per_24h > i64::MAX as u64
        {
            bail!("invalid root reward spending policy");
        }
        Ok(())
    }

    pub fn budget(&self, real_sats: u64) -> Result<u64> {
        self.validate()?;
        let percent =
            u64::try_from(u128::from(real_sats) * u128::from(self.member_percent_ceiling) / 100)?;
        Ok(percent.min(self.max_sats_per_feed))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Funding {
    pub strike_sats: u64,
    pub proof_hash: String,
}

impl Funding {
    pub fn validate(&self, threshold_sats: u64) -> Result<()> {
        if self.strike_sats > threshold_sats {
            bail!("funding exceeds confirmed debit");
        }
        crate::nostr_event::validate_hex(&self.proof_hash, 32)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Reservation {
    pub id: Uuid,
    pub request_id: Uuid,
    pub epoch: Uuid,
    pub owner: String,
    pub plan_hash: String,
    pub funding_hash: String,
    pub policy_hash: String,
    pub reserved_sats: u64,
    pub created_at: i64,
}

impl Reservation {
    pub fn validate(&self) -> Result<()> {
        if self.id.is_nil()
            || self.request_id.is_nil()
            || self.epoch.is_nil()
            || self.reserved_sats == 0
            || self.reserved_sats > i64::MAX as u64
            || self.created_at < 0
        {
            bail!("invalid reward reservation");
        }
        crate::nostr_event::validate_public_key(&self.owner)?;
        crate::nostr_event::validate_hex(&self.plan_hash, 32)?;
        crate::nostr_event::validate_hex(&self.policy_hash, 32)?;
        crate::nostr_event::validate_hex(&self.funding_hash, 32)
    }
}
