//! Signed social input never proves a payment. Raw inbox entries survive network
//! failures; a frozen domain envelope is reused on every deferred replay.
use anyhow::{Context, Result, bail};

use super::{
    membership::{Action, FeederAvailability, Profile, Target},
    store::{Interaction, Receipt, Status, Store},
    targets::{AuthenticatedProfile, ConversationTarget, Targets, stream_coordinate},
};
use crate::nostr_event::{Coordinate, VerifiedEvent, validate_hex, validate_public_key};

#[derive(Debug, Clone)]
pub struct SocialEvent {
    event: VerifiedEvent,
    action: Action,
    event_id: Option<String>,
    coordinate: Option<String>,
    kind_hint: Option<u64>,
    author_hint: Option<String>,
    embedded: Option<VerifiedEvent>,
}

impl SocialEvent {
    pub fn parse(event: VerifiedEvent, owner: &str, now: i64) -> Result<Option<Self>> {
        validate_public_key(owner)?;
        event.require_not_future(now)?;
        let signed = event.event();
        if signed.pubkey == owner || !matches!(signed.kind, 6 | 7 | 16) {
            return Ok(None);
        }
        if signed.kind == 7 && !matches!(signed.content.as_str(), "" | "+") {
            return Ok(None);
        }
        let event_id = if signed.kind == 7 {
            // NIP-25 explicitly defines the last e as the actual target. Earlier
            // thread references must never turn an unrelated reaction into an award.
            last_tag(&event, "e")?
        } else {
            event.singleton_tag("e", false)?.map(str::to_owned)
        };
        if let Some(id) = &event_id {
            validate_hex(id, 32)?;
        }
        let coordinate = event.singleton_tag("a", false)?.map(str::to_owned);
        if let Some(coordinate) = &coordinate {
            let _: Coordinate = coordinate.parse()?;
        }
        let kind_hint = event
            .singleton_tag("k", false)?
            .map(|value| -> Result<u64> {
                let kind = value.parse::<u64>()?;
                if kind > 65535 || kind.to_string() != value {
                    bail!("invalid social target kind");
                }
                Ok(kind)
            })
            .transpose()?;
        let author_hint = last_tag(&event, "p")?;
        if let Some(author) = &author_hint {
            validate_public_key(author)?;
        }
        let embedded = if signed.kind != 7 && !signed.content.is_empty() {
            Some(
                VerifiedEvent::from_json(&signed.content)
                    .context("repost body must contain its signed target")?,
            )
        } else {
            None
        };
        if signed.kind == 7 && event_id.is_none() {
            bail!("reaction requires its target event ID");
        }
        if signed.kind == 6 && (event_id.is_none() || coordinate.is_some()) {
            bail!("kind-6 repost requires one note ID without a stream coordinate");
        }
        if signed.kind == 16 && coordinate.is_none() && embedded.is_none() {
            bail!("version-specific generic repost requires the full signed target");
        }
        if let Some(target) = &embedded {
            target.require_not_future(now)?;
            if event_id.as_ref().is_some_and(|id| id != &target.event().id)
                || kind_hint.is_some_and(|kind| kind != target.event().kind)
                || author_hint
                    .as_ref()
                    .is_some_and(|key| key != &target.event().pubkey)
                || coordinate
                    .as_ref()
                    .is_some_and(|a| stream_coordinate(target).ok().as_ref() != Some(a))
            {
                bail!("repost references contradict its signed target");
            }
        }
        let event_id =
            event_id.or_else(|| embedded.as_ref().map(|target| target.event().id.clone()));
        let result = Self {
            action: if signed.kind == 7 {
                Action::Like
            } else {
                Action::Repost
            },
            event,
            event_id,
            coordinate,
            kind_hint,
            author_hint,
            embedded,
        };
        if result
            .kind_hint
            .is_some_and(|kind| !result.accepts_kind(kind))
            || result
                .embedded
                .as_ref()
                .is_some_and(|target| !result.accepts_kind(target.event().kind))
        {
            return Ok(None);
        }
        Ok(Some(result))
    }

    pub fn event(&self) -> &VerifiedEvent {
        &self.event
    }
    pub fn embedded(&self) -> Option<&VerifiedEvent> {
        self.embedded.as_ref()
    }
    pub fn event_id(&self) -> Option<&str> {
        self.event_id.as_deref()
    }
    pub fn coordinate(&self) -> Option<&str> {
        self.coordinate.as_deref()
    }
    pub fn action(&self) -> Action {
        self.action
    }

    fn accepts_kind(&self, kind: u64) -> bool {
        match self.event.event().kind {
            6 => kind == 1,
            16 => matches!(kind, 30311 | 9041),
            7 => matches!(kind, 1 | 30311 | 9041),
            _ => false,
        }
    }

