//! Authenticated project targets and profiles. A host tag or event-supplied URL
//! never grants provider authority, and a relay receipt never grants money.
use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context, Result, bail};
use bitcoin::secp256k1::{Message, Secp256k1, XOnlyPublicKey, schnorr::Signature};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::{
    calendar::DayWindow,
    membership::{Profile, Target, validate_lud16},
};
use crate::{
    nostr::SignedNostrEvent,
    nostr_event::{Coordinate, VerifiedEvent, validate_hex, validate_public_key},
    public_network,
};

const STREAM_FRESH_SECONDS: i64 = 3600;
const PROVIDER_TTL: i64 = 1800;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TargetPolicy {
    pub project_pubkey: String,
    pub tracked_tags: Vec<String>,
}

impl TargetPolicy {
    pub fn validate(&self) -> Result<()> {
        validate_public_key(&self.project_pubkey)?;
        if self.tracked_tags.is_empty() || self.tracked_tags.len() > 8 {
            bail!("CyberHerd requires a bounded announcement-tag allowlist");
        }
        for tag in &self.tracked_tags {
            let value = tag.trim_start_matches('#');
            if value.is_empty()
                || value.len() > 64
                || !value
                    .chars()
                    .all(|c| c.is_alphanumeric() || c == '_' || c == '-')
            {
                bail!("invalid CyberHerd announcement tag");
            }
        }
        Ok(())
    }

    pub fn is_announcement(&self, event: &VerifiedEvent, day: &DayWindow) -> bool {
        self.is_tagged_project_note(event) && day.contains(event.event().created_at as i64)
    }

    pub fn policy_key(&self) -> Result<String> {
        self.validate()?;
        let tags: BTreeSet<_> = self
            .tracked_tags
            .iter()
            .map(|tag| tag.trim_start_matches('#').to_lowercase())
            .collect();
        Ok(hex::encode(Sha256::digest(serde_json::to_vec(&tags)?)))
    }

