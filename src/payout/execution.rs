//! Core-owned execution identity. Serialized documents are evidence only; an
//! actual authenticated, fresh core lease is required at the provider boundary.
use super::provider::QuoteRecord;
use crate::reward_funding::{FundingPage, Reservation, RewardPolicy};
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub reservation: Reservation,
    pub member: String,
    pub quote_json: String,
}
impl Request {
    pub(crate) fn quote(&self) -> Result<QuoteRecord> {
        self.reservation.validate()?;
        crate::nostr_event::validate_public_key(&self.member)?;
        if self.member == self.reservation.owner || self.quote_json.len() > 24576 {
            bail!("invalid execution claim scope/size");
        }
        let quote: QuoteRecord = serde_json::from_str(&self.quote_json)
            .map_err(|_| anyhow::anyhow!("invalid execution quote evidence"))?;
        quote.validate()?;
        if quote.request.created_at < self.reservation.created_at
            || quote.request.gross_sats > self.reservation.reserved_sats
            || serde_json::to_string(&quote)? != self.quote_json
        {
            bail!("execution quote differs from reservation/canonical evidence");
        }
        Ok(quote)
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Record {
    pub id: Uuid,
    pub request: Request,
    pub created_at: i64,
}
impl Record {
    pub fn validate(&self) -> Result<()> {
        self.request.quote()?;
        if self.id.is_nil() || self.created_at < self.request.reservation.created_at {
            bail!("invalid immutable execution claim");
        }
        Ok(())
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Lease {
    pub record: Record,
    pub execution_enabled: bool,
    pub nostr_enabled: bool,
    pub observed_at: i64,
}
impl Lease {
    pub fn validate(&self) -> Result<()> {
        self.record.validate()?;
        if self.observed_at < 0 {
            bail!("invalid execution observation time");
        }
        Ok(())
    }
    pub fn permitted(&self, now: i64) -> bool {
        self.execution_enabled
            && self.observed_at >= 0
            && self.observed_at <= now
            && now - self.observed_at <= 5
            && self
                .record
                .request
                .quote()
                .is_ok_and(|q| !q.request.nip57 || self.nostr_enabled)
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct State {
    pub page: FundingPage,
    pub claim: Option<Lease>,
}
impl State {
    pub fn validate(&self, epoch: Uuid, id: Uuid, owner: &str, member: &str) -> Result<()> {
        crate::nostr_event::validate_public_key(member)?;
        self.page.validate()?;
        if self.page.after != 0
            || self.page.reservation.id != id
            || self.page.reservation.epoch != epoch
            || self.page.reservation.owner != owner
        {
            bail!("execution state scope changed");
        }
        if let Some(claim) = &self.claim {
            claim.validate()?;
            if claim.record.request.reservation != self.page.reservation
                || claim.record.request.member != member
                || claim.observed_at != self.page.observed_at
                || claim.execution_enabled != self.page.execution_enabled
                || claim.nostr_enabled != self.page.nostr_enabled
            {
                bail!("execution claim and funding snapshot disagree");
            }
        }
        Ok(())
    }
}
pub(crate) struct Limits<'a> {
    pub policy: Option<&'a RewardPolicy>,
    pub account: Option<Uuid>,
    pub enabled: bool,
    pub nostr: bool,
}

pub use crate::core_bridge::{
    AuthenticatedExecutionLease as AuthenticatedLease,
    AuthenticatedExecutionState as AuthenticatedState,
};

pub(crate) fn record_json(record: &Record) -> Result<String> {
    record.validate()?;
    serde_json::to_string(record).context("cannot encode execution record")
}
