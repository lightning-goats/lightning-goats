//! Frozen human/public presentation. Rendering cannot create financial events
//! or change membership, and never consults mutable scores for a queued result.
use super::{
    conversation::{self, Context, MemberConversation},
    membership::{Action, Decision, Profile, Transition},
    shares::{self, SocialAwards},
    store::{Interaction, Snapshot, StoredEvent},
    templates::Catalog,
};
use anyhow::{Context as _, Result, bail};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Rendered {
    pub identity: String,
    pub cycle: i64,
    pub member: String,
    pub conversation: Option<MemberConversation>,
    pub context: Context,
    pub category: String,
    pub nostr: String,
    pub overlay: String,
    pub accepted: bool,
    pub requires_joining: bool,
    pub additional_members: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub broadcast: Option<super::feeding_delivery::Broadcast>,
}

#[derive(Deserialize)]
struct InteractionEvent {
    interaction: Interaction,
    transition: Transition,
    snapshot: Snapshot,
    context: Option<super::targets::InteractionContext>,
    #[serde(default)]
    conversation: Option<MemberConversation>,
    #[serde(default)]
    allocation_changed: bool,
    #[serde(default)]
    victim: Option<(String, Profile, u64)>,
}
#[derive(Deserialize)]
struct ResultEvent {
    result: Option<crate::payout::results::Row>,
    breakdown: super::rewards::Row,
}

pub(crate) fn render(event: &StoredEvent, catalog: &Catalog) -> Result<Option<Rendered>> {
    match event.event_type.as_str() {
        "feeding_notice" => {
            let message: Rendered = serde_json::from_str(&event.snapshot_json)?;
            message.validate_broadcast(&event.owner)?;
            Ok(Some(message))
        }
        "operator_member" => operator_member(event, catalog),
        "interaction" => interaction(event, catalog),
        "payout_result" | "payout_skipped" => payout(event, catalog),
        // Reset, operator edits, weather and information never become member
        // thread posts by falling through an arbitrary template/category name.
        _ => Ok(None),
    }
}