    pub fn is_tagged_project_note(&self, event: &VerifiedEvent) -> bool {
        let signed = event.event();
        if signed.kind != 1 || signed.pubkey != self.project_pubkey {
            return false;
        }
        let expected: BTreeSet<_> = self
            .tracked_tags
            .iter()
            .map(|tag| tag.trim_start_matches('#').to_lowercase())
            .collect();
        if event
            .tag_values("t")
            .any(|tag| expected.contains(&tag.trim_start_matches('#').to_lowercase()))
        {
            return true;
        }
        signed.content.split('#').skip(1).any(|tail| {
            let tag: String = tail
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_' || *c == '-')
                .collect();
            !tag.is_empty() && expected.contains(&tag.to_lowercase())
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum StreamAuthority {
    ProjectAuthor,
    HostProof,
    Provider {
        service: String,
        pubkey: String,
        valid_until: i64,
    },
}

#[derive(Debug, Clone)]
pub struct AuthenticatedStream {
    owner: String,
    event: VerifiedEvent,
    coordinate: String,
    authority: StreamAuthority,
}

impl AuthenticatedStream {
    pub fn event(&self) -> &VerifiedEvent {
        &self.event
    }
    pub fn coordinate(&self) -> &str {
        &self.coordinate
    }
    pub fn authority(&self) -> &StreamAuthority {
        &self.authority
    }
    pub fn owner(&self) -> &str {
        &self.owner
    }
    pub fn is_live(&self, now: i64) -> bool {
        self.authority_current(now)
            && self.event.event().created_at as i64 >= now.saturating_sub(STREAM_FRESH_SECONDS)
            && self.event.event().created_at as i64 <= now.saturating_add(300)
            && self.event.singleton_tag("status", false).ok().flatten() == Some("live")
            && match numeric_tag(&self.event, "ends") {
                Ok(Some(end)) => end > now,
                Ok(None) => true,
                Err(_) => false,
            }
    }
    pub fn recently_ended(&self, now: i64) -> bool {
        self.authority_current(now)
            && self.event.singleton_tag("status", false).ok().flatten() == Some("ended")
            && (now.saturating_sub(900)..=now).contains(&(self.event.event().created_at as i64))
    }
    fn authority_current(&self, now: i64) -> bool {
        match &self.authority {
            StreamAuthority::Provider { valid_until, .. } => *valid_until > now,
            _ => true,
        }
    }
    pub(crate) fn restore(
        event: VerifiedEvent,
        owner: &str,
        authority: StreamAuthority,
        now: i64,
    ) -> Result<Self> {
        let coordinate = stream_coordinate(&event)?;
        event.require_not_future(now)?;
        let valid = match &authority {
            StreamAuthority::ProjectAuthor => event.event().pubkey == owner,
            StreamAuthority::HostProof => valid_host_proof(&event, owner, &coordinate),
            StreamAuthority::Provider {
                service,
                pubkey,
                valid_until,
            } => {
                provider_origin(service).is_some()
                    && event
                        .singleton_tag("service", false)?
                        .is_some_and(|value| value.trim_end_matches('/') == service)
                    && has_host(&event, owner)
                    && pubkey == &event.event().pubkey
                    && *valid_until >= 0
            }
        };
        if !valid {
            bail!("persisted stream authority contradicts its signed event");
        }
        Ok(Self {
            owner: owner.into(),
            event,
            coordinate,
            authority,
        })
    }
}

#[derive(Debug, Clone)]
pub enum StreamAuthentication {
    Accepted(Box<AuthenticatedStream>),
    Rejected,
    Unavailable,
}

#[derive(Debug, Clone, Default)]
pub struct ProviderCache {
    keys: BTreeMap<(String, String), (String, i64)>,
    attempted: BTreeMap<(String, String), i64>,
}

impl ProviderCache {
    pub async fn authenticate(
        &mut self,
        event: VerifiedEvent,
        owner: &str,
        now: i64,
    ) -> Result<StreamAuthentication> {
        validate_public_key(owner)?;
        let coordinate = stream_coordinate(&event)?;
        event.require_not_future(now)?;
        if event.event().pubkey == owner {
            return Ok(StreamAuthentication::Accepted(Box::new(
                AuthenticatedStream {
                    owner: owner.into(),
                    event,
                    coordinate,
                    authority: StreamAuthority::ProjectAuthor,
                },
            )));
        }
        if valid_host_proof(&event, owner, &coordinate) {
            return Ok(StreamAuthentication::Accepted(Box::new(
                AuthenticatedStream {
                    owner: owner.into(),
                    event,
                    coordinate,
                    authority: StreamAuthority::HostProof,
                },
            )));
        }
        if !has_host(&event, owner) {
            return Ok(StreamAuthentication::Rejected);
        }
        let Some(service) = event.singleton_tag("service", false)? else {
            return Ok(StreamAuthentication::Rejected);
        };
        let service = service.trim_end_matches('/').to_owned();
        let Some(origin) = provider_origin(&service) else {
            return Ok(StreamAuthentication::Rejected);
        };
        let cache_key = (service.clone(), owner.to_owned());
        if !self
            .keys
            .get(&cache_key)
            .is_some_and(|(key, expires)| key == &event.event().pubkey && *expires > now)
        {
            // A burst of forged publishers must not multiply fixed-origin HTTP
            // requests, including during provider outages. Callers use one
            // observation timestamp for a bounded discovery batch. A mismatch
            // still refreshes on the next batch to allow real key rotation.
            if self.attempted.get(&cache_key) == Some(&now) {
                return Ok(
                    if self
                        .keys
                        .get(&cache_key)
                        .is_some_and(|(_, until)| *until > now)
                    {
                        StreamAuthentication::Rejected
                    } else {
                        StreamAuthentication::Unavailable
                    },
                );
            }
            if self.attempted.len() >= 64 && !self.attempted.contains_key(&cache_key) {
                self.attempted
                    .retain(|_, at| *at >= now.saturating_sub(PROVIDER_TTL));
            }
            if self.attempted.len() >= 64 && !self.attempted.contains_key(&cache_key) {
                bail!("stream provider request budget reached");
            }
            self.attempted.insert(cache_key.clone(), now);
            let endpoint = format!("{origin}/.well-known/lnurlp/{owner}");
            let bytes = match public_network::get_json(&endpoint, 32_768).await {
                Ok(bytes) => bytes,
                Err(_) => return Ok(StreamAuthentication::Unavailable),
            };
            let value: serde_json::Value = match serde_json::from_slice(&bytes) {
                Ok(value) => value,
                Err(_) => return Ok(StreamAuthentication::Unavailable),
            };
            let Some(key) = value["nostrPubkey"].as_str() else {
                return Ok(StreamAuthentication::Rejected);
            };
            if validate_public_key(key).is_err() {
                return Ok(StreamAuthentication::Rejected);
            }
            if self.keys.len() >= 64 && !self.keys.contains_key(&cache_key) {
                self.keys.retain(|_, (_, until)| *until > now);
            }
            if self.keys.len() >= 64 && !self.keys.contains_key(&cache_key) {
                bail!("stream provider cache bound reached");
            }
            self.keys.insert(
                cache_key.clone(),
                (key.to_owned(), now.saturating_add(PROVIDER_TTL)),
            );
        }
        let (pubkey, valid_until) = self
            .keys
            .get(&cache_key)
            .context("provider cache lost its verified response")?;
        if pubkey != &event.event().pubkey {
            return Ok(StreamAuthentication::Rejected);
        }
        let authority = StreamAuthority::Provider {
            service,
            pubkey: pubkey.clone(),
            valid_until: *valid_until,
        };
        Ok(StreamAuthentication::Accepted(Box::new(
            AuthenticatedStream {
                owner: owner.into(),
                event,
                coordinate,
                authority,
            },
        )))
    }
}

fn provider_origin(service: &str) -> Option<&'static str> {
    match service {
        "https://api-core.zap.stream/api/v1" => Some("https://api-core.zap.stream"),
        "https://api-uk.zap.stream/api/v1" => Some("https://api-uk.zap.stream"),
        _ => None,
    }
}

fn has_host(event: &VerifiedEvent, owner: &str) -> bool {
    event.event().tags.iter().any(|tag| {
        tag.len() >= 4 && tag[0] == "p" && tag[1] == owner && tag[3].eq_ignore_ascii_case("host")
    })
}

fn valid_host_proof(event: &VerifiedEvent, owner: &str, coordinate: &str) -> bool {
    let Ok(key) = owner.parse::<XOnlyPublicKey>() else {
        return false;
    };
    let hash: [u8; 32] = Sha256::digest(coordinate.as_bytes()).into();
    event
        .event()
        .tags
        .iter()
        .filter(|tag| {
            tag.len() >= 5
                && tag[0] == "p"
                && tag[1] == owner
                && tag[3].eq_ignore_ascii_case("host")
        })
        .any(|tag| {
            if validate_hex(&tag[4], 64).is_err() {
                return false;
            }
            let Ok(signature) = tag[4].parse::<Signature>() else {
                return false;
            };
            Secp256k1::verification_only()
                .verify_schnorr(&signature, &Message::from_digest(hash), &key)
                .is_ok()
        })
}

pub fn stream_coordinate(event: &VerifiedEvent) -> Result<String> {
    if event.event().kind != 30311 {
        bail!("stream target requires kind 30311");
    }
    let id = event
        .singleton_tag("d", true)?
        .context("stream has no stable identifier")?;
    let value = format!("30311:{}:{id}", event.event().pubkey);
    value.parse::<Coordinate>()?;
    Ok(value)
}

pub fn goal_reference(event: &VerifiedEvent) -> Result<Option<String>> {
    event
        .singleton_tag("goal", false)?
        .map(|id| {
            validate_hex(id, 32)?;
            Ok(id.to_owned())
        })
        .transpose()
}

fn numeric_tag(event: &VerifiedEvent, name: &str) -> Result<Option<i64>> {
    event
        .singleton_tag(name, false)?
        .map(|value| {
            let number = value.parse::<i64>()?;
            if number < 0 || number.to_string() != value {
                bail!("invalid numeric target tag");
            }
            Ok(number)
        })
        .transpose()
}

fn valid_goal(event: &VerifiedEvent, owner: &str, now: i64) -> bool {
    event.event().kind == 9041
        && event.event().pubkey == owner
        && event.require_not_future(now).is_ok()
        && numeric_tag(event, "amount")
            .ok()
            .flatten()
            .is_some_and(|amount| amount > 0)
        && event
            .event()
            .tags
            .iter()
            .any(|tag| tag.len() > 1 && tag[0] == "relays")
        && match numeric_tag(event, "closed_at") {
            Ok(Some(at)) => at > now,
            Ok(None) => true,
            Err(_) => false,
        }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ConversationTarget {
    pub event_id: String,
    pub kind: u64,
    pub coordinate: Option<String>,
    pub payee_pubkey: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct InteractionContext {
    matched: ConversationTarget,
    destinations: Vec<ConversationTarget>,
}

impl InteractionContext {
    pub fn matched(&self) -> &ConversationTarget {
        &self.matched
    }
    pub fn destinations(&self) -> &[ConversationTarget] {
        &self.destinations
    }
}

#[derive(Debug, Clone)]
pub struct Targets {
    policy: TargetPolicy,
    announcement: Option<VerifiedEvent>,
    stream: Option<AuthenticatedStream>,
    goal: Option<VerifiedEvent>,
    known: BTreeMap<String, VerifiedEvent>,
    aliases: BTreeMap<String, String>,
    uncertain: BTreeSet<String>,
}

impl Targets {
    pub fn project(
        policy: TargetPolicy,
        day: &DayWindow,
        mut events: Vec<VerifiedEvent>,
        streams: Vec<AuthenticatedStream>,
        uncertain: BTreeSet<String>,
        now: i64,
    ) -> Result<Self> {
        policy.validate()?;
        day.validate()?;
        if !day.contains(now) || events.len() > 4096 || streams.len() > 256 || uncertain.len() > 256
        {
            bail!("invalid target projection bounds or calendar");
        }
        let stream = select_active_stream(&streams, &policy.project_pubkey, now)?;
        let mut aliases = BTreeMap::new();
        for stream in streams {
            if stream.owner() != policy.project_pubkey {
                bail!("stream authority belongs to another project owner");
            }
            aliases.insert(stream.event.event().id.clone(), stream.coordinate.clone());
            events.push(stream.event);
        }
        let reference = stream
            .as_ref()
            .and_then(|value| goal_reference(&value.event).ok().flatten());
        let known: BTreeMap<_, _> = events
            .into_iter()
            .filter(|event| event.require_not_future(now).is_ok())
            .map(|event| (event.event().id.clone(), event))
            .collect();
        let mut notes: Vec<_> = known
            .values()
            .filter(|event| policy.is_announcement(event, day))
            .cloned()
            .collect();
        if let Some(reference) = reference.as_ref() {
            let matching: Vec<_> = notes
                .iter()
                .filter(|event| goal_reference(event).ok().flatten().as_ref() == Some(reference))
                .cloned()
                .collect();
            if !matching.is_empty() {
                notes = matching;
            }
        }
        let announcement = notes.into_iter().max_by(|a, b| {
            a.event()
                .created_at
                .cmp(&b.event().created_at)
                .then_with(|| a.event().id.cmp(&b.event().id))
        });
        let reference = reference.or(announcement
            .as_ref()
            .and_then(|event| goal_reference(event).ok().flatten()));
        let goal = reference
            .and_then(|id| known.get(&id))
            .filter(|event| valid_goal(event, &policy.project_pubkey, now))
            .cloned();
        Ok(Self {
            policy,
            announcement,
            stream,
            goal,
            known,
            aliases,
            uncertain,
        })
    }

    pub fn current(&self) -> Vec<ConversationTarget> {
        let mut result = Vec::new();
        if let Some(note) = &self.announcement {
            result.push(self.context(note, None));
        }
        if let Some(stream) = &self.stream {
            result.push(self.context(&stream.event, Some(stream.coordinate.clone())));
        }
        if let Some(goal) = &self.goal {
            result.push(self.context(goal, None));
        }
        result
    }

    pub fn event(&self, id: &str) -> Option<&VerifiedEvent> {
        self.known.get(id)
    }

    pub fn owner(&self) -> &str {
        &self.policy.project_pubkey
    }

    pub fn authenticated_stream_version(&self, id: &str) -> bool {
        self.aliases.contains_key(id)
    }

    pub(crate) fn interaction_context(&self, matched: ConversationTarget) -> InteractionContext {
        InteractionContext {
            matched,
            destinations: self
                .current()
                .into_iter()
                .filter(|target| matches!(target.kind, 1 | 30311))
                .collect(),
        }
    }

    pub fn classify(
        &self,
        ids: &[String],
        coordinates: &[String],
        discovery_available: bool,
    ) -> (Target, Option<ConversationTarget>) {
        for current in self.current() {
            if ids.contains(&current.event_id)
                || current.coordinate.as_ref().is_some_and(|root| {
                    coordinates.contains(root)
                        || ids.iter().any(|id| self.aliases.get(id) == Some(root))
                })
            {
                return (Target::Current, Some(current));
            }
        }
        for id in ids {
            if let Some(event) = self.known.get(id) {
                if event.event().pubkey == self.policy.project_pubkey && event.event().kind == 1 {
                    return (Target::ProjectContent, Some(self.context(event, None)));
                }
            }
        }
        if !discovery_available
            || ids
                .iter()
                .chain(coordinates.iter())
                .any(|id| self.uncertain.contains(id))
        {
            (Target::Unavailable, None)
        } else {
            (Target::Other, None)
        }
    }

    fn context(&self, event: &VerifiedEvent, coordinate: Option<String>) -> ConversationTarget {
        ConversationTarget {
            event_id: event.event().id.clone(),
            kind: event.event().kind,
            coordinate,
            payee_pubkey: self.policy.project_pubkey.clone(),
        }
    }
}

pub(crate) fn select_active_stream(
    streams: &[AuthenticatedStream],
    owner: &str,
    now: i64,
) -> Result<Option<AuthenticatedStream>> {
    let mut heads: BTreeMap<&str, &AuthenticatedStream> = BTreeMap::new();
    for stream in streams {
        if stream.owner() != owner {
            bail!("stream authority belongs to another project owner");
        }
        if heads
            .get(stream.coordinate())
            .is_none_or(|old| supersedes(stream.event.event(), old.event.event()))
        {
            heads.insert(stream.coordinate(), stream);
        }
    }
    Ok(heads
        .into_values()
        .filter(|stream| stream.is_live(now))
        .max_by(|a, b| {
            a.event
                .event()
                .created_at
                .cmp(&b.event.event().created_at)
                .then_with(|| a.event.event().id.cmp(&b.event.event().id))
        })
        .cloned())
}

#[must_use]
pub fn supersedes(new: &SignedNostrEvent, old: &SignedNostrEvent) -> bool {
    new.created_at > old.created_at || (new.created_at == old.created_at && new.id < old.id)
}

#[derive(Debug, Clone)]
pub struct AuthenticatedProfile {
    event: VerifiedEvent,
    profile: Profile,
}

impl AuthenticatedProfile {
    pub fn parse(event: VerifiedEvent, expected_pubkey: &str, now: i64) -> Result<Self> {
        if event.event().kind != 0 || event.event().pubkey != expected_pubkey {
            bail!("profile author does not match the authenticated actor");
        }
        event.require_not_future(now)?;
        let metadata: serde_json::Value =
            serde_json::from_str(&event.event().content).unwrap_or_default();
        let raw_name = metadata["display_name"]
            .as_str()
            .filter(|s| !s.trim().is_empty())
            .or_else(|| metadata["name"].as_str())
            .unwrap_or("Anon");
        let mut name = String::new();
        for c in raw_name.chars().filter(|c| !c.is_control()) {
            if name.len() + c.len_utf8() > 160 {
                break;
            }
            name.push(c);
        }
        let name = if name.trim().is_empty() {
            "Anon".into()
        } else {
            name.trim().to_owned()
        };
        let lud16 = metadata["lud16"]
            .as_str()
            .map(str::trim)
            .filter(|address| validate_lud16(address).is_ok())
            .map(str::to_owned);
        let picture = metadata["picture"]
            .as_str()
            .filter(|url| public_network::validate_destination(url, "https").is_ok())
            .map(str::to_owned);
        let profile = Profile {
            name,
            picture,
            lud16,
        };
        profile.validate()?;
        Ok(Self { event, profile })
    }
    pub fn event(&self) -> &VerifiedEvent {
        &self.event
    }
    pub fn profile(&self) -> &Profile {
        &self.profile
    }
}

#[cfg(test)]
mod tests {
    use bitcoin::secp256k1::{Keypair, SecretKey};
    use serde_json::json;

    use super::*;

    fn key(marker: u8) -> Keypair {
        Keypair::from_secret_key(
            &Secp256k1::new(),
            &SecretKey::from_slice(&[marker; 32]).unwrap(),
        )
    }

    fn public(marker: u8) -> String {
        key(marker).x_only_public_key().0.to_string()
    }

    fn signed(
        marker: u8,
        kind: u64,
        at: i64,
        tags: Vec<Vec<String>>,
        content: &str,
    ) -> VerifiedEvent {
        let mut event = SignedNostrEvent {
            id: String::new(),
            pubkey: public(marker),
            created_at: at as u64,
            kind,
            tags,
            content: content.into(),
            sig: String::new(),
        };
        let digest: [u8; 32] =
            Sha256::digest(crate::nostr_event::canonical_bytes(&event).unwrap()).into();
        event.id = hex::encode(digest);
        event.sig = Secp256k1::new()
            .sign_schnorr_no_aux_rand(&Message::from_digest(digest), &key(marker))
            .to_string();
        VerifiedEvent::verify(event).unwrap()
    }

    fn tag(values: &[&str]) -> Vec<String> {
        values.iter().map(|s| (*s).into()).collect()
    }

    fn policy() -> TargetPolicy {
        TargetPolicy {
            project_pubkey: public(1),
            tracked_tags: vec!["LightningGoats".into()],
        }
    }

    fn accept(value: StreamAuthentication) -> AuthenticatedStream {
        match value {
            StreamAuthentication::Accepted(stream) => *stream,
            _ => panic!("expected authenticated stream"),
        }
    }

    #[tokio::test]
    async fn host_proof_binds_coordinate_and_arbitrary_services_grant_no_authority() {
        let now = 1_794_225_600;
        let coordinate = format!("30311:{}:show", public(2));
        let digest: [u8; 32] = Sha256::digest(coordinate.as_bytes()).into();
        let proof = Secp256k1::new()
            .sign_schnorr_no_aux_rand(&Message::from_digest(digest), &key(1))
            .to_string();
        let mut tags = vec![
            tag(&["d", "show"]),
            tag(&["status", "live"]),
            tag(&["p", &public(1), "", "Host", &proof]),
        ];
        let mut cache = ProviderCache::default();
        let stream = accept(
            cache
                .authenticate(signed(2, 30311, now, tags.clone(), ""), &public(1), now)
                .await
                .unwrap(),
        );
        assert_eq!(stream.authority(), &StreamAuthority::HostProof);
        assert!(stream.is_live(now));
        tags[0][1] = "another-show".into();
        assert!(matches!(
            cache
                .authenticate(signed(2, 30311, now, tags, ""), &public(1), now)
                .await
                .unwrap(),
            StreamAuthentication::Rejected
        ));
        for service in [
            "http://127.0.0.1/",
            "https://attacker.example/api/v1",
            "https://api-core.zap.stream.attacker.example/api/v1",
        ] {
            let event = signed(
                2,
                30311,
                now,
                vec![
                    tag(&["d", "show"]),
                    tag(&["status", "live"]),
                    tag(&["p", &public(1), "", "host"]),
                    tag(&["service", service]),
                ],
                "",
            );
            assert!(matches!(
                cache.authenticate(event, &public(1), now).await.unwrap(),
                StreamAuthentication::Rejected
            ));
        }
    }

    #[tokio::test]
    async fn provider_attestation_expires_and_cannot_be_reused_for_another_owner() {
        let now = 1_794_225_600;
        let service = "https://api-core.zap.stream/api/v1";
        let mut cache = ProviderCache::default();
        // Simulate the private cache produced by a successful fixed-origin TLS fetch.
        cache
            .keys
            .insert((service.into(), public(1)), (public(2), now + 1800));
        let stream = accept(
            cache
                .authenticate(
                    signed(
                        2,
                        30311,
                        now,
                        vec![
                            tag(&["d", "show"]),
                            tag(&["status", "live"]),
                            tag(&["p", &public(1), "", "host"]),
                            tag(&["service", service]),
                        ],
                        "",
                    ),
                    &public(1),
                    now,
                )
                .await
                .unwrap(),
        );
        assert!(stream.is_live(now + 1799));
        assert!(!stream.is_live(now + 1800));
        let day = DayWindow::at("America/Denver", now).unwrap();
        let mut foreign_policy = policy();
        foreign_policy.project_pubkey = public(3);
        assert!(
            Targets::project(
                foreign_policy,
                &day,
                vec![],
                vec![stream],
                BTreeSet::new(),
                now
            )
            .is_err()
        );
    }

    #[tokio::test]
    async fn forged_provider_publishers_cannot_repeat_metadata_fetches_in_one_batch() {
        let now = 1_794_225_600;
        let service = "https://api-core.zap.stream/api/v1";
        let mut cache = ProviderCache::default();
        cache.attempted.insert((service.into(), public(1)), now);
        let event = signed(
            3,
            30311,
            now,
            vec![
                tag(&["d", "show"]),
                tag(&["status", "live"]),
                tag(&["p", &public(1), "", "host"]),
                tag(&["service", service]),
            ],
            "",
        );
        // No transport is called after the recorded failed attempt.
        assert!(matches!(
            cache
                .authenticate(event.clone(), &public(1), now)
                .await
                .unwrap(),
            StreamAuthentication::Unavailable
        ));
        cache
            .keys
            .insert((service.into(), public(1)), (public(2), now + 1800));
        assert!(matches!(
            cache.authenticate(event, &public(1), now).await.unwrap(),
            StreamAuthentication::Rejected
        ));
    }

    #[tokio::test]
    async fn latest_ended_stream_cannot_be_revived_by_replayed_live_alias() {
        let now = 1_794_225_600;
        let mut cache = ProviderCache::default();
        let live = accept(
            cache
                .authenticate(
                    signed(
                        1,
                        30311,
                        now - 20,
                        vec![tag(&["d", "show"]), tag(&["status", "live"])],
                        "",
                    ),
                    &public(1),
                    now,
                )
                .await
                .unwrap(),
        );
        let ended = accept(
            cache
                .authenticate(
                    signed(
                        1,
                        30311,
                        now - 10,
                        vec![tag(&["d", "show"]), tag(&["status", "ended"])],
                        "",
                    ),
                    &public(1),
                    now,
                )
                .await
                .unwrap(),
        );
        assert!(
            select_active_stream(&[ended.clone(), live.clone()], &public(1), now)
                .unwrap()
                .is_none()
        );
        assert!(ended.recently_ended(now));
        let day = DayWindow::at("America/Denver", now).unwrap();
        let targets = Targets::project(
            policy(),
            &day,
            vec![],
            vec![live.clone(), ended],
            BTreeSet::new(),
            now,
        )
        .unwrap();
        assert_eq!(
            targets
                .classify(&[live.event().event().id.clone()], &[], true)
                .0,
            Target::Other
        );
        assert!(targets.current().is_empty());
    }

    #[tokio::test]
    async fn matching_goal_note_wins_and_old_stream_ids_remain_current_aliases() {
        let now = 1_794_225_600;
        let day = DayWindow::at("America/Denver", now).unwrap();
        let goal = signed(
            1,
            9041,
            now - 100,
            vec![
                tag(&["amount", "1000"]),
                tag(&["relays", "wss://relay.example"]),
            ],
            "Goal",
        );
        let note = signed(
            1,
            1,
            now - 30,
            vec![
                tag(&["t", "#LIGHTNINGGOATS"]),
                tag(&["goal", &goal.event().id]),
            ],
            "Today's show",
        );
        let unrelated = signed(1, 1, now - 5, vec![], "Another #lightninggoats note");
        let mut cache = ProviderCache::default();
        let old = accept(
            cache
                .authenticate(
                    signed(
                        1,
                        30311,
                        now - 20,
                        vec![
                            tag(&["d", "show"]),
                            tag(&["status", "live"]),
                            tag(&["goal", &goal.event().id]),
                        ],
                        "old",
                    ),
                    &public(1),
                    now,
                )
                .await
                .unwrap(),
        );
        let current = accept(
            cache
                .authenticate(
                    signed(
                        1,
                        30311,
                        now - 10,
                        vec![
                            tag(&["d", "show"]),
                            tag(&["status", "live"]),
                            tag(&["goal", &goal.event().id]),
                        ],
                        "new",
                    ),
                    &public(1),
                    now,
                )
                .await
                .unwrap(),
        );
        let targets = Targets::project(
            policy(),
            &day,
            vec![goal.clone(), unrelated, note.clone()],
            vec![old.clone(), current.clone()],
            BTreeSet::new(),
            now,
        )
        .unwrap();
        assert_eq!(
            targets
                .current()
                .iter()
                .map(|target| target.event_id.as_str())
                .collect::<Vec<_>>(),
            vec![
                note.event().id.as_str(),
                current.event().event().id.as_str(),
                goal.event().id.as_str()
            ]
        );
        let (_, context) = targets.classify(&[old.event().event().id.clone()], &[], true);
        assert_eq!(context.unwrap().event_id, current.event().event().id);
        assert_eq!(
            targets.classify(&["12".repeat(32)], &[], false).0,
            Target::Unavailable
        );
    }

    #[test]
    fn profile_does_not_require_nip05_and_never_retains_invalid_payout_metadata() {
        let now = 1_794_225_600;
        let event=signed(2,0,now,vec![],&json!({"display_name":"  Tester\n🐐  ","lud16":"tester@example.com","picture":"https://images.example/goat.png","nip05":"invalid"}).to_string());
        let profile = AuthenticatedProfile::parse(event.clone(), &public(2), now).unwrap();
        assert_eq!(profile.profile().name, "Tester🐐");
        assert_eq!(
            profile.profile().lud16.as_deref(),
            Some("tester@example.com")
        );
        assert!(AuthenticatedProfile::parse(event, &public(1), now).is_err());
        let negative = signed(
            2,
            0,
            now + 1,
            vec![],
            &json!({"lud16":"http://127.0.0.1","picture":"https://127.0.0.1/private"}).to_string(),
        );
        let profile = AuthenticatedProfile::parse(negative, &public(2), now).unwrap();
        assert!(profile.profile().lud16.is_none());
        assert!(profile.profile().picture.is_none());
    }
}
