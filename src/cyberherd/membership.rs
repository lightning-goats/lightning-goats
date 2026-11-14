//! Deterministic membership transitions. Callers must authenticate sources and
//! atomically persist state, immutable interaction receipts and emitted snapshots.
//! This module does not accept relay receipts as financial settlement evidence.
use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use super::shares::{SocialAwards, point_units, proportional_shares};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Settings {
    pub minimum_sats: u64,
    pub max_members: u16,
    pub member_percent: u8,
}

impl Settings {
    pub fn validate(&self) -> Result<()> {
        if self.minimum_sats > i64::MAX as u64
            || !(1..=1000).contains(&self.max_members)
            || self.member_percent > 100
        {
            bail!("invalid CyberHerd membership settings");
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Profile {
    pub name: String,
    pub picture: Option<String>,
    pub lud16: Option<String>,
}

impl Profile {
    pub fn validate(&self) -> Result<()> {
        if self.name.len() > 160 || self.name.chars().any(char::is_control) {
            bail!("invalid CyberHerd display name");
        }
        if let Some(picture) = &self.picture {
            let url = reqwest::Url::parse(picture).context("invalid profile picture URL")?;
            if picture.len() > 2048
                || url.scheme() != "https"
                || !url.username().is_empty()
                || url.password().is_some()
            {
                bail!("profile picture must use bounded HTTPS without credentials");
            }
        }
        if let Some(lud16) = &self.lud16 {
            validate_lud16(lud16)?;
        }
        Ok(())
    }
}

/// Syntactic eligibility only. Outgoing LNURL resolution must additionally
/// enforce DNS/IP, redirect, body, deadline and signed invoice constraints.
pub fn validate_lud16(address: &str) -> Result<()> {
    if address.len() > 320 {
        bail!("Lightning address is too long");
    }
    let (user, domain) = address
        .split_once('@')
        .context("Lightning address requires user@domain")?;
    if user.is_empty()
        || matches!(user, "." | "..")
        || user.len() > 128
        || !user
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"._+-".contains(&byte))
    {
        bail!("invalid Lightning address user");
    }
    if domain.len() > 253
        || !domain.contains('.')
        || domain.ends_with('.')
        || domain.parse::<std::net::IpAddr>().is_ok()
    {
        bail!("Lightning address requires a public DNS name");
    }
    let lower = domain.to_ascii_lowercase();
    if [".localhost", ".local", ".internal", ".onion"]
        .iter()
        .any(|suffix| lower.ends_with(suffix))
    {
        bail!("Lightning address uses a non-public suffix");
    }
    for label in domain.split('.') {
        if label.is_empty()
            || label.len() > 63
            || label.starts_with('-')
            || label.ends_with('-')
            || !label
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        {
            bail!("invalid Lightning address domain");
        }
    }
    // Use the same non-network syntax policy as the eventual resolver. A
    // reserved suffix or numeric IP alias must become an ineligible profile,
    // not poison the whole reward plan after it passed admission validation.
    let url = crate::public_network::validate_destination(&format!("https://{domain}/"), "https")?;
    if url
        .host_str()
        .is_some_and(|host| host.parse::<std::net::IpAddr>().is_ok())
    {
        bail!("Lightning address requires a DNS name rather than an IP alias");
    }
    Ok(())
}

pub fn validate_pubkey(pubkey: &str) -> Result<()> {
    if pubkey.len() != 64
        || !pubkey
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        bail!("CyberHerd identity must be a canonical 32-byte public key");
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Member {
    pub profile: Profile,
    pub active: bool,
    pub amount_sats: u64,
    pub lifetime_sats: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Roster {
    pub members: BTreeMap<String, Member>,
    pub awards: BTreeMap<String, SocialAwards>,
    pub evidence: BTreeMap<String, SocialAwards>,
    pub banned: BTreeSet<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum Action {
    Zap { amount_sats: u64 },
    Repost,
    Like,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum Target {
    Current,
    ProjectContent,
    Other,
    Unavailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FeederAvailability {
    Ready,
    Paused,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum Decision {
    Deferred,
    Banned,
    IneligibleTarget,
    MissingPayoutAddress,
    BelowMinimum { required_sats: u64 },
    Full { required_sats: u64 },
    AlreadyAwarded,
    Joined,
    Increased,
    SocialAwarded,
    Displaced { victim: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Transition {
    pub decision: Decision,
    pub increment_sats: u64,
    pub social_points: u64,
    pub amount_sats: u64,
    pub shares_micro_percent: BTreeMap<String, u64>,
    pub spots_remaining: u16,
}

impl Roster {
    pub fn active_count(&self) -> usize {
        self.members.values().filter(|member| member.active).count()
    }

    pub fn shares(&self, settings: &Settings) -> Result<BTreeMap<String, u64>> {
        settings.validate()?;
        let mut weights = Vec::new();
        for (key, member) in &self.members {
            if member.active
                && !self.banned.contains(key)
                && member
                    .profile
                    .lud16
                    .as_deref()
                    .is_some_and(|address| validate_lud16(address).is_ok())
            {
                weights.push((
                    key.clone(),
                    point_units(
                        member.amount_sats,
                        self.awards.get(key).copied().unwrap_or_default(),
                        settings.minimum_sats,
                    )?,
                ));
            }
        }
        proportional_shares(&weights, settings.member_percent)
    }

    /// Strong exception safety: overflow/invalid configuration cannot partially
    /// evict a member or change scores. Persistence still needs its own transaction.
    pub fn apply(
        &mut self,
        settings: &Settings,
        pubkey: &str,
        profile: &Profile,
        action: Action,
        target: Target,
        feeder: FeederAvailability,
    ) -> Result<Transition> {
        settings.validate()?;
        validate_pubkey(pubkey)?;
        profile.validate()?;
        if let Action::Zap { amount_sats } = action {
            if amount_sats == 0 || amount_sats > i64::MAX as u64 {
                bail!("CyberHerd zap must contain positive durable whole sats");
            }
        }
        let mut next = self.clone();
        let (decision, increment_sats, social_points) =
            next.apply_inner(settings, pubkey, profile, action, target, feeder)?;
        let transition = Transition {
            decision,
            increment_sats,
            social_points,
            amount_sats: next
                .members
                .get(pubkey)
                .map_or(0, |member| member.amount_sats),
            shares_micro_percent: next.shares(settings)?,
            spots_remaining: usize::from(settings.max_members)
                .saturating_sub(next.active_count())
                .try_into()?,
        };
        *self = next;
        Ok(transition)
    }

    fn apply_inner(
        &mut self,
        settings: &Settings,
        pubkey: &str,
        profile: &Profile,
        action: Action,
        target: Target,
        feeder: FeederAvailability,
    ) -> Result<(Decision, u64, u64)> {
        if feeder != FeederAvailability::Ready || target == Target::Unavailable {
            return Ok((Decision::Deferred, 0, 0));
        }
        if self.banned.contains(pubkey) {
            return Ok((Decision::Banned, 0, 0));
        }
        let active = self.members.get(pubkey).is_some_and(|member| member.active);
        let eligible = target == Target::Current
            || (active && matches!(action, Action::Zap { .. }) && target == Target::ProjectContent);
        if !eligible {
            return Ok((Decision::IneligibleTarget, 0, 0));
        }
        let amount = match action {
            Action::Zap { amount_sats } => amount_sats,
            _ => 0,
        };
        let earned = self.awards.get(pubkey).copied().unwrap_or_default();
        let social = match action {
            Action::Repost => SocialAwards {
                repost: true,
                like: false,
            },
            Action::Like => SocialAwards {
                repost: false,
                like: true,
            },
            Action::Zap { .. } => SocialAwards::default(),
        };
        if (social.repost && earned.repost) || (social.like && earned.like) {
            return Ok((Decision::AlreadyAwarded, 0, 0));
        }
        if amount == 0 {
            self.evidence
                .entry(pubkey.to_owned())
                .or_default()
                .include(social);
        }
        let member = self
            .members
            .entry(pubkey.to_owned())
            .or_insert_with(|| Member {
                profile: profile.clone(),
                active: false,
                amount_sats: 0,
                lifetime_sats: 0,
            });
        member.lifetime_sats = checked_sats_add(member.lifetime_sats, amount)?;
        if active {
            member.profile = profile.clone();
            member.amount_sats = checked_sats_add(member.amount_sats, amount)?;
            let awards = self.awards.entry(pubkey.to_owned()).or_default();
            let previous_points = awards.points();
            awards.include(social);
            return Ok((
                if amount > 0 {
                    Decision::Increased
                } else {
                    Decision::SocialAwarded
                },
                amount,
                awards.points() - previous_points,
            ));
        }
        // A nonmember like cannot enter a full herd, regardless of profile data.
        if amount == 0
            && action == Action::Like
            && self.active_count() >= usize::from(settings.max_members)
        {
            let lowest = self
                .members
                .values()
                .filter(|m| m.active)
                .map(|m| m.amount_sats)
                .min()
                .context("full herd has no member")?;
            let required_sats = lowest
                .checked_add(1)
                .context("headbutt minimum overflow")?
                .max(settings.minimum_sats);
            return Ok((Decision::Full { required_sats }, 0, 0));
        }
        if amount == 0 && profile.lud16.is_none() {
            return Ok((Decision::MissingPayoutAddress, 0, 0));
        }
        if amount > 0 && amount < settings.minimum_sats {
            return Ok((
                Decision::BelowMinimum {
                    required_sats: settings.minimum_sats,
                },
                0,
                0,
            ));
        }
        let mut decision = Decision::Joined;
        if self.active_count() >= usize::from(settings.max_members) {
            let victim = self
                .members
                .iter()
                .filter(|(_, member)| member.active)
                .min_by(|(ak, a), (bk, b)| {
                    a.amount_sats.cmp(&b.amount_sats).then_with(|| ak.cmp(bk))
                })
                .map(|(key, member)| (key.clone(), member.amount_sats))
                .context("full CyberHerd has no active member")?;
            let required_sats = victim
                .1
                .checked_add(1)
                .context("headbutt minimum overflow")?
                .max(settings.minimum_sats);
            if action == Action::Like
                || (action == Action::Repost && victim.1 > 0)
                || (amount > 0 && amount <= victim.1)
            {
                return Ok((Decision::Full { required_sats }, 0, 0));
            }
            let removed = self
                .members
                .get_mut(&victim.0)
                .context("headbutt victim disappeared")?;
            removed.active = false;
            removed.amount_sats = 0;
            decision = Decision::Displaced { victim: victim.0 };
        }
        let member = self
            .members
            .get_mut(pubkey)
            .context("joining member disappeared")?;
        member.active = true;
        member.profile = profile.clone();
        member.amount_sats = amount;
        let awards = self.awards.entry(pubkey.to_owned()).or_default();
        let previous_points = awards.points();
        awards.include(self.evidence.get(pubkey).copied().unwrap_or_default());
        Ok((decision, amount, awards.points() - previous_points))
    }

    /// Replay receipts live outside this cycle state and must never be erased.
    pub fn reset_cycle(&mut self) {
        for member in self.members.values_mut() {
            member.active = false;
            member.amount_sats = 0;
        }
        self.awards.clear();
        self.evidence.clear();
    }

    pub fn ban(&mut self, pubkey: &str) -> Result<()> {
        validate_pubkey(pubkey)?;
        self.banned.insert(pubkey.to_owned());
        if let Some(member) = self.members.get_mut(pubkey) {
            member.active = false;
            member.amount_sats = 0;
        }
        Ok(())
    }
}

fn checked_sats_add(a: u64, b: u64) -> Result<u64> {
    a.checked_add(b)
        .filter(|amount| *amount <= i64::MAX as u64)
        .context("CyberHerd contribution overflow")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings(capacity: u16) -> Settings {
        Settings {
            minimum_sats: 10,
            max_members: capacity,
            member_percent: 10,
        }
    }
    fn key(n: u8) -> String {
        format!("{n:064x}")
    }
    fn profile(n: u8) -> Profile {
        Profile {
            name: format!("Member {n}"),
            picture: None,
            lud16: Some(format!("member{n}@example.com")),
        }
    }
    fn apply(roster: &mut Roster, capacity: u16, n: u8, action: Action) -> Transition {
        roster
            .apply(
                &settings(capacity),
                &key(n),
                &profile(n),
                action,
                Target::Current,
                FeederAvailability::Ready,
            )
            .unwrap()
    }

    #[test]
    fn zap_displaces_any_social_member_then_requires_strictly_more_than_paid_lowest() {
        let mut r = Roster::default();
        assert_eq!(
            apply(&mut r, 2, 2, Action::Repost).decision,
            Decision::Joined
        );
        assert_eq!(apply(&mut r, 2, 1, Action::Like).decision, Decision::Joined);
        assert_eq!(
            apply(&mut r, 2, 3, Action::Zap { amount_sats: 10 }).decision,
            Decision::Displaced { victim: key(1) }
        );
        assert_eq!(
            apply(&mut r, 2, 4, Action::Zap { amount_sats: 20 }).decision,
            Decision::Displaced { victim: key(2) }
        );
        assert_eq!(
            apply(&mut r, 2, 5, Action::Zap { amount_sats: 10 }).decision,
            Decision::Full { required_sats: 11 }
        );
        assert_eq!(
            apply(&mut r, 2, 5, Action::Zap { amount_sats: 11 }).decision,
            Decision::Displaced { victim: key(3) }
        );
        assert_eq!(r.members[&key(3)].amount_sats, 0);
        assert_eq!(r.members[&key(3)].lifetime_sats, 10);
    }

    #[test]
    fn repost_replaces_zero_zap_but_like_never_headbutts() {
        let mut r = Roster::default();
        apply(&mut r, 1, 1, Action::Repost);
        assert!(matches!(
            apply(&mut r, 1, 2, Action::Like).decision,
            Decision::Full { .. }
        ));
        assert_eq!(
            apply(&mut r, 1, 3, Action::Repost).decision,
            Decision::Displaced { victim: key(1) }
        );
        apply(&mut r, 1, 4, Action::Zap { amount_sats: 10 });
        assert!(matches!(
            apply(&mut r, 1, 5, Action::Repost).decision,
            Decision::Full { .. }
        ));
    }

    #[test]
    fn failed_prejoin_socials_promote_once_and_awarded_types_cannot_free_reenter() {
        let mut r = Roster::default();
        apply(&mut r, 1, 1, Action::Zap { amount_sats: 10 });
        apply(&mut r, 1, 2, Action::Like);
        apply(&mut r, 1, 2, Action::Repost);
        assert!(!r.awards.contains_key(&key(2)));
        let admitted = apply(&mut r, 1, 2, Action::Zap { amount_sats: 11 });
        assert_eq!(admitted.social_points, 4);
        assert_eq!(
            apply(&mut r, 1, 2, Action::Like).decision,
            Decision::AlreadyAwarded
        );
        apply(&mut r, 1, 3, Action::Zap { amount_sats: 12 });
        assert_eq!(
            apply(&mut r, 1, 2, Action::Repost).decision,
            Decision::AlreadyAwarded
        );
        let reentered = apply(&mut r, 1, 2, Action::Zap { amount_sats: 13 });
        assert_eq!(reentered.social_points, 0);
        assert_eq!(r.members[&key(2)].amount_sats, 13);
        assert_eq!(r.members[&key(2)].lifetime_sats, 24);
    }

    #[test]
    fn active_small_zap_accepts_project_content_and_social_requires_current_target() {
        let mut r = Roster::default();
        apply(&mut r, 1, 1, Action::Zap { amount_sats: 10 });
        let increased = r
            .apply(
                &settings(1),
                &key(1),
                &profile(1),
                Action::Zap { amount_sats: 1 },
                Target::ProjectContent,
                FeederAvailability::Ready,
            )
            .unwrap();
        assert_eq!(increased.decision, Decision::Increased);
        assert_eq!(increased.amount_sats, 11);
        assert_eq!(
            r.apply(
                &settings(1),
                &key(1),
                &profile(1),
                Action::Like,
                Target::ProjectContent,
                FeederAvailability::Ready
            )
            .unwrap()
            .decision,
            Decision::IneligibleTarget
        );
        assert_eq!(
            r.apply(
                &settings(1),
                &key(2),
                &profile(2),
                Action::Zap { amount_sats: 100 },
                Target::ProjectContent,
                FeederAvailability::Ready
            )
            .unwrap()
            .decision,
            Decision::IneligibleTarget
        );
    }

    #[test]
    fn no_nip05_requirement_free_entry_needs_address_paid_member_without_one_has_no_share() {
        let mut r = Roster::default();
        let no_address = Profile {
            name: "Alice".into(),
            ..Profile::default()
        };
        assert_eq!(
            r.apply(
                &settings(2),
                &key(1),
                &no_address,
                Action::Like,
                Target::Current,
                FeederAvailability::Ready
            )
            .unwrap()
            .decision,
            Decision::MissingPayoutAddress
        );
        let paid = r
            .apply(
                &settings(2),
                &key(1),
                &no_address,
                Action::Zap { amount_sats: 10 },
                Target::Current,
                FeederAvailability::Ready,
            )
            .unwrap();
        assert_eq!(paid.decision, Decision::Joined);
        assert_eq!(paid.social_points, 1);
        assert!(paid.shares_micro_percent.is_empty());
        let social = apply(&mut r, 2, 2, Action::Like);
        assert_eq!(social.shares_micro_percent[&key(2)], 10_000_000);
    }

    #[test]
    fn paused_unknown_banned_and_other_target_never_change_membership() {
        for (feeder, target) in [
            (FeederAvailability::Paused, Target::Current),
            (FeederAvailability::Unknown, Target::Current),
            (FeederAvailability::Ready, Target::Unavailable),
        ] {
            let mut r = Roster::default();
            let before = r.clone();
            assert_eq!(
                r.apply(
                    &settings(1),
                    &key(1),
                    &profile(1),
                    Action::Like,
                    target,
                    feeder
                )
                .unwrap()
                .decision,
                Decision::Deferred
            );
            assert_eq!(r, before);
        }
        let mut r = Roster::default();
        apply(&mut r, 1, 1, Action::Zap { amount_sats: 10 });
        r.ban(&key(1)).unwrap();
        r.reset_cycle();
        assert_eq!(
            apply(&mut r, 1, 1, Action::Zap { amount_sats: 100 }).decision,
            Decision::Banned
        );
        assert_eq!(r.members[&key(1)].lifetime_sats, 10);
        assert!(r.awards.is_empty());
    }

    #[test]
    fn overflow_never_partially_mutates_and_bad_addresses_fail_closed() {
        let mut r = Roster::default();
        apply(
            &mut r,
            1,
            1,
            Action::Zap {
                amount_sats: i64::MAX as u64,
            },
        );
        let before = r.clone();
        assert!(
            r.apply(
                &settings(1),
                &key(1),
                &profile(1),
                Action::Zap { amount_sats: 1 },
                Target::Current,
                FeederAvailability::Ready
            )
            .is_err()
        );
        assert_eq!(r, before);
        for address in [
            "no-at",
            "a@127.0.0.1",
            "a@localhost",
            "a@home.local",
            "a@evil.com/path",
            "a@evil.com:443",
            "a@@evil.com",
            "a@-evil.com",
            "a@evil.com.",
            "a@pay.test",
            "a@pay.TEST",
            "a@pay.invalid",
            "a@pay.home",
            "a@1.2.3",
            "a@8.8.8.8",
            ".@example.com",
            "..@example.com",
        ] {
            assert!(validate_lud16(address).is_err(), "{address}");
        }
        assert!(validate_lud16("alice+goats@pay.example.com").is_ok());
    }
}
