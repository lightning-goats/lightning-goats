//! Authenticate every consumed real receive with the payout credential. A core
//! budget or an operator-labelled account UUID alone is not provider evidence.
use std::collections::BTreeSet;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::{
    core_bridge::AuthenticatedFundingPage,
    reward_funding::{Funding, Reservation, StrikeFundingAnchor},
    strike::StrikeClient,
};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ProviderScope {
    pub environment: String,
    pub account_id: Uuid,
    pub credential_fingerprint: String,
}

pub struct SourceVerifier {
    client: StrikeClient,
    scope: ProviderScope,
}

impl SourceVerifier {
    /// Root configuration names the account; resource access with this exact
    /// credential establishes the relation to actual credited receives. No
    /// public profile or fabricated provider account attestation is used.
    pub fn new(account_id: Uuid, expected_fingerprint: &str, api_key: String) -> Result<Self> {
        let scope = Self::scope(account_id, expected_fingerprint, &api_key)?;
        Ok(Self {
            client: StrikeClient::new("https://api.strike.me", api_key)?,
            scope,
        })
    }

    fn scope(account_id: Uuid, expected: &str, key: &str) -> Result<ProviderScope> {
        crate::nostr_event::validate_hex(expected, 32)?;
        if account_id.is_nil()
            || key.is_empty()
            || key.len() > 4096
            || key.chars().any(char::is_control)
            || key.trim() != key
        {
            bail!("invalid payout account/credential binding");
        }
        let actual = hex::encode(Sha256::digest(key.as_bytes()));
        if actual != expected {
            bail!("payout credential differs from root fingerprint");
        }
        Ok(ProviderScope {
            environment: "strike-production".into(),
            account_id,
            credential_fingerprint: actual,
        })
    }

    pub fn provider_scope(&self) -> &ProviderScope {
        &self.scope
    }

