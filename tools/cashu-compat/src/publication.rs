//! Typed NIP-61 publication intents. This module has no signing key or transport.
use anyhow::{Result, ensure};
use base64::{Engine, engine::general_purpose::STANDARD};
use lightning_goats_nostr_event::{VerifiedEvent, validate_hex, validate_public_key};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use uuid::Uuid;

pub(crate) fn nutzap_lock_key(x_only: &str) -> String {
    format!("02{x_only}")
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "purpose", rename_all = "snake_case", deny_unknown_fields)]
pub enum Intent {
    Announcement {
        owner: String,
        created_at: u64,
        receiving_key: String,
        mints: Vec<String>,
        relays: Vec<String>,
    },
    Redemption {
        owner: String,
        created_at: u64,
        acquisition_id: Uuid,
        source_event: String,
        sender: String,
        net_sats: u64,
    },
}
#[derive(Serialize)]
pub struct Unsigned {
    pub pubkey: String,
    pub created_at: u64,
    pub kind: u64,
    pub tags: Vec<Vec<String>>,
    pub content: String,
}
impl Intent {
    /// History retains its original acquired context. Announcements must still
    /// advertise the current receiving policy, even after signing or route freeze.
    pub fn require_current_announcement(&self, current: Option<&Self>) -> Result<()> {
        if let Self::Announcement { created_at, .. } = self {
            let mut expected = current
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("announcement policy unavailable"))?;
            let Self::Announcement {
                created_at: expected_time,
                ..
            } = &mut expected
            else {
                anyhow::bail!("invalid announcement policy")
            };
            *expected_time = *created_at;
            ensure!(
                *self == expected,
                "announcement differs from current receiving policy"
            );
        }
        Ok(())
    }
    /// Preparing a public announcement never signs, publishes, or enables receiving.
    pub fn announcement(config: &crate::service_config::Config, created_at: u64) -> Result<Self> {
        let prepared = config.prepare()?;
        let relays = config
            .relay_intake
            .as_ref()
            .filter(|r| r.enabled)
            .ok_or_else(|| {
                anyhow::anyhow!("announcement requires enabled configured relay intake")
            })?;
        ensure!(
            config.receive_nutzap,
            "announcement requires enabled nutzap admission"
        );
        let mut mints: Vec<_> = config
            .mints
            .iter()
            .filter(|m| m.receive_nutzap && m.bolt11_exit_qualified)
            .map(|m| m.advertised_url.clone())
            .collect();
        let mut relays = relays.relays.clone();
        mints.sort();
        relays.sort();
        let intent = Self::Announcement {
            owner: config.recipient.clone(),
            created_at,
            receiving_key: nutzap_lock_key(&prepared.receiver.x_only_public_key().to_string()),
            mints,
            relays,
        };
        intent.validate()?;
        Ok(intent)
    }
    pub fn owner(&self) -> &str {
        match self {
            Self::Announcement { owner, .. } | Self::Redemption { owner, .. } => owner,
        }
    }
    pub fn validate(&self) -> Result<()> {
        validate_public_key(self.owner())?;
        let created_at = match self {
            Self::Announcement {
                created_at,
                receiving_key,
                mints,
                relays,
                ..
            } => {
                // Retained intents can still validate their original signed bytes.
                let x_only = if receiving_key.len() == 66 {
                    receiving_key
                        .strip_prefix("02")
                        .ok_or_else(|| anyhow::anyhow!("invalid announced nutzap lock key"))?
                } else {
                    receiving_key
                };
                validate_public_key(x_only)?;
                ensure!(
                    x_only != self.owner()
                        && !mints.is_empty()
                        && mints.len() <= 16
                        && !relays.is_empty()
                        && relays.len() <= 8,
                    "invalid announcement scope"
                );
                let mut unique = BTreeSet::new();
                for relay in relays {
                    let url = crate::relay::validate_relay(relay)?;
                    ensure!(
                        url.query().is_none() && unique.insert(url.to_string()),
                        "announcement relays must be unique public URLs without queries"
                    );
                }
                crate::relay::Filter {
                    recipient: self.owner().into(),
                    mints: mints.clone(),
                    since: 0,
                    until: 0,
                    limit: 1,
                }
                .validate()?;
                *created_at
            }
            Self::Redemption {
                created_at,
                acquisition_id,
                source_event,
                sender,
                net_sats,
                ..
            } => {
                ensure!(
                    !acquisition_id.is_nil() && *net_sats > 0 && *net_sats <= i64::MAX as u64,
                    "invalid redemption amount or identity"
                );
                validate_hex(source_event, 32)?;
                validate_public_key(sender)?;
                *created_at
            }
        };
        ensure!(
            created_at <= i64::MAX as u64,
            "invalid publication timestamp"
        );
        Ok(())
    }
    /// Sensitive metadata for a purpose-restricted NIP-44 encrypt-to-self broker.
    /// No token events exist here, so no invented 7375 reference is included.
    pub fn encryption_plaintext(&self) -> Result<Option<String>> {
        self.validate()?;
        Ok(match self {
            Self::Redemption { net_sats, .. } => Some(serde_json::to_string(&[
                ["direction".to_owned(), "in".to_owned()],
                ["amount".to_owned(), net_sats.to_string()],
                ["unit".to_owned(), "sat".to_owned()],
            ])?),
            Self::Announcement { .. } => None,
        })
    }
    pub fn unsigned(&self, ciphertext: Option<&str>) -> Result<Unsigned> {
        self.validate()?;
        let (created_at, kind, tags, content) = match self {
            Self::Announcement {
                created_at,
                receiving_key,
                mints,
                relays,
                ..
            } => {
                ensure!(
                    ciphertext.is_none(),
                    "announcement must not contain history ciphertext"
                );
                let mut tags: Vec<_> = relays
                    .iter()
                    .map(|r| vec!["relay".into(), r.clone()])
                    .collect();
                tags.extend(
                    mints
                        .iter()
                        .map(|m| vec!["mint".into(), m.clone(), "sat".into()]),
                );
                tags.push(vec!["pubkey".into(), receiving_key.clone()]);
                (*created_at, 10019, tags, String::new())
            }
            Self::Redemption {
                created_at,
                source_event,
                sender,
                ..
            } => {
                let ciphertext = ciphertext
                    .ok_or_else(|| anyhow::anyhow!("redemption encryption not frozen"))?;
                validate_envelope(ciphertext)?;
                (
                    *created_at,
                    7376,
                    vec![
                        vec![
                            "e".into(),
                            source_event.clone(),
                            String::new(),
                            "redeemed".into(),
                        ],
                        vec!["p".into(), sender.clone()],
                    ],
                    ciphertext.to_owned(),
                )
            }
        };
        let unsigned = Unsigned {
            pubkey: self.owner().into(),
            created_at,
            kind,
            tags,
            content,
        };
        ensure!(
            serde_json::to_vec(&unsigned)?.len() <= 64_000,
            "publication exceeds signed-event budget"
        );
        Ok(unsigned)
    }
    pub fn verify_signed(&self, ciphertext: Option<&str>, raw: &str) -> Result<VerifiedEvent> {
        let expected = self.unsigned(ciphertext)?;
        let verified = VerifiedEvent::from_json(raw)?;
        let actual = verified.event();
        ensure!(
            actual.pubkey == expected.pubkey
                && actual.created_at == expected.created_at
                && actual.kind == expected.kind
                && actual.tags == expected.tags
                && actual.content == expected.content,
            "signed publication changed frozen intent"
        );
        Ok(verified)
    }
}
/// Envelope validation is NOT MAC/decryption verification. The restricted signer
/// must authenticate encryption to the owner's own key before returning it.
pub(crate) fn validate_envelope(ciphertext: &str) -> Result<()> {
    ensure!(
        (132..=1024).contains(&ciphertext.len()),
        "history encryption envelope exceeds bound"
    );
    let bytes = STANDARD
        .decode(ciphertext)
        .map_err(|_| anyhow::anyhow!("invalid history encryption envelope"))?;
    ensure!(
        (99..=768).contains(&bytes.len()) && bytes[0] == 2 && STANDARD.encode(&bytes) == ciphertext,
        "unsupported history encryption envelope"
    );
    Ok(())
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use cdk::secp256k1::{Keypair, Message, Secp256k1, SecretKey};
    use lightning_goats_nostr_event::{SignedNostrEvent, canonical_bytes};
    use sha2::{Digest, Sha256};
    pub(crate) fn owner() -> String {
        let secp = Secp256k1::new();
        Keypair::from_secret_key(&secp, &SecretKey::from_slice(&[4; 32]).unwrap())
            .x_only_public_key()
            .0
            .to_string()
    }
    pub(crate) fn announcement() -> Intent {
        Intent::Announcement {
            owner: owner(),
            created_at: 100,
            receiving_key: cdk::nuts::SecretKey::from_slice(&[1; 32])
                .unwrap()
                .public_key()
                .x_only_public_key()
                .to_string(),
            mints: vec!["https://mint.example".into()],
            relays: vec!["wss://relay.example".into()],
        }
    }
    /// Structurally valid synthetic envelope, NOT a cryptographic encryption fixture.
    pub(crate) fn envelope(byte: u8) -> String {
        let mut payload = vec![byte; 99];
        payload[0] = 2;
        STANDARD.encode(payload)
    }
    pub(crate) fn sign(unsigned: Unsigned) -> Result<String> {
        let secp = Secp256k1::new();
        let key = Keypair::from_secret_key(&secp, &SecretKey::from_slice(&[4; 32])?);
        let mut event = SignedNostrEvent {
            id: String::new(),
            pubkey: unsigned.pubkey,
            created_at: unsigned.created_at,
            kind: unsigned.kind,
            tags: unsigned.tags,
            content: unsigned.content,
            sig: String::new(),
        };
        let hash: [u8; 32] = Sha256::digest(canonical_bytes(&event)?).into();
        event.id = hex::encode(hash);
        event.sig = secp
            .sign_schnorr_no_aux_rand(&Message::from_digest(hash), &key)
            .to_string();
        Ok(serde_json::to_string(&event)?)
    }
    #[test]
    fn announcement_key_encoding_and_signed_intent_are_exact() -> Result<()> {
        let intent = announcement();
        let unsigned = intent.unsigned(None)?;
        assert_eq!(unsigned.kind, 10019);
        assert!(unsigned.content.is_empty());
        assert_eq!(
            unsigned.tags[1],
            vec!["mint", "https://mint.example", "sat"]
        );
        let advertised = unsigned.tags[2][1].clone();
        assert_eq!(advertised.len(), 64);
        let locked = cdk::nuts::PublicKey::from_hex(format!("02{advertised}"))?;
        assert_eq!(locked.x_only_public_key().to_string(), advertised);
        let raw = sign(unsigned)?;
        intent.verify_signed(None, &raw)?;
        let mut compressed = intent.clone();
        if let Intent::Announcement { receiving_key, .. } = &mut compressed {
            *receiving_key = nutzap_lock_key(receiving_key);
        }
        let new = compressed.unsigned(None)?;
        assert_eq!(new.tags[2][1], locked.to_hex());
        let canonical = sign(new)?;
        compressed.verify_signed(None, &canonical)?;
        assert!(intent.verify_signed(None, &canonical).is_err());
        assert!(compressed.verify_signed(None, &raw).is_err());
        assert!(
            intent
                .require_current_announcement(Some(&compressed))
                .is_err()
        );
        for change in 0..4 {
            let mut unsigned = intent.unsigned(None)?;
            match change {
                0 => unsigned.kind = 1,
                1 => unsigned.created_at += 1,
                2 => unsigned.content = "unexpected".into(),
                _ => unsigned.tags.push(vec!["p".into(), owner()]),
            };
            assert!(intent.verify_signed(None, &sign(unsigned)?).is_err());
        }
        let mut invalid = intent.clone();
        for bad in [
            owner(),
            nutzap_lock_key(&owner()),
            format!("03{advertised}"),
        ] {
            if let Intent::Announcement { receiving_key, .. } = &mut invalid {
                *receiving_key = bad;
            }
            assert!(invalid.validate().is_err());
        }
        invalid = intent.clone();
        if let Intent::Announcement { relays, .. } = &mut invalid {
            relays[0] = "wss://relay.example/?secret=private".into();
        }
        assert!(invalid.validate().is_err());
        Ok(())
    }
    #[test]
    fn redemption_amount_is_private_and_unsigned_requires_versioned_envelope() -> Result<()> {
        let intent = Intent::Redemption {
            owner: owner(),
            created_at: 100,
            acquisition_id: Uuid::new_v4(),
            source_event: "ab".repeat(32),
            sender: owner(),
            net_sats: 7,
        };
        let plaintext = intent.encryption_plaintext()?.unwrap();
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&plaintext)?,
            serde_json::json!([["direction", "in"], ["amount", "7"], ["unit", "sat"]])
        );
        for invalid in [
            None,
            Some(plaintext.as_str()),
            Some("#unsupported"),
            Some("eA=="),
        ] {
            assert!(intent.unsigned(invalid).is_err());
        }
        let ciphertext = envelope(9);
        let unsigned = intent.unsigned(Some(&ciphertext))?;
        assert_eq!(
            unsigned.tags,
            vec![
                vec![
                    "e".into(),
                    "ab".repeat(32),
                    String::new(),
                    "redeemed".into()
                ],
                vec!["p".into(), owner()]
            ]
        );
        assert!(!serde_json::to_string(&unsigned)?.contains("amount"));
        assert!(!plaintext.contains("created") && !plaintext.contains("7375"));
        intent.verify_signed(Some(&ciphertext), &sign(unsigned)?)?;
        let mut unsupported = vec![1; 99];
        unsupported[0] = 1;
        assert!(
            intent
                .unsigned(Some(&STANDARD.encode(unsupported)))
                .is_err()
        );
        Ok(())
    }
}

#[cfg(test)]
mod configuration_tests {
    use super::*;
    #[test]
    fn announcement_uses_only_active_qualified_mints_and_requires_receiving() -> Result<()> {
        let mut config =
            crate::relay_intake::tests::config(std::path::Path::new("/tmp/publication-fixture"))?;
        let mut unqualified = config.mints[0].clone();
        unqualified.id = "unqualified".into();
        unqualified.advertised_url = "https://unqualified.example".into();
        unqualified.bolt11_exit_qualified = false;
        config.mints.push(unqualified);
        let intent = Intent::announcement(&config, 100)?;
        let unsigned = intent.unsigned(None)?;
        assert_eq!(unsigned.tags.iter().filter(|t| t[0] == "mint").count(), 1);
        assert!(!serde_json::to_string(&unsigned)?.contains("unqualified"));
        config.relay_intake.as_mut().unwrap().enabled = false;
        assert!(Intent::announcement(&config, 100).is_err());
        config.relay_intake.as_mut().unwrap().enabled = true;
        config.receive_nutzap = false;
        assert!(Intent::announcement(&config, 100).is_err());
        Ok(())
    }
}