fn interaction(event: &StoredEvent, catalog: &Catalog) -> Result<Option<Rendered>> {
    let input: InteractionEvent = serde_json::from_str(&event.snapshot_json)?;
    let i = &input.interaction;
    let t = &input.transition;
    let snapshot = &input.snapshot;
    if snapshot.owner != event.owner || snapshot.cycle != event.cycle {
        bail!("message snapshot scope changed");
    }
    if matches!(
        t.decision,
        Decision::Deferred | Decision::AlreadyAwarded | Decision::IneligibleTarget
    ) || matches!(i.action, Action::Zap { amount_sats } if amount_sats < 10)
    {
        return Ok(None);
    }
    let accepted = matches!(
        t.decision,
        Decision::Joined
            | Decision::Displaced { .. }
            | Decision::Increased
            | Decision::SocialAwarded
    );
    let category = match (&t.decision, i.action) {
        (Decision::Displaced { .. }, Action::Repost) => "repost_displaces",
        (Decision::Displaced { .. }, _) => "headbutt_success",
        (Decision::Joined, Action::Repost) => "kind_6_repost",
        (Decision::Joined, Action::Like) => "kind_7_reaction",
        (Decision::Joined, _) => "member_added",
        (Decision::Increased, _) => "member_increase",
        (Decision::SocialAwarded, Action::Repost) => "existing_member_repost",
        (Decision::SocialAwarded, _) => "existing_member_reaction",
        (Decision::Full { .. }, Action::Repost) => "kind_6_headbutt_failure",
        (Decision::Full { .. }, Action::Like) => "kind_7_headbutt_failure",
        (Decision::Full { .. }, _) => "headbutt_failure",
        (Decision::BelowMinimum { .. }, _) => "join_below_minimum",
        _ => "cyber_herd_unadmitted",
    };
    let identity = conversation::identity(&event.owner, &i.pubkey, &i.source_id)?;
    let additional_members = match &t.decision {
        Decision::Displaced { victim } => vec![victim.clone()],
        _ => vec![],
    };
    let context = input
        .context
        .as_ref()
        .map(Context::from_interaction)
        .unwrap_or_default();
    context.validate(&event.owner)?;
    if let Some(c) = &input.conversation {
        c.validate(&event.owner)?;
    }
    let mut human = values(&i.profile, &i.pubkey, false)?;
    let mut public = values(&i.profile, &i.pubkey, true)?;
    let minimum = snapshot.settings.minimum_sats.max(1);
    let awards = snapshot
        .roster
        .awards
        .get(&i.pubkey)
        .copied()
        .unwrap_or_default();
    let amount = snapshot
        .roster
        .members
        .get(&i.pubkey)
        .map_or(0, |m| m.amount_sats);
    let share = t.shares_micro_percent.get(&i.pubkey).copied().unwrap_or(0);
    let victim = input.victim.or_else(|| {
        snapshot
            .roster
            .members
            .iter()
            .filter(|(_, m)| m.active)
            .min_by(|(a, x), (b, y)| x.amount_sats.cmp(&y.amount_sats).then(a.cmp(b)))
            .map(|(key, m)| (key.clone(), m.profile.clone(), m.amount_sats))
    });
    for (v, mentions) in [(&mut human, false), (&mut public, true)] {
        v.insert("increase_amount".into(), t.increment_sats.to_string());
        v.insert(
            "interaction_sats".into(),
            match i.action {
                Action::Zap { amount_sats } => amount_sats,
                _ => 0,
            }
            .to_string(),
        );
        v.insert(
            "attacker_amount".into(),
            match i.action {
                Action::Zap { amount_sats } => amount_sats,
                _ => 0,
            }
            .to_string(),
        );
        v.insert("new_total".into(), amount.to_string());
        v.insert("new_amount".into(), amount.to_string());
        let required = match t.decision {
            Decision::Full { required_sats } | Decision::BelowMinimum { required_sats } => {
                required_sats
            }
            _ => 0,
        };
        v.insert("required_sats".into(), required.to_string());
        v.insert(
            "difference".into(),
            required
                .saturating_sub(match i.action {
                    Action::Zap { amount_sats } => amount_sats,
                    _ => 0,
                })
                .to_string(),
        );
        v.insert(
            "active_members".into(),
            snapshot.roster.active_count().to_string(),
        );
        v.insert(
            "max_members".into(),
            snapshot.settings.max_members.to_string(),
        );
        v.insert("spots_remaining".into(), t.spots_remaining.to_string());
        v.insert(
            "pool_percent".into(),
            snapshot.settings.member_percent.to_string(),
        );
        if let Some((key, p, sats)) = &victim {
            v.insert(
                "victim_name".into(),
                if mentions {
                    mention(key)?
                } else {
                    display_name(p)
                },
            );
            v.insert("victim_amount".into(), sats.to_string());
        } else {
            v.insert("victim_name".into(), "the herd".into());
            v.insert("victim_amount".into(), "0".into());
        }
        score(v, amount, awards, minimum, share)?;
        let added =
            10 * u128::from(t.increment_sats) + u128::from(minimum) * u128::from(t.social_points);
        v.insert("earned_points".into(), ratio(added, u128::from(minimum))?);
    }
    let compose = |values: &BTreeMap<String, String>| -> Result<Option<String>> {
        let Some(base) = catalog.render(category, &identity, values)? else {
            return Ok(None);
        };
        let mut parts = vec![base];
        append(
            &mut parts,
            catalog,
            match i.action {
                Action::Zap { .. } => "cyber_herd_zap_interaction",
                Action::Repost => "cyber_herd_repost_interaction",
                Action::Like => "cyber_herd_like_interaction",
            },
            &identity,
            values,
        )?;
        if accepted {
            append(
                &mut parts,
                catalog,
                "cyber_herd_interaction_score",
                &identity,
                values,
            )?;
            append(
                &mut parts,
                catalog,
                "cyber_herd_current_score",
                &identity,
                values,
            )?;
        } else {
            append(
                &mut parts,
                catalog,
                "cyber_herd_unadmitted_score",
                &identity,
                values,
            )?;
        }
        append(
            &mut parts,
            catalog,
            if t.spots_remaining == 0 {
                "cyber_herd_space_full"
            } else {
                "cyber_herd_space_open"
            },
            &identity,
            values,
        )?;
        if accepted && input.allocation_changed {
            let footer = roster_footer(catalog, &identity, snapshot)?;
            if !footer.is_empty() {
                parts.push(footer);
            }
        }
        Ok(Some(bounded(parts)?))
    };
    let (Some(nostr), Some(overlay)) = (compose(&public)?, compose(&human)?) else {
        return Ok(None);
    };
    Ok(Some(Rendered {
        identity,
        cycle: event.cycle,
        member: i.pubkey.clone(),
        conversation: input.conversation,
        context,
        category: category.into(),
        nostr,
        overlay,
        accepted,
        requires_joining: false,
        additional_members,
        broadcast: None,
    }))
}

