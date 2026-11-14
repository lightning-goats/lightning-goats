//! Bounded, resumable read/plan/reserve rounds. No invoice callback, provider
//! spend, signer or public-message capability is present.
use std::collections::BTreeMap;

use anyhow::{Context, Result};
use futures_util::{StreamExt, stream};

use super::{
    calendar,
    discovery::Discovery,
    relay::Query,
    rewards::{PlanningPolicy, State},
    store::Store,
};
use crate::{core_bridge::unix::Client, payout::recipient::Recipient};

#[derive(Debug, PartialEq, Eq)]
pub enum Step {
    Idle,
    Waiting,
    ProfilesRefreshed,
    RecipientsStaged,
    Frozen,
    Reserved,
}

pub struct Worker<R> {
    pub store: Store,
    pub client: Client,
    pub discovery: Discovery<R>,
    pub owner: String,
    pub policy: PlanningPolicy,
}

impl<R: Query> Worker<R> {
    pub async fn tick(&self) -> Result<Step> {
        self.policy.validate()?;
        let Some(id) = self.store.next_reward_feeding(&self.owner).await? else {
            return Ok(Step::Idle);
        };
        if let Some(plan) = self.store.reward_plan(&self.owner, id).await? {
            if plan.state == State::AwaitingReservation {
                // Retry the exact durable request FIRST. A lost response may
                // already have consumed the advisory available budget.
                let approved = self
                    .client
                    .reserve_reward(
                        plan.plan.epoch,
                        id,
                        &self.owner,
                        &plan.hash,
                        plan.plan.reserved_sats,
                    )
                    .await?;
                self.store
                    .attach_reward_reservation(&self.owner, &approved)
                    .await?;
                return Ok(Step::Reserved);
            }
            return Ok(Step::Idle);
        }
        let barrier = self
            .store
            .feeding_barrier(&self.owner, id)
            .await?
            .context("reward feeding barrier missing")?;
        let binding = self.store.core_cursor(&self.owner).await?.binding;
        let funded = barrier
            .feeding
            .funding
            .as_ref()
            .is_some_and(|f| f.strike_sats > 0);
        let allowance = if funded && barrier.snapshot.settings.member_percent > 0 {
            Some(
                self.client
                    .reward_allowance(binding.epoch, id, &self.owner)
                    .await?,
            )
        } else {
            None
        };
        let zero_budget = allowance.as_ref().is_none_or(|a| {
            a.allowance().member_percent_ceiling == 0 || a.allowance().max_sats_per_feed == 0
        });
        if zero_budget {
            self.store
                .freeze_reward_plan(
                    &self.owner,
                    id,
                    super::rewards::Preparation {
                        policy: &self.policy,
                        allowance: allowance.as_ref(),
                        profiles: &[],
                        recipients: &BTreeMap::new(),
                        now: calendar::now()?,
                    },
                )
                .await?;
            return Ok(Step::Frozen);
        }
        let now = calendar::now()?;
        let mut profiles = Vec::new();
        let mut missing = Vec::new();
        for (key, member) in &barrier.snapshot.roster.members {
            if !member.active || barrier.snapshot.roster.banned.contains(key) {
                continue;
            }
            match self
                .store
                .reward_profile_read(&self.owner, key, now, self.policy.profile_max_age_seconds)
                .await?
            {
                Some(read) => profiles.push(read),
                None if missing.len() < 8 => missing.push(key.clone()),
                None => {}
            }
        }
        if !missing.is_empty() {
            self.discovery
                .refresh_reward_profiles(&self.store, &self.owner, &missing, now)
                .await?;
            return Ok(Step::ProfilesRefreshed);
        }
        let mut terms = BTreeMap::new();
        let mut fetch = Vec::new();
        for profile in &profiles {
            if profile.address().is_none() {
                continue;
            }
            match self
                .store
                .staged_reward_recipient(&self.owner, id, profile)
                .await?
            {
                Some(recipient) => {
                    terms.insert(profile.pubkey().to_owned(), recipient);
                }
                None if fetch.len() < 8 => fetch.push(profile.clone()),
                None => {}
            }
        }
        if !fetch.is_empty() {
            let mut results = stream::iter(fetch.into_iter().map(|profile| async move {
                let recipient = match profile.address() {
                    Some(address) => Recipient::fetch(address).await.ok(),
                    None => None,
                };
                (profile, recipient)
            }))
            .buffer_unordered(4);
            while let Some((profile, recipient)) = results.next().await {
                self.store
                    .stage_reward_recipient(&self.owner, id, &profile, recipient.as_ref())
                    .await?;
            }
            return Ok(Step::RecipientsStaged);
        }
        self.store
            .freeze_reward_plan(
                &self.owner,
                id,
                super::rewards::Preparation {
                    policy: &self.policy,
                    allowance: allowance.as_ref(),
                    profiles: &profiles,
                    recipients: &terms,
                    now: calendar::now()?,
                },
            )
            .await?;
        Ok(Step::Frozen)
    }
}
