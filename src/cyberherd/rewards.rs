//! Frozen confirmed-feeding allocation. A plan is private accounting evidence,
//! not permission to submit a payment or to announce a treat.
use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::{
    ingress::{CoreBinding, FeedingBarrier, hash},
    membership::Profile,
    shares::{FULL_PERCENT, MICRO_PERCENT, SocialAwards, point_units, proportional_shares},
};
use crate::{
    payout::recipient::{Recipient, address_identity},
    reward_funding::{Allowance, Reservation},
};

pub struct Preparation<'a> {
    pub policy: &'a PlanningPolicy,
    pub allowance: Option<&'a crate::core_bridge::AuthenticatedAllowance>,
    pub profiles: &'a [ProfileRead],
    pub recipients: &'a BTreeMap<String, Option<Recipient>>,
    pub now: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PlanningPolicy {
    pub fee_basis_points: u16,
    pub minimum_fee_sats: u64,
    pub minimum_net_sats: u64,
    // Omitted zero must keep historical frozen-plan serialization/hash exact.
    #[serde(default, skip_serializing_if = "zero")]
    pub maximum_net_sats: u64,
    pub profile_max_age_seconds: u32,
    pub operation_addresses: Vec<String>,
}

fn zero(value: &u64) -> bool {
    *value == 0
}

impl PlanningPolicy {
    pub fn validate(&self) -> Result<()> {
        if self.fee_basis_points > 10000
            || self.minimum_fee_sats > i64::MAX as u64
            || self.minimum_net_sats == 0
            || self.minimum_net_sats > i64::MAX as u64 / 1000
            || self.maximum_net_sats > i64::MAX as u64 / 1000
            || !(60..=86400).contains(&self.profile_max_age_seconds)
            || self.operation_addresses.len() > 128
        {
            bail!("invalid root reward planning policy");
        }
        for address in &self.operation_addresses {
            address_identity(address)?;
        }
        Ok(())
    }
}

/// Successful relay completion (including an authenticated negative profile)
/// is distinct from an unavailable read. Fields are not caller-constructible.
#[derive(Debug, Clone)]
pub struct ProfileRead {
    pub(crate) pubkey: String,
    pub(crate) head_id: Option<String>,
    pub(crate) profile: Option<Profile>,
    pub(crate) observed_at: i64,
}