fn payout(event: &StoredEvent, catalog: &Catalog) -> Result<Option<Rendered>> {
    let input: ResultEvent = serde_json::from_str(&event.snapshot_json)?;
    let row = &input.breakdown;
    let category = if let Some(result) = &input.result {
        result.validate()?;
        if event.event_type != "payout_result" || row.pubkey != result.member || row.skip.is_some()
        {
            bail!("payout message changed frozen member");
        }
        match result.result {
            crate::payout::results::Status::Completed { .. } => "cyber_herd_treats",
            crate::payout::results::Status::Failed => "cyber_herd_delivery_failed",
            crate::payout::results::Status::Unsubmitted => "cyber_herd_unsubmitted",
        }
    } else {
        if event.event_type != "payout_skipped" {
            bail!("payout message lacks verified result");
        }
        match row.skip {
            Some(
                super::rewards::Skip::ZeroShare
                | super::rewards::Skip::BelowMinimum
                | super::rewards::Skip::RecipientMinimum,
            ) => "cyber_herd_no_treats",
            Some(
                super::rewards::Skip::RecipientMaximum | super::rewards::Skip::RecipientUnavailable,
            ) => "cyber_herd_unsubmitted",
            _ => return Ok(None),
        }
    };
    let profile = row
        .profile
        .as_ref()
        .context("payout message lost frozen profile")?;
    let identity = conversation::identity(&event.owner, &row.pubkey, &event.source_id)?;
    let compose = |mentions: bool| -> Result<Option<String>> {
        let mut values = values(profile, &row.pubkey, mentions)?;
        values.insert(
            "new_amount".into(),
            match input.result.as_ref().map(|r| &r.result) {
                Some(crate::payout::results::Status::Completed { net_sats, .. }) => *net_sats,
                _ => 0,
            }
            .to_string(),
        );
        let Some(base) = catalog.render(category, &identity, &values)? else {
            return Ok(None);
        };
        let mut parts = vec![base];
        if let Some(basis) = row.score_basis {
            score(
                &mut values,
                row.amount_sats,
                row.awards,
                basis,
                row.share_micro_percent,
            )?;
            append(
                &mut parts,
                catalog,
                "cyber_herd_treat_breakdown",
                &identity,
                &values,
            )?;
        }
        Ok(Some(bounded(parts)?))
    };
    let (Some(nostr), Some(overlay)) = (compose(true)?, compose(false)?) else {
        return Ok(None);
    };
    let context = row
        .conversation
        .as_ref()
        .map(|c| c.context.clone())
        .unwrap_or_default();
    context.validate(&event.owner)?;
    Ok(Some(Rendered {
        identity,
        cycle: event.cycle,
        member: row.pubkey.clone(),
        conversation: row.conversation.clone(),
        context,
        category: category.into(),
        nostr,
        overlay,
        accepted: false,
        requires_joining: true,
        additional_members: vec![],
        broadcast: None,
    }))
}

