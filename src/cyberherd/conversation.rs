//! Frozen conversation addresses are presentation evidence, never payment,
//! membership or host authority. Ingress supplies them only after target checks.
use super::{
    ingress::hash,
    targets::{ConversationTarget, InteractionContext},
};
use crate::{nostr::SignedNostrEvent, nostr_event::VerifiedEvent};
use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};

pub fn identity(owner: &str, member: &str, source: &str) -> Result<String> {
    crate::nostr_event::validate_public_key(owner)?;
    crate::nostr_event::validate_public_key(member)?;
    if source.len() > 512 || source.chars().any(char::is_control) {
        bail!("invalid conversation source identity");
    }
    let mut source = source;
    while let Some(inner) = source.strip_prefix("msg:") {
        source = inner;
    }
    if source.is_empty() {
        bail!("empty conversation source identity");
    }
    hash(&(owner, member, source))
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(deny_unknown_fields)]
pub struct Context {
    pub note: Option<ConversationTarget>,
    pub stream: Option<ConversationTarget>,
}
impl Context {
    pub(crate) fn from_interaction(value: &InteractionContext) -> Self {
        let note = if value.matched().kind == 1 {
            Some(value.matched().clone())
        } else {
            value.destinations().iter().find(|t| t.kind == 1).cloned()
        };
        let stream = value
            .destinations()
            .iter()
            .find(|t| t.kind == 30311)
            .cloned();
        Self { note, stream }
    }
    pub fn validate(&self, owner: &str) -> Result<()> {
        crate::nostr_event::validate_public_key(owner)?;
        for target in self.targets() {
            crate::nostr_event::validate_hex(&target.event_id, 32)?;
            if target.payee_pubkey != owner {
                bail!("conversation belongs to another project");
            }
        }
        if self
            .note
            .as_ref()
            .is_some_and(|n| n.kind != 1 || n.coordinate.is_some())
        {
            bail!("invalid regular conversation root");
        }
        if let Some(stream) = &self.stream {
            if stream.kind != 30311 {
                bail!("invalid stream conversation kind");
            }
            validate_coordinate(
                stream
                    .coordinate
                    .as_deref()
                    .ok_or_else(|| anyhow::anyhow!("stream has no stable address"))?,
            )?;
        }
        Ok(())
    }
    pub fn targets(&self) -> impl Iterator<Item = &ConversationTarget> {
        self.note.iter().chain(self.stream.iter())
    }
}
pub fn validate_coordinate(coordinate: &str) -> Result<()> {
    let parts: Vec<_> = coordinate.splitn(3, ':').collect();
    if parts.len() != 3
        || parts[0] != "30311"
        || parts[2].is_empty()
        || parts[2].len() > 512
        || parts[2].chars().any(char::is_control)
    {
        bail!("invalid stable stream address");
    }
    crate::nostr_event::validate_public_key(parts[1])
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct MemberConversation {
    /// Admission interaction, retained independently of publication success.
    pub generation: String,
    pub context: Context,
}
impl MemberConversation {
    pub fn validate(&self, owner: &str) -> Result<()> {
        crate::nostr_event::validate_hex(&self.generation, 32)?;
        self.context.validate(owner)
    }
}

/// A real publication ACK is retained by the membership delivery store. Native
/// signatures prove the referenced event's author; the trusted intake UID
/// attests its relay acknowledgement when returning this over private IPC.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PublishedReference {
    pub event: SignedNostrEvent,
    pub relay: String,
    pub acknowledged_at: i64,
}
impl PublishedReference {
    pub fn validate(&self, owner: &str, member: &str) -> Result<()> {
        let event = VerifiedEvent::verify(self.event.clone())?;
        if event.event().pubkey != owner
            || !matches!(event.event().kind, 1 | 1311)
            || self.acknowledged_at < i64::try_from(event.event().created_at)?
            || !event.event().tags.iter().any(|t| {
                t.first().is_some_and(|x| x == "p") && t.get(1).is_some_and(|x| x == member)
            })
        {
            bail!("invalid published member joining reference");
        }
        crate::public_network::validate_destination(&self.relay, "wss")?;
        Ok(())
    }
}
