//! Bounded NIP-01/BIP-340 authentication shared by receive and CyberHerd ingress.
//! Possessing a signed event proves its author, never a payment or host claim.
use std::str::FromStr;

use anyhow::{Context, Result, bail};
use bitcoin::secp256k1::{Message, Secp256k1, XOnlyPublicKey, schnorr::Signature};
use sha2::{Digest, Sha256};

use crate::nostr::SignedNostrEvent;

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

#[cfg(test)]
pub(crate) fn test_event(
    secret: u8,
    kind: u64,
    created_at: u64,
    content: &str,
    tags: Vec<Vec<String>>,
) -> SignedNostrEvent {
    use bitcoin::secp256k1::{Keypair, SecretKey};
    let secp = Secp256k1::new();
    let key = Keypair::from_secret_key(&secp, &SecretKey::from_slice(&[secret; 32]).unwrap());
    let mut event = SignedNostrEvent {
        id: String::new(),
        pubkey: key.x_only_public_key().0.to_string(),
        created_at,
        kind,
        tags,
        content: content.into(),
        sig: String::new(),
    };
    let hash: [u8; 32] = Sha256::digest(canonical_bytes(&event).unwrap()).into();
    event.id = hex::encode(hash);
    event.sig = secp
        .sign_schnorr_no_aux_rand(&Message::from_digest(hash), &key)
        .to_string();
    event
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verifies_canonical_unicode_control_escaping_and_signed_tags() {
        let event = test_event(
            1,
            1,
            100,
            "Goats ⚡\n\"quotes\" \\ tabs\t",
            vec![vec!["t".into(), "CyberHerd".into()]],
        );
        let json = serde_json::to_string_pretty(&event).unwrap();
        let verified = VerifiedEvent::from_json(&json).unwrap();
        assert_eq!(verified.event(), &event);
        assert_eq!(
            verified.singleton_tag("t", true).unwrap(),
            Some("CyberHerd")
        );
        assert_eq!(
            verified.tag_values("t").collect::<Vec<_>>(),
            vec!["CyberHerd"]
        );
        assert!(verified.require_not_future(0).is_ok());
        assert!(verified.require_not_future(-1).is_err());
    }

    #[test]
    fn mutated_fields_recomputed_forged_ids_and_uppercase_fail_authentication() {
        let event = test_event(1, 7, 100, "+", vec![]);
        let mut edited = event.clone();
        edited.content = "-".into();
        assert!(VerifiedEvent::verify(edited.clone()).is_err());
        edited.id = hex::encode(Sha256::digest(canonical_bytes(&edited).unwrap()));
        assert!(VerifiedEvent::verify(edited).is_err());
        let mut edited = event.clone();
        edited.pubkey = test_event(2, 1, 100, "", vec![]).pubkey;
        edited.id = hex::encode(Sha256::digest(canonical_bytes(&edited).unwrap()));
        assert!(VerifiedEvent::verify(edited).is_err());
        let mut edited = event;
        edited.id = edited.id.to_ascii_uppercase();
        assert!(VerifiedEvent::verify(edited).is_err());
    }

    #[test]
    fn bounded_tags_timestamps_duplicate_fields_and_singletons_fail_closed() {
        let json = " ".repeat(MAX_EVENT_BYTES + 1);
        assert!(VerifiedEvent::from_json(&json).is_err());
        assert!(VerifiedEvent::verify(test_event(1, 65_536, 100, "", vec![])).is_err());
        assert!(VerifiedEvent::verify(test_event(1, 1, u64::MAX, "", vec![])).is_err());
        assert!(VerifiedEvent::verify(test_event(1, 1, 100, "", vec![vec![]])).is_err());
        let event = test_event(
            1,
            1,
            100,
            "",
            vec![vec!["p".into(), "one".into()], vec!["p".into()]],
        );
        let verified = VerifiedEvent::verify(event).unwrap();
        assert!(verified.singleton_tag("p", true).is_err());
        assert!(verified.singleton_tag("missing", true).is_err());
        let event = test_event(1, 1, 1000, "", vec![]);
        assert!(
            VerifiedEvent::verify(event.clone())
                .unwrap()
                .require_not_future(0)
                .is_err()
        );
        let json = serde_json::to_string(&event)
            .unwrap()
            .replacen('{', "{\"kind\":1,", 1);
        assert!(VerifiedEvent::from_json(&json).is_err());
    }

    #[test]
    fn coordinates_require_real_keys_canonical_kind_and_replaceable_semantics() {
        let key = test_event(1, 1, 100, "", vec![]).pubkey;
        let value = format!("30311:{key}:goats:stream");
        let coordinate: Coordinate = value.parse().unwrap();
        assert_eq!(coordinate.identifier, "goats:stream");
        assert_eq!(coordinate.to_string(), value);
        for value in [
            format!("1:{key}:note"),
            format!("030311:{key}:goats"),
            format!("10000:{key}:nonempty"),
            format!("30311:{key}"),
            format!("30311:{}:stream", "00".repeat(32)),
        ] {
            assert!(value.parse::<Coordinate>().is_err(), "{value}");
        }
    }
}