fn append(
    parts: &mut Vec<String>,
    catalog: &Catalog,
    category: &str,
    identity: &str,
    values: &BTreeMap<String, String>,
) -> Result<()> {
    if let Some(text) = catalog.render(category, identity, values)? {
        parts.push(text);
    }
    Ok(())
}
fn bounded(parts: Vec<String>) -> Result<String> {
    let text = parts
        .into_iter()
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n");
    if text.len() > 32768 {
        bail!("member message exceeds size bound");
    }
    Ok(text)
}
fn values(profile: &Profile, member: &str, mentions: bool) -> Result<BTreeMap<String, String>> {
    let name = if mentions {
        mention(member)?
    } else {
        display_name(profile)
    };
    Ok(BTreeMap::from([
        ("name".into(), name.clone()),
        ("member_name".into(), name.clone()),
        ("attacker_name".into(), name),
    ]))
}
pub fn display_name(profile: &Profile) -> String {
    let name = profile.name.trim();
    if name.is_empty() {
        "Anon".into()
    } else {
        name.replace("nostr:", "nostr∶")
            .replace("npub1", "npub\u{200b}1")
            .replace("nprofile1", "nprofile\u{200b}1")
    }
}
fn mention(member: &str) -> Result<String> {
    crate::nostr_event::validate_public_key(member)?;
    Ok(format!(
        "nostr:{}",
        bitcoin::bech32::encode::<bitcoin::bech32::Bech32>(
            bitcoin::bech32::Hrp::parse("npub")?,
            &hex::decode(member)?
        )?
    ))
}
pub fn ratio(numerator: u128, denominator: u128) -> Result<String> {
    if denominator == 0 {
        bail!("zero display denominator");
    }
    let whole = numerator / denominator;
    let fraction = (numerator % denominator)
        .checked_mul(1_000_000)
        .context("display overflow")?
        / denominator;
    if fraction == 0 {
        Ok(whole.to_string())
    } else {
        Ok(format!("{whole}.{fraction:06}")
            .trim_end_matches('0')
            .to_owned())
    }
}
fn score(
    v: &mut BTreeMap<String, String>,
    amount: u64,
    awards: SocialAwards,
    basis: u64,
    share: u64,
) -> Result<()> {
    if basis == 0 {
        bail!("missing frozen score basis");
    }
    v.insert("zap_sats".into(), amount.to_string());
    v.insert(
        "zap_points".into(),
        ratio(10 * u128::from(amount), u128::from(basis))?,
    );
    v.insert(
        "repost_points".into(),
        (3 * u8::from(awards.repost)).to_string(),
    );
    v.insert("like_points".into(), u8::from(awards.like).to_string());
    v.insert(
        "total_points".into(),
        ratio(
            shares::point_units(amount, awards, basis)?,
            u128::from(basis),
        )?,
    );
    v.insert("share_percent".into(), ratio(u128::from(share), 1_000_000)?);
    Ok(())
}
fn roster_footer(catalog: &Catalog, identity: &str, snapshot: &Snapshot) -> Result<String> {
    let mut parts = vec![];
    let shares = snapshot.roster.shares(&snapshot.settings)?;
    let values = BTreeMap::from([(
        "pool_percent".into(),
        snapshot.settings.member_percent.to_string(),
    )]);
    append(
        &mut parts,
        catalog,
        "cyber_herd_share_header",
        identity,
        &values,
    )?;
    for (key, share) in shares {
        let member = &snapshot.roster.members[&key];
        let values = BTreeMap::from([
            ("name".into(), display_name(&member.profile)),
            ("share_percent".into(), ratio(u128::from(share), 1_000_000)?),
        ]);
        append(
            &mut parts,
            catalog,
            "cyber_herd_share_row",
            identity,
            &values,
        )?;
    }
    bounded(parts)
}