    async fn verify(&self, anchor: &StrikeFundingAnchor) -> Result<()> {
        anchor.validate()?;
        // Existing authoritative receive verification handles exact signed
        // request identity and BTC/P2P actual credited amount without rounding.
        // Keep private request IDs/provider errors out of returned diagnostics.
        let actual = self
            .client
            .reconcile_completed_receive(&anchor.stored_request(), anchor.receive_id)
            .await
            .map_err(|_| {
                anyhow::anyhow!("payout credential could not qualify consumed Strike receive")
            })?;
        if actual.amount_msat != anchor.settled_msat || actual.payment_hash != anchor.settled_hash {
            bail!("provider receive contradicts consumed reward funding");
        }
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct ExpectedPlan {
    pub core_uid: u32,
    pub epoch: Uuid,
    pub owner: String,
    pub plan_hash: String,
    /// Independent root payout policy, not the incoming plan's percentage.
    pub member_percent_ceiling: u8,
    pub per_feed_sats: u64,
    pub max_parts: u16,
}

impl ExpectedPlan {
    fn validate(&self) -> Result<()> {
        crate::nostr_event::validate_public_key(&self.owner)?;
        crate::nostr_event::validate_hex(&self.plan_hash, 32)?;
        if self.core_uid == 0
            || self.epoch.is_nil()
            || self.member_percent_ceiling > 100
            || self.per_feed_sats > i64::MAX as u64
            || !(1..=8192).contains(&self.max_parts)
        {
            bail!("invalid independent payout qualification policy");
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct QualifiedSource {
    pub ledger_id: i64,
    pub consumed_sats: u64,
    pub receive_id: Uuid,
    pub receive_request_id: Uuid,
    pub received_sats: u64,
}

/// Not deserializable or constructible from copied funding/plan JSON. A future
/// payout store must independently retain source consumption across grants.
pub struct QualifiedFunding {
    core_uid: u32,
    funding: Funding,
    threshold_sats: u64,
    reservation: Reservation,
    provider: ProviderScope,
    sources: Vec<QualifiedSource>,
}

impl QualifiedFunding {
    pub fn funding(&self) -> &Funding {
        &self.funding
    }
    pub fn threshold_sats(&self) -> u64 {
        self.threshold_sats
    }
    pub fn core_uid(&self) -> u32 {
        self.core_uid
    }
    pub fn reservation(&self) -> &Reservation {
        &self.reservation
    }
    pub fn provider(&self) -> &ProviderScope {
        &self.provider
    }
    pub fn sources(&self) -> &[QualifiedSource] {
        &self.sources
    }
}

pub struct Session {
    expected: ExpectedPlan,
    reservation: Reservation,
    funding: Funding,
    threshold_sats: u64,
    through: i64,
    total_sats: u64,
    real_sats: u64,
    count: usize,
    done: bool,
    provider: Option<ProviderScope>,
    sources: Vec<QualifiedSource>,
    seen: BTreeSet<Uuid>,
}

impl Session {
    pub fn new(expected: ExpectedPlan, first: &AuthenticatedFundingPage) -> Result<Self> {
        expected.validate()?;
        let p = first.page();
        p.validate()?;
        if first.core_uid() != expected.core_uid
            || p.after != 0
            || p.reservation.epoch != expected.epoch
            || p.reservation.owner != expected.owner
            || p.reservation.plan_hash != expected.plan_hash
        {
            bail!("payout qualification requires first authenticated page for exact plan/core");
        }
        let ceiling = u64::try_from(
            u128::from(p.funding.strike_sats) * u128::from(expected.member_percent_ceiling) / 100,
        )?
        .min(expected.per_feed_sats);
        if p.reservation.reserved_sats > ceiling {
            bail!("reservation exceeds independent payout policy");
        }
        Ok(Self {
            expected,
            reservation: p.reservation.clone(),
            funding: p.funding.clone(),
            threshold_sats: p.threshold_sats,
            through: 0,
            total_sats: 0,
            real_sats: 0,
            count: 0,
            done: false,
            provider: None,
            sources: vec![],
            seen: BTreeSet::new(),
        })
    }

    pub fn after(&self) -> i64 {
        self.through
    }
    pub fn is_complete(&self) -> bool {
        self.done
    }

    /// Commit page progress only after ALL its provider reads and sums pass.
    /// Retrying a failed read never silently skips the earlier source.
    pub async fn accept(
        &mut self,
        page: &AuthenticatedFundingPage,
        verifier: &SourceVerifier,
    ) -> Result<()> {
        let p = page.page();
        p.validate()?;
        if self.done
            || page.core_uid() != self.expected.core_uid
            || p.after != self.through
            || p.reservation != self.reservation
            || p.funding != self.funding
            || p.threshold_sats != self.threshold_sats
            || self
                .provider
                .as_ref()
                .is_some_and(|old| old != &verifier.scope)
            || self.count + p.parts.len() > usize::from(self.expected.max_parts)
        {
            bail!("payout funding sequence/provider changed or exceeded bound");
        }
        let mut total = self.total_sats;
        let mut real = self.real_sats;
        let mut sources = Vec::new();
        let mut seen = self.seen.clone();
        for part in &p.parts {
            total = total
                .checked_add(part.consumed_sats)
                .context("funding sum overflow")?;
            if let Some(anchor) = &part.anchor {
                if !seen.insert(anchor.receive_id) {
                    bail!("same provider receive appears in multiple consumed lots");
                }
                verifier.verify(anchor).await?;
                real = real
                    .checked_add(part.consumed_sats)
                    .context("real funding sum overflow")?;
                sources.push(QualifiedSource {
                    ledger_id: part.ledger_id,
                    consumed_sats: part.consumed_sats,
                    receive_id: anchor.receive_id,
                    receive_request_id: anchor.receive_request_id,
                    received_sats: anchor.settled_msat / 1000,
                });
            }
        }
        if total > self.threshold_sats
            || real > self.funding.strike_sats
            || (!p.more && (total != self.threshold_sats || real != self.funding.strike_sats))
        {
            bail!("incomplete or excessive payout funding sequence");
        }
        self.through = p.through;
        self.total_sats = total;
        self.real_sats = real;
        self.count += p.parts.len();
        self.done = !p.more;
        self.provider = Some(verifier.scope.clone());
        self.seen = seen;
        self.sources.extend(sources);
        Ok(())
    }

    pub fn finish(self) -> Result<QualifiedFunding> {
        if !self.done {
            bail!("consumed sources have not all been qualified");
        }
        Ok(QualifiedFunding {
            core_uid: self.expected.core_uid,
            funding: self.funding,
            threshold_sats: self.threshold_sats,
            reservation: self.reservation,
            provider: self.provider.context("provider qualification missing")?,
            sources: self.sources,
        })
    }
}

#[cfg(all(test, unix))]
#[path = "funding_tests.rs"]
mod tests;
