//! Core-owned proof and budget limits. A reservation is a spending ceiling,
//! never a wallet balance, submitted payment or evidence of delivery.
use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub const MAX_FUNDING_PARTS: usize = 8;

/// Correlated receive evidence, private to the payout role. Provider account
/// qualification must read this actual request/receive with its own credential.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct StrikeFundingAnchor {
    pub receive_request_id: Uuid,
    pub receive_id: Uuid,
    pub address_user: String,
    pub credit_pool: String,
    pub requested_msat: u64,
    pub settled_msat: u64,
    pub invoice: String,
    pub description_hash: String,
    pub payment_hash: String,
    pub settled_hash: Option<String>,
}

impl StrikeFundingAnchor {
    pub fn validate(&self) -> Result<()> {
        if self.receive_request_id.is_nil()
            || self.receive_id.is_nil()
            || self.credit_pool != "herd"
            || self.invoice.len() > 8192
            || self.settled_msat == 0
            || !self.settled_msat.is_multiple_of(1000)
            || self.settled_msat > i64::MAX as u64
            || self.requested_msat > i64::MAX as u64
        {
            bail!("invalid private Strike funding anchor");
        }
        crate::domain::invoice::validate_user(&self.address_user)?;
        crate::nostr_event::validate_hex(&self.payment_hash, 32)?;
        crate::nostr_event::validate_hex(&self.description_hash, 32)?;
        if self
            .settled_hash
            .as_ref()
            .is_some_and(|hash| hash != &self.payment_hash)
        {
            bail!("funding settlement hash disagrees with issued request");
        }
        crate::bolt11::verify(
            &self.invoice,
            self.requested_msat,
            &self.payment_hash,
            &self.description_hash,
        )?;
        Ok(())
    }

    pub fn stored_request(&self) -> crate::ledger::StoredStrikeReceiveRequest {
        crate::ledger::StoredStrikeReceiveRequest {
            receive_request_id: self.receive_request_id,
            address_user: self.address_user.clone(),
            credit_pool: self.credit_pool.clone(),
            amount_msat: self.requested_msat,
            description_hash: self.description_hash.clone(),
            payment_hash: self.payment_hash.clone(),
            invoice: self.invoice.clone(),
            created_provider: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct FundingPart {
    pub ledger_id: i64,
    pub consumed_sats: u64,
    pub anchor: Option<StrikeFundingAnchor>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct FundingPage {
    pub reservation: Reservation,
    pub funding: Funding,
    pub threshold_sats: u64,
    pub execution_enabled: bool,
    pub observed_at: i64,
    pub after: i64,
    pub through: i64,
    pub more: bool,
    pub parts: Vec<FundingPart>,
}

impl FundingPage {
    pub fn execution_permitted(&self, now: i64) -> bool {
        self.execution_enabled
            && self.observed_at >= 0
            && self.observed_at <= now
            && now - self.observed_at <= 5
    }
    pub fn validate(&self) -> Result<()> {
        self.reservation.validate()?;
        self.funding.validate(self.threshold_sats)?;
        if self.threshold_sats == 0
            || self.observed_at < 0
            || self.threshold_sats > i64::MAX as u64
            || self.funding.proof_hash != self.reservation.funding_hash
            || self.reservation.reserved_sats > self.funding.strike_sats
            || self.after < 0
            || self.through < self.after
            || self.parts.len() > MAX_FUNDING_PARTS
            || (self.parts.is_empty() && (self.through != self.after || self.more))
        {
            bail!("invalid private funding page scope/cursor");
        }
        let mut cursor = self.after;
        let mut sum = 0u64;
        for part in &self.parts {
            if part.ledger_id <= cursor || part.consumed_sats == 0 {
                bail!("funding page is not strictly ordered");
            }
            if let Some(anchor) = &part.anchor {
                anchor.validate()?;
                if part.consumed_sats > anchor.settled_msat / 1000 {
                    bail!("funding part exceeds receive");
                }
            }
            sum = sum
                .checked_add(part.consumed_sats)
                .ok_or_else(|| anyhow::anyhow!("funding page sum overflow"))?;
            cursor = part.ledger_id;
        }
        if cursor != self.through
            || sum > self.threshold_sats
            || serde_json::to_vec(self)?.len() > crate::core_bridge::MAX_BATCH_BYTES
        {
            bail!("funding page exceeds cursor, financial or wire bound");
        }
        Ok(())
    }
}

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

/// A fresh advisory read, not a reservation or provider balance. The core
/// repeats all limits atomically when reserving the completed plan.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Allowance {
    pub epoch: Uuid,
    pub request_id: Uuid,
    pub owner: String,
    pub funding: Funding,
    pub threshold_sats: u64,
    pub member_percent_ceiling: u8,
    pub max_sats_per_feed: u64,
    pub available_sats: u64,
    pub policy_hash: String,
}

impl Allowance {
    pub fn validate(&self) -> Result<()> {
        if self.epoch.is_nil()
            || self.request_id.is_nil()
            || self.threshold_sats == 0
            || self.threshold_sats > i64::MAX as u64
            || self.member_percent_ceiling > 100
            || self.max_sats_per_feed > i64::MAX as u64
            || self.available_sats > i64::MAX as u64
        {
            bail!("invalid reward planning allowance");
        }
        crate::nostr_event::validate_public_key(&self.owner)?;
        self.funding.validate(self.threshold_sats)?;
        crate::nostr_event::validate_hex(&self.policy_hash, 32)
    }
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