impl Rendered {
    pub(crate) fn validate_broadcast(&self, owner: &str) -> Result<()> {
        self.broadcast
            .as_ref()
            .context("feeding broadcast proof missing")?
            .validate(owner, &self.identity)?;
        self.context.validate(owner)?;
        if self.category != "feeder_trigger"
            || self.member != owner
            || self.conversation.is_some()
            || self.context.note.is_some()
            || self.accepted
            || self.requires_joining
            || !self.additional_members.is_empty()
            || self.nostr.trim().is_empty()
            || self.nostr.len() > 32768
            || self.overlay.trim().is_empty()
            || self.overlay.len() > 32768
        {
            bail!("invalid confirmed feeding broadcast envelope");
        }
        Ok(())
    }
}

/// Optional wording only; the store atomically claims its once-per-cycle slot.
pub(crate) fn repost_hint(
    event: &StoredEvent,
    catalog: &Catalog,
    message: &Rendered,
) -> Result<Option<(String, String)>> {
    if event.event_type != "payout_skipped" || message.category != "cyber_herd_no_treats" {
        return Ok(None);
    }
    crate::nostr_event::validate_hex(&message.identity, 32)?;
    let input: ResultEvent = serde_json::from_str(&event.snapshot_json)?;
    if input.breakdown.awards.repost
        || input.breakdown.pubkey != message.member
        || u8::from_str_radix(&message.identity[..2], 16)? % 3 != 0
    {
        return Ok(None);
    }
    let profile = input
        .breakdown
        .profile
        .as_ref()
        .context("no-treat hint lacks frozen profile")?;
    let public = catalog.render(
        "repost_hint",
        &message.identity,
        &values(profile, &message.member, true)?,
    )?;
    let human = catalog.render(
        "repost_hint",
        &message.identity,
        &values(profile, &message.member, false)?,
    )?;
    Ok(public.zip(human))
}

fn operator_member(event: &StoredEvent, catalog: &Catalog) -> Result<Option<Rendered>> {
    let notice: super::operator::Notice = serde_json::from_str(&event.snapshot_json)?;
    if notice.snapshot.owner != event.owner
        || notice.snapshot.cycle != event.cycle
        || !matches!(
            notice.category.as_str(),
            "member_added" | "member_removed" | "member_score_updated"
        )
    {
        bail!("invalid operator member notice");
    }
    notice.context.validate(&event.owner)?;
    if let Some(c) = &notice.conversation {
        c.validate(&event.owner)?;
    }
    let identity = conversation::identity(&event.owner, &notice.member, &event.source_id)?;
    let amount = notice
        .snapshot
        .roster
        .members
        .get(&notice.member)
        .map_or(0, |m| m.amount_sats);
    let shares = notice.snapshot.roster.shares(&notice.snapshot.settings)?;
    let compose = |mentions: bool| -> Result<Option<String>> {
        let mut v = values(&notice.profile, &notice.member, mentions)?;
        v.insert("new_total".into(), amount.to_string());
        score(
            &mut v,
            amount,
            notice
                .snapshot
                .roster
                .awards
                .get(&notice.member)
                .copied()
                .unwrap_or_default(),
            notice.snapshot.settings.minimum_sats.max(1),
            shares.get(&notice.member).copied().unwrap_or(0),
        )?;
        let Some(body) = catalog.render(&notice.category, &identity, &v)? else {
            return Ok(None);
        };
        let mut parts = vec![body];
        if notice.allocation_changed {
            let footer = roster_footer(catalog, &identity, &notice.snapshot)?;
            if !footer.is_empty() {
                parts.push(footer);
            }
        }
        Ok(Some(bounded(parts)?))
    };
    let (Some(nostr), Some(overlay)) = (compose(true)?, compose(false)?) else {
        return Ok(None);
    };
    Ok(Some(Rendered {
        identity,
        cycle: event.cycle,
        member: notice.member,
        conversation: notice.conversation,
        context: notice.context,
        category: notice.category,
        nostr,
        overlay,
        accepted: notice.activation,
        requires_joining: !notice.activation,
        additional_members: vec![],
        broadcast: None,
    }))
}