impl ProfileRead {
    pub fn address(&self) -> Option<&str> {
        self.profile.as_ref()?.lud16.as_deref()
    }
    pub fn pubkey(&self) -> &str {
        &self.pubkey
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Skip {
    MissingProfile,
    NoAddress,
    Operator,
    SharedAddress,
    SharedCallback,
    ZeroShare,
    BelowMinimum,
    RecipientUnavailable,
    RecipientMinimum,
    RecipientMaximum,
    PolicyMaximum,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Row {
    pub pubkey: String,
    pub profile_event_id: Option<String>,
    pub profile: Option<Profile>,
    pub amount_sats: u64,
    pub awards: SocialAwards,
    /// Decimal u128; JSON consumers must not round it to a binary float.
    pub point_units: String,
    pub share_micro_percent: u64,
    pub gross_sats: u64,
    pub fee_limit_sats: u64,
    pub net_sats: u64,
    pub gross_micro_percent_of_feed: u64,
    pub recipient: Option<Recipient>,
    pub skip: Option<Skip>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conversation: Option<super::conversation::MemberConversation>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub score_basis: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub zap_content: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Plan {
    pub version: u8,
    pub owner: String,
    pub epoch: Uuid,
    pub core_uid: u32,
    pub request_id: Uuid,
    pub barrier_hash: String,
    pub cycle: i64,
    pub roster_revision: i64,
    pub funding_hash: Option<String>,
    pub real_sats: u64,
    pub threshold_sats: u64,
    pub member_percent: u8,
    pub budget_sats: u64,
    pub reserved_sats: u64,
    pub operations_sats: u64,
    pub root_policy_hash: Option<String>,
    pub planning_policy: PlanningPolicy,
    pub created_at: i64,
    pub rows: Vec<Row>,
}

impl Plan {
    pub fn hash(&self) -> Result<String> {
        hash(self)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum State {
    AwaitingReservation,
    Reserved,
    NoPayout,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredPlan {
    pub hash: String,
    pub plan: Plan,
    pub state: State,
    pub reservation: Option<Reservation>,
}

/// Called only with a stored immutable barrier, current profile observations,
/// and the authenticated core allowance verified by the store.
pub(super) fn build(
    barrier: &FeedingBarrier,
    binding: &CoreBinding,
    policy: &PlanningPolicy,
    allowance: Option<&Allowance>,
    profiles: &[ProfileRead],
    recipients: &BTreeMap<String, Option<Recipient>>,
    now: i64,
) -> Result<Plan> {
    policy.validate()?;
    let snapshot = &barrier.snapshot;
    snapshot.settings.validate()?;
    if now < 0 || now < barrier.feeding.confirmed_at || snapshot.owner.is_empty() {
        bail!("invalid reward plan time/scope");
    }
    let funding = barrier.feeding.funding.as_ref();
    let real_sats = funding.map_or(0, |f| f.strike_sats);
    let mut percent = snapshot.settings.member_percent;
    let mut cap = 0;
    if let Some(a) = allowance {
        a.validate()?;
        if a.owner != snapshot.owner
            || a.epoch != binding.epoch
            || a.request_id != barrier.feeding.request_id
            || a.threshold_sats != barrier.feeding.threshold_sats
            || Some(&a.funding) != funding
        {
            bail!("reward allowance contradicts confirmed feeding barrier");
        }
        percent = percent.min(a.member_percent_ceiling);
        cap = a.max_sats_per_feed;
    } else if real_sats > 0 && percent > 0 {
        bail!("real reward planning requires fresh root allowance");
    }
    let budget = u64::try_from(u128::from(real_sats) * u128::from(percent) / 100)?.min(cap);
    let mut plan = Plan {
        version: 1,
        owner: snapshot.owner.clone(),
        epoch: binding.epoch,
        core_uid: binding.core_uid,
        request_id: barrier.feeding.request_id,
        barrier_hash: hash(barrier)?,
        cycle: snapshot.cycle,
        roster_revision: snapshot.revision,
        funding_hash: funding.map(|f| f.proof_hash.clone()),
        real_sats,
        threshold_sats: barrier.feeding.threshold_sats,
        member_percent: percent,
        budget_sats: budget,
        reserved_sats: 0,
        operations_sats: barrier.feeding.threshold_sats,
        root_policy_hash: allowance.map(|a| a.policy_hash.clone()),
        planning_policy: policy.clone(),
        created_at: now,
        rows: vec![],
    };
    // Explicitly unfunded/zero-policy feedings are closed without profile or
    // recipient work. They cannot be reopened later as retroactive rewards.
    if budget == 0 {
        return Ok(plan);
    }
    let operations: BTreeSet<_> = policy
        .operation_addresses
        .iter()
        .map(|a| address_identity(a))
        .collect::<Result<_>>()?;
    let mut addresses: BTreeMap<String, usize> = BTreeMap::new();
    let mut callbacks: BTreeMap<String, usize> = BTreeMap::new();
    let mut weights = Vec::new();
    for (key, member) in &snapshot.roster.members {
        if !member.active || snapshot.roster.banned.contains(key) {
            continue;
        }
        let read = profiles
            .iter()
            .find(|p| p.pubkey == *key)
            .context("fresh profile read missing")?;
        if read.observed_at > now
            || now - read.observed_at > i64::from(policy.profile_max_age_seconds)
        {
            bail!("reward profile read expired");
        }
        let awards = snapshot.roster.awards.get(key).copied().unwrap_or_default();
        let points = point_units(member.amount_sats, awards, snapshot.settings.minimum_sats)?;
        let mut row = Row {
            pubkey: key.clone(),
            profile_event_id: read.head_id.clone(),
            profile: read.profile.clone(),
            amount_sats: member.amount_sats,
            awards,
            point_units: points.to_string(),
            share_micro_percent: 0,
            gross_sats: 0,
            fee_limit_sats: 0,
            net_sats: 0,
            gross_micro_percent_of_feed: 0,
            recipient: None,
            skip: None,
            conversation: barrier.conversations.get(key).cloned(),
            score_basis: Some(snapshot.settings.minimum_sats.max(1)),
            zap_content: None,
        };
        match &read.profile {
            None => row.skip = Some(Skip::MissingProfile),
            Some(profile) => match &profile.lud16 {
                None => row.skip = Some(Skip::NoAddress),
                Some(address) => {
                    let identity = address_identity(address)?;
                    if key == &snapshot.owner || operations.contains(&identity) {
                        row.skip = Some(Skip::Operator);
                    } else {
                        *addresses.entry(identity).or_default() += 1;
                    }
                }
            },
        }
        if row.skip.is_none() {
            row.recipient = recipients
                .get(key)
                .context("recipient resolution missing")?
                .clone();
            if let Some(recipient) = &row.recipient {
                recipient.validate(read.address().context("eligible profile missing address")?)?;
                *callbacks.entry(recipient.callback_identity()?).or_default() += 1;
            }
        }
        plan.rows.push(row);
    }
    for row in &mut plan.rows {
        if row.skip.is_none() {
            let identity = address_identity(
                row.profile
                    .as_ref()
                    .and_then(|p| p.lud16.as_deref())
                    .context("missing payee")?,
            )?;
            if addresses[&identity] > 1 {
                row.skip = Some(Skip::SharedAddress);
            } else if row
                .recipient
                .as_ref()
                .map(|r| r.callback_identity())
                .transpose()?
                .is_some_and(|id| callbacks[&id] > 1)
            {
                row.skip = Some(Skip::SharedCallback);
            }
        }
        if row.skip.is_none() {
            weights.push((row.pubkey.clone(), row.point_units.parse::<u128>()?));
        }
    }
    let shares = proportional_shares(&weights, percent)?;
    for row in &mut plan.rows {
        if row.skip.is_some() {
            continue;
        }
        row.share_micro_percent = shares[&row.pubkey];
        row.gross_sats = u64::try_from(
            u128::from(budget) * u128::from(row.share_micro_percent)
                / (u128::from(percent) * u128::from(MICRO_PERCENT)),
        )?;
        row.gross_micro_percent_of_feed = u64::try_from(
            u128::from(row.gross_sats) * u128::from(FULL_PERCENT) / u128::from(plan.threshold_sats),
        )?;
        let fee = u64::try_from(
            (u128::from(row.gross_sats) * u128::from(policy.fee_basis_points)).div_ceil(10000),
        )?
        .max(policy.minimum_fee_sats);
        let net = row.gross_sats.saturating_sub(fee);
        row.skip = if row.gross_sats == 0 {
            Some(Skip::ZeroShare)
        } else if fee >= row.gross_sats
            || net < policy.minimum_net_sats
            || net > i64::MAX as u64 / 1000
        {
            Some(Skip::BelowMinimum)
        } else if net > policy.maximum_net_sats {
            Some(Skip::PolicyMaximum)
        } else {
            match &row.recipient {
                None => Some(Skip::RecipientUnavailable),
                Some(r) if net * 1000 < r.min_msat => Some(Skip::RecipientMinimum),
                Some(r) if net * 1000 > r.max_msat => Some(Skip::RecipientMaximum),
                Some(_) => None,
            }
        };
        if row.skip.is_none() {
            row.fee_limit_sats = fee;
            row.net_sats = net;
            plan.reserved_sats = plan
                .reserved_sats
                .checked_add(row.gross_sats)
                .context("reward sum overflow")?;
        }
    }
    if plan.reserved_sats > plan.budget_sats
        || allowance.is_some_and(|a| plan.reserved_sats > a.available_sats)
    {
        bail!("reward plan exceeds currently available root budget");
    }
    plan.operations_sats = plan
        .threshold_sats
        .checked_sub(plan.reserved_sats)
        .context("reward exceeds feeding")?;
    Ok(plan)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        core_bridge::Feeding,
        cyberherd::{
            membership::{Member, Roster, Settings},
            store::Snapshot,
        },
        reward_funding::Funding,
    };
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn fee_and_rounding_budget_is_conserved_without_carry(
            real in 0u64..1_000_001, percent in 0u8..=100, ceiling in 0u8..=100, cap in 0u64..100_001,
            bps in 0u16..=10_000, min_fee in 0u64..101, amount in 1u64..1_000_001,
        ) {
            let public=|n|crate::nostr_event::test_event(n,0,100,"{}",vec![]).pubkey;
            let owner=public(1);let member=public(2);
            let profile=Profile{name:"Member".into(),picture:None,lud16:Some("member@example.com".into())};
            let binding=CoreBinding{core_uid:10001,epoch:Uuid::new_v4(),start_after:0,credit_pool:"herd".into(),payees:BTreeMap::from([("herd".into(),owner.clone())])};
            let funding=Funding{strike_sats:real,proof_hash:"ab".repeat(32)};
            let barrier=FeedingBarrier{ingress_seq:1,conversations:BTreeMap::new(),feeding:Feeding{request_id:Uuid::new_v4(),threshold_sats:1_000_000,confirmed_at:100,presentation:None,funding:Some(funding.clone())},
                snapshot:Snapshot{owner:owner.clone(),settings:Settings{minimum_sats:10,max_members:10,member_percent:percent},roster:Roster{
                    members:BTreeMap::from([(member.clone(),Member{profile:profile.clone(),active:true,amount_sats:amount,lifetime_sats:amount})]),..Roster::default()},cycle:1,cycle_start:0,revision:1}};
            let allowance=Allowance{epoch:binding.epoch,request_id:barrier.feeding.request_id,owner,funding,threshold_sats:1_000_000,
                member_percent_ceiling:ceiling,max_sats_per_feed:cap,available_sats:i64::MAX as u64,policy_hash:"cd".repeat(32)};
            let planning=PlanningPolicy{fee_basis_points:bps,minimum_fee_sats:min_fee,minimum_net_sats:1,maximum_net_sats:i64::MAX as u64 / 1000,profile_max_age_seconds:3600,operation_addresses:vec![]};
            let read=ProfileRead{pubkey:member.clone(),head_id:Some("ef".repeat(32)),profile:Some(profile),observed_at:100};
            let recipient=Recipient::parse("member@example.com",br#"{"tag":"payRequest","callback":"https://pay.example.com/member","minSendable":1,"maxSendable":1000000000000,"metadata":"[[\"text/plain\",\"Treats\"]]"}"#).unwrap();
            let plan=build(&barrier,&binding,&planning,Some(&allowance),&[read],&BTreeMap::from([(member,Some(recipient))]),100).unwrap();
            let budget=(real*u64::from(percent.min(ceiling))/100).min(cap);
            prop_assert_eq!(plan.budget_sats,budget);
            prop_assert!(plan.reserved_sats<=budget);
            prop_assert_eq!(plan.operations_sats+plan.reserved_sats,1_000_000);
            if budget==0 {prop_assert!(plan.rows.is_empty());}
            else {
                let row=&plan.rows[0];
                prop_assert_eq!(row.gross_sats,budget);
                prop_assert_eq!(row.net_sats+row.fee_limit_sats,plan.reserved_sats);
                if row.skip.is_none() {
                    prop_assert!(row.fee_limit_sats>=min_fee);
                    prop_assert!(u128::from(row.fee_limit_sats)*10000>=u128::from(row.gross_sats)*u128::from(bps));
                } else {prop_assert_eq!((row.net_sats,row.fee_limit_sats),(0,0));}
            }
        }
    }
}
