//! Shared bounded NIP-01 event verification for core and Cashu wallet.
#![forbid(unsafe_code)]

pub mod relay_list;

use std::str::FromStr;

use anyhow::{Context, Result, bail};
use bitcoin::secp256k1::{Message, Secp256k1, XOnlyPublicKey, schnorr::Signature};
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct SignedNostrEvent {
    pub id: String,
    pub pubkey: String,
    pub created_at: u64,
    pub kind: u64,
    pub tags: Vec<Vec<String>>,
    pub content: String,
    pub sig: String,
}

pub const MAX_EVENT_BYTES: usize = 65_536;

/// No Deserialize implementation: callers cannot manufacture this capability
/// without canonical ID and signature verification.
#[derive(Debug, Clone)]
pub struct VerifiedEvent(SignedNostrEvent);

impl VerifiedEvent {
    pub fn from_json(json: &str) -> Result<Self> {
        if json.len() > MAX_EVENT_BYTES {
            bail!("Nostr event exceeds size bound");
        }
        let event: SignedNostrEvent =
            serde_json::from_str(json).context("invalid Nostr event JSON")?;
        Self::verify(event)
    }

    pub fn verify(event: SignedNostrEvent) -> Result<Self> {
        validate_hex(&event.id, 32)?;
        validate_hex(&event.pubkey, 32)?;
        validate_hex(&event.sig, 64)?;
        if event.kind > 65_535
            || event.created_at > i64::MAX as u64
            || event.content.len() > 32_768
            || event.tags.len() > 1024
        {
            bail!("Nostr event exceeds field bounds");
        }
        for tag in &event.tags {
            if tag.is_empty() || tag.len() > 32 || tag.iter().any(|value| value.len() > 8192) {
                bail!("Nostr event has invalid tag bounds");
            }
        }
        let serialized = canonical_bytes(&event)?;
        if serialized.len() > MAX_EVENT_BYTES {
            bail!("Nostr event exceeds size bound");
        }
        let hash: [u8; 32] = Sha256::digest(&serialized).into();
        if hex::encode(hash) != event.id {
            bail!("Nostr event ID does not match canonical content");
        }
        let public = XOnlyPublicKey::from_str(&event.pubkey).context("invalid Nostr public key")?;
        let signature =
            Signature::from_str(&event.sig).context("invalid Nostr signature encoding")?;
        Secp256k1::verification_only()
            .verify_schnorr(&signature, &Message::from_digest(hash), &public)
            .context("invalid Nostr signature")?;
        Ok(Self(event))
    }

    #[must_use]
    pub fn event(&self) -> &SignedNostrEvent {
        &self.0
    }

    /// Upper clock bound belongs to ingress policy, not signature validity. Old
    /// signed targets remain useful as authenticated conversation anchors.
    pub fn require_not_future(&self, now: i64) -> Result<()> {
        if now < 0 || self.0.created_at > (now as u64).saturating_add(300) {
            bail!("Nostr event timestamp is in the future");
        }
        Ok(())
    }

    pub fn tag_values<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a str> {
        self.0
            .tags
            .iter()
            .filter(move |tag| tag.first().is_some_and(|key| key == name))
            .filter_map(|tag| tag.get(1).map(String::as_str))
    }

    /// Cardinality is checked even for malformed one-element tags, so a second
    /// hidden p/e/a tag cannot bypass a NIP-57 singleton constraint.
    pub fn singleton_tag(&self, name: &str, required: bool) -> Result<Option<&str>> {
        let mut tags = self
            .0
            .tags
            .iter()
            .filter(|tag| tag.first().is_some_and(|key| key == name));
        let first = tags.next();
        if tags.next().is_some() {
            bail!("duplicate singleton Nostr tag");
        }
        let Some(tag) = first else {
            if required {
                bail!("missing required Nostr tag");
            }
            return Ok(None);
        };
        let value = tag
            .get(1)
            .filter(|value| !value.is_empty())
            .context("empty singleton Nostr tag")?;
        Ok(Some(value))
    }
}

pub fn canonical_bytes(event: &SignedNostrEvent) -> Result<Vec<u8>> {
    Ok(serde_json::to_vec(&serde_json::json!([
        0,
        event.pubkey,
        event.created_at,
        event.kind,
        event.tags,
        event.content
    ]))?)
}

pub fn validate_hex(value: &str, bytes: usize) -> Result<()> {
    if value.len() != bytes * 2
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        bail!("Nostr identifiers must use canonical lowercase hex");
    }
    Ok(())
}

pub fn validate_public_key(value: &str) -> Result<()> {
    validate_hex(value, 32)?;
    XOnlyPublicKey::from_str(value).context("invalid Nostr public key")?;
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Coordinate {
    pub kind: u16,
    pub author: String,
    pub identifier: String,
}

impl FromStr for Coordinate {
    type Err = anyhow::Error;
    fn from_str(value: &str) -> Result<Self> {
        if value.len() > 640 {
            bail!("Nostr coordinate exceeds size bound");
        }
        let mut parts = value.splitn(3, ':');
        let kind = parts
            .next()
            .context("missing coordinate kind")?
            .parse::<u16>()?;
        let author = parts.next().context("missing coordinate author")?;
        let identifier = parts
            .next()
            .context("missing coordinate identifier separator")?;
        validate_public_key(author)?;
        if !((30_000..40_000).contains(&kind)
            || (10_000..20_000).contains(&kind)
            || matches!(kind, 0 | 3))
        {
            bail!("coordinate requires a replaceable/addressable event kind");
        }
        if !(30_000..40_000).contains(&kind) && !identifier.is_empty() {
            bail!("replaceable coordinate must use an empty identifier");
        }
        if identifier.len() > 512 || identifier.chars().any(char::is_control) {
            bail!("invalid Nostr coordinate identifier");
        }
        let coordinate = Self {
            kind,
            author: author.into(),
            identifier: identifier.into(),
        };
        if coordinate.to_string() != value {
            bail!("noncanonical Nostr coordinate");
        }
        Ok(coordinate)
    }
}

impl std::fmt::Display for Coordinate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}:{}", self.kind, self.author, self.identifier)
    }
}
