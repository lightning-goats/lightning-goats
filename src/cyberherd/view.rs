//! Public, read-only projection. No provider/payment, source, payout, signer or
//! operator command records are serialized from the membership store.
use super::{
    delivery::{display_name, ratio},
    shares::{SocialAwards, point_units},
    store::Snapshot,
};
use anyhow::{Context as _, Result, bail};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Member {
    pub rank: u16,
    pub pubkey: String,
    pub display_name: String,
    pub picture: Option<String>,
    /// Whole sats and points use decimal text, never a lossy browser float.
    pub amount_sats: String,
    pub points: String,
    pub share_micro_percent: u64,
    pub share_percent: String,
    pub awards: SocialAwards,
    pub share_eligible: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Roster {
    pub owner: String,
    pub cycle: i64,
    pub cycle_start: i64,
    pub revision: i64,
    pub capacity: u16,
    pub minimum_sats: String,
    pub member_percent: u8,
    pub total_members: u16,
    pub members: Vec<Member>,
}
impl Roster {
    pub fn from_snapshot(snapshot: &Snapshot) -> Result<Self> {
        snapshot.settings.validate()?;
        let shares = snapshot.roster.shares(&snapshot.settings)?;
        let mut rows = vec![];
        for (key, member) in snapshot
            .roster
            .members
            .iter()
            .filter(|(key, m)| m.active && !snapshot.roster.banned.contains(*key))
        {
            crate::nostr_event::validate_public_key(key)?;
            member.profile.validate()?;
            let awards = snapshot.roster.awards.get(key).copied().unwrap_or_default();
            let units = point_units(member.amount_sats, awards, snapshot.settings.minimum_sats)?;
            rows.push((units, key, member, awards));
        }
        rows.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(b.1)));
        let members = rows
            .into_iter()
            .enumerate()
            .map(|(index, (units, key, m, awards))| -> Result<Member> {
                let share = shares.get(key).copied().unwrap_or(0);
                let picture = m.profile.picture.as_ref().and_then(|raw| {
                    if crate::public_network::validate_destination(raw, "https").is_err() {
                        return None;
                    }
                    let canonical = reqwest::Url::parse(raw).ok()?.to_string();
                    (canonical.len() <= 512).then_some(canonical)
                });
                Ok(Member {
                    rank: u16::try_from(index + 1)?,
                    pubkey: key.clone(),
                    display_name: display_name(&m.profile),
                    picture,
                    amount_sats: m.amount_sats.to_string(),
                    points: ratio(units, u128::from(snapshot.settings.minimum_sats.max(1)))?,
                    share_micro_percent: share,
                    share_percent: ratio(u128::from(share), 1_000_000)?,
                    awards,
                    share_eligible: shares.contains_key(key),
                })
            })
            .collect::<Result<Vec<_>>>()?;
        let roster = Self {
            owner: snapshot.owner.clone(),
            cycle: snapshot.cycle,
            cycle_start: snapshot.cycle_start,
            revision: snapshot.revision,
            capacity: snapshot.settings.max_members,
            minimum_sats: snapshot.settings.minimum_sats.to_string(),
            member_percent: snapshot.settings.member_percent,
            total_members: u16::try_from(members.len())?,
            members,
        };
        roster.validate(false)?;
        Ok(roster)
    }
    /// A card packet contains exactly the first ten ranked members; the public
    /// API retains all members in bounded pages. Neither can carry event types.
    pub fn validate(&self, cards: bool) -> Result<()> {
        crate::nostr_event::validate_public_key(&self.owner)?;
        let minimum = decimal(&self.minimum_sats)?;
        if self.cycle <= 0
            || self.cycle_start < 0
            || self.revision < 0
            || !(1..=1000).contains(&self.capacity)
            || self.member_percent > 100
            || self.total_members > self.capacity
            || self.members.len()
                != usize::from(if cards {
                    self.total_members.min(10)
                } else {
                    self.total_members
                })
        {
            bail!("invalid public roster metadata");
        }
        let mut keys = std::collections::BTreeSet::new();
        let mut previous: Option<(u128, &str)> = None;
        let mut total_share = 0u64;
        for (index, m) in self.members.iter().enumerate() {
            crate::nostr_event::validate_public_key(&m.pubkey)?;
            let units = point_units(decimal(&m.amount_sats)?, m.awards, minimum)?;
            if m.rank != u16::try_from(index + 1)?
                || !keys.insert(&m.pubkey)
                || m.display_name.len() > 256
                || m.display_name.chars().any(char::is_control)
                || m.points != ratio(units, u128::from(minimum.max(1)))?
                || m.share_percent != ratio(u128::from(m.share_micro_percent), 1_000_000)?
                || (!m.share_eligible && m.share_micro_percent != 0)
                || previous
                    .is_some_and(|(p, k)| p < units || (p == units && k >= m.pubkey.as_str()))
            {
                bail!("invalid public roster member/order");
            }
            if let Some(picture) = &m.picture {
                if picture.len() > 512 {
                    bail!("public picture exceeds bound");
                }
                crate::public_network::validate_destination(picture, "https")?;
            }
            total_share = total_share
                .checked_add(m.share_micro_percent)
                .context("public share overflow")?;
            previous = Some((units, &m.pubkey));
        }
        if total_share > u64::from(self.member_percent) * 1_000_000 {
            bail!("public roster shares exceed pool");
        }
        Ok(())
    }
    pub fn cards(&self) -> Self {
        let mut result = self.clone();
        result.members.truncate(10);
        result
    }
}
fn decimal(value: &str) -> Result<u64> {
    let n = value.parse::<u64>()?;
    if n > i64::MAX as u64 || value != n.to_string() {
        bail!("public amount must be a canonical whole-sat decimal");
    }
    Ok(n)
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Overlay {
    pub epoch: uuid::Uuid,
    pub roster: Roster,
}
impl Overlay {
    pub fn validate(&self) -> Result<()> {
        self.roster.validate(true)?;
        if self.epoch.is_nil() || serde_json::to_vec(self)?.len() > 12 * 1024 {
            bail!("invalid roster overlay scope/size");
        }
        Ok(())
    }
    pub fn identity(&self) -> Result<String> {
        self.validate()?;
        super::ingress::hash(self)
    }
}