    pub fn classify(
        &self,
        targets: &Targets,
        discovery_available: bool,
    ) -> (Target, Option<ConversationTarget>) {
        if !discovery_available {
            // Cached targets are useful evidence, but an incomplete refresh
            // cannot prove that an announcement/stream is still current or stale.
            return (Target::Unavailable, None);
        }
        let id = self.event_id.as_deref();
        let target = id.and_then(|id| targets.event(id));
        // Even when an a tag is current, an accompanying e must authenticate as
        // a version of that same target. A made-up e is not an alias capability.
        if id.is_some() && target.is_none() {
            return (Target::Unavailable, None);
        }
        if let Some(target) = target {
            if target.event().kind == 30311
                && !targets.authenticated_stream_version(&target.event().id)
            {
                return (Target::Unavailable, None);
            }
            if !self.accepts_kind(target.event().kind)
                || self
                    .kind_hint
                    .is_some_and(|kind| kind != target.event().kind)
                || self
                    .author_hint
                    .as_ref()
                    .is_some_and(|author| author != &target.event().pubkey)
                || self
                    .coordinate
                    .as_ref()
                    .is_some_and(|a| stream_coordinate(target).ok().as_ref() != Some(a))
            {
                return (Target::Other, None);
            }
        } else if let Some(coordinate) = self.coordinate.as_deref() {
            if self.event.event().kind != 16
                || coordinate
                    .parse::<Coordinate>()
                    .map_or(true, |value| value.kind != 30311)
            {
                return (Target::Other, None);
            }
        }
        let ids = id.map(str::to_owned).into_iter().collect::<Vec<_>>();
        let coordinates = self.coordinate.clone().into_iter().collect::<Vec<_>>();
        let result = targets.classify(&ids, &coordinates, discovery_available);
        if let Some(context) = &result.1 {
            if self.author_hint.as_ref().is_some_and(|author| {
                targets
                    .event(&context.event_id)
                    .is_none_or(|target| author != &target.event().pubkey)
            }) {
                return (Target::Other, None);
            }
        }
        result
    }
}

fn last_tag(event: &VerifiedEvent, name: &str) -> Result<Option<String>> {
    event
        .event()
        .tags
        .iter()
        .rev()
        .find(|tag| tag[0] == name)
        .map(|tag| {
            tag.get(1)
                .filter(|value| !value.is_empty())
                .cloned()
                .context("empty social reference")
        })
        .transpose()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SocialStep {
    AwaitingProfile,
    Recorded(Receipt),
}

pub struct SocialEnvironment<'a> {
    pub targets: &'a Targets,
    pub discovery_available: bool,
    pub feeder: FeederAvailability,
    pub profile: Option<&'a AuthenticatedProfile>,
    pub now: i64,
}

pub async fn stage(store: &Store, owner: &str, event: VerifiedEvent, now: i64) -> Result<bool> {
    let Some(social) = SocialEvent::parse(event, owner, now)? else {
        return Ok(false);
    };
    store.enqueue_social(owner, social.event(), now).await
}

/// Caller refreshes targets on every recovery batch, particularly after override
/// resumes. A missing profile is a retryable lookup, never a negative address.
pub async fn process(
    store: &Store,
    owner: &str,
    social: &SocialEvent,
    environment: SocialEnvironment<'_>,
) -> Result<SocialStep> {
    let SocialEnvironment {
        targets,
        discovery_available,
        feeder,
        profile,
        now,
    } = environment;
    validate_public_key(owner)?;
    social.event.require_not_future(now)?;
    if targets.owner() != owner || social.event.event().pubkey == owner {
        bail!("social processing owner does not match its authenticated scope");
    }
    let (target, context) = social.classify(targets, discovery_available);
    let source_id = format!("nostr:{}", social.event.event().id);
    let interaction = match store.frozen_interaction(owner, &source_id).await? {
        Some(frozen) => {
            if frozen.source_hash != social.event.event().id
                || frozen.pubkey != social.event.event().pubkey
                || frozen.action != social.action
            {
                bail!("frozen social envelope contradicts its authenticated event");
            }
            frozen
        }
        None => {
            let profile = match profile {
                Some(profile) if profile.event().event().pubkey == social.event.event().pubkey => {
                    profile.event().require_not_future(now)?;
                    profile.profile().clone()
                }
                Some(_) => bail!("social profile belongs to another signer"),
                None if target == Target::Other && feeder == FeederAvailability::Ready => {
                    Profile::default()
                }
                None => {
                    store
                        .finish_social(owner, &social.event.event().id, false, now)
                        .await?;
                    return Ok(SocialStep::AwaitingProfile);
                }
            };
            Interaction {
                source_id,
                source_hash: social.event.event().id.clone(),
                pubkey: social.event.event().pubkey.clone(),
                profile,
                action: social.action,
                occurred_at: social.event.event().created_at as i64,
            }
        }
    };
    let context = context.map(|matched| targets.interaction_context(matched));
    let receipt = store
        .process_with_context(owner, &interaction, target, feeder, context.as_ref())
        .await?;
    store
        .finish_social(
            owner,
            &social.event.event().id,
            receipt.status != Status::Deferred,
            now,
        )
        .await?;
    Ok(SocialStep::Recorded(receipt))
}
