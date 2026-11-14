//! Private contact envelopes and fixed fees. No project signing key or feed authority.
use anyhow::{Result, bail};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::{nostr::SignedNostrEvent, nostr_event::VerifiedEvent};

pub const FEE_MSAT: u64 = 100_000;
pub const MAX_REQUEST_BYTES: usize = 32_768;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub recipient: String,
    pub inbox_relays: Vec<String>,
}
impl Config {
    pub fn validate(&self, project: &str) -> Result<()> {
        crate::nostr_event::validate_public_key(&self.recipient)?;
        if self.recipient != project || self.inbox_relays.is_empty() || self.inbox_relays.len() > 8
        {
            bail!("private contact must use the project recipient and bounded inbox relays");
        }
        for relay in &self.inbox_relays {
            crate::public_network::validate_destination(relay, "wss")?;
        }
        Ok(())
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StatusRequest {
    pub id: Uuid,
    pub capability: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub id: Uuid,
    pub capability: String,
    pub reply_email: String,
    pub envelope: SignedNostrEvent,
}

/// Only this validated type can create a private, fee-bearing delivery intent.
/// Deliberately not Debug: caller material must not enter diagnostic logs.
#[derive(Clone)]
pub struct Intent {
    pub(crate) id: Uuid,
    pub(crate) capability_hash: String,
    pub(crate) reply_email_hash: String,
    pub(crate) envelope_json: String,
    pub(crate) envelope_id: String,
    pub(crate) recipient: String,
    pub(crate) description_hash: String,
    pub(crate) relays_json: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Status {
    pub id: Uuid,
    pub state: String,
    pub fee_sats: u64,
    pub invoice: Option<String>,
}

pub fn capability_hash(value: &str) -> Result<String> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        bail!("invalid contact status capability");
    }
    Ok(hex::encode(Sha256::digest(value.as_bytes())))
}

pub fn validate_envelope(raw: &str, recipient: &str) -> Result<VerifiedEvent> {
    crate::nostr_event::validate_public_key(recipient)?;
    if raw.len() > 24_576 {
        bail!("contact envelope exceeds size bound");
    }
    let verified = VerifiedEvent::from_json(raw)?;
    let event = verified.event();
    if event.kind != 1059
        || event.tags != [vec!["p".to_owned(), recipient.to_owned()]]
        || event.content.len() > 24_000
    {
        bail!("contact requires one private recipient gift wrapper");
    }
    let decoded = STANDARD.decode(&event.content)?;
    if decoded.len() < 99 || decoded.first() != Some(&2) {
        bail!("contact requires a NIP-44 v2 encrypted envelope");
    }
    Ok(verified)
}

impl Intent {
    pub fn validate(
        request: Request,
        recipient: &str,
        relays: &[String],
        now: i64,
    ) -> Result<Self> {
        if request.id.get_version_num() != 4 || now < 0 || relays.is_empty() || relays.len() > 8 {
            bail!("invalid contact identity, time or inbox policy");
        }
        for relay in relays {
            crate::public_network::validate_destination(relay, "wss")?;
        }
        let email = request.reply_email;
        // Validate transiently; persist only its digest. The browser includes
        // the actual reply address inside the recipient's encrypted message.
        if email.is_empty()
            || email.len() > 254
            || email
                .chars()
                .any(|c| c.is_whitespace() || c.is_control() || "<>".contains(c))
            || email.matches('@').count() != 1
        {
            bail!("contact requires a reply email address");
        }
        let Some((local, domain)) = email.split_once('@') else {
            bail!("invalid reply email address");
        };
        if local.is_empty()
            || local.len() > 64
            || local.starts_with('.')
            || local.ends_with('.')
            || local.contains("..")
            || !domain.contains('.')
            || domain.split('.').any(|label| {
                label.is_empty()
                    || label.len() > 63
                    || label.starts_with('-')
                    || label.ends_with('-')
                    || !label
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b == b'-')
            })
        {
            bail!("invalid reply email address");
        }
        let envelope_json = serde_json::to_string(&request.envelope)?;
        let verified = validate_envelope(&envelope_json, recipient)?;
        verified.require_not_future(now)?;
        if verified.event().created_at < (now as u64).saturating_sub(173_100) {
            bail!("contact gift wrapper is too old for new admission");
        }
        let reply_email_hash = hex::encode(Sha256::digest(email.as_bytes()));
        let description_hash = description_hash(
            request.id,
            &verified.event().id,
            recipient,
            &reply_email_hash,
        )?;
        Ok(Self {
            id: request.id,
            capability_hash: capability_hash(&request.capability)?,
            reply_email_hash,
            envelope_id: verified.event().id.clone(),
            envelope_json,
            recipient: recipient.into(),
            description_hash,
            relays_json: serde_json::to_string(relays)?,
        })
    }
}

pub(crate) fn description_hash(
    id: Uuid,
    event: &str,
    recipient: &str,
    email_hash: &str,
) -> Result<String> {
    Ok(hex::encode(Sha256::digest(serde_json::to_vec(&(
        "lightning-goats-private-contact-v1",
        id,
        FEE_MSAT,
        event,
        recipient,
        email_hash,
    ))?)))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) fn fixture() -> Intent {
        let recipient = crate::nostr_event::test_event(3, 1, 100, "", vec![]).pubkey;
        let encrypted = STANDARD.encode([vec![2_u8], vec![1; 120]].concat());
        let envelope = crate::nostr_event::test_event(
            1,
            1059,
            100,
            &encrypted,
            vec![vec!["p".into(), recipient.clone()]],
        );
        Intent::validate(
            Request {
                id: Uuid::new_v4(),
                capability: "ab".repeat(32),
                reply_email: "visitor@example.org".into(),
                envelope,
            },
            &recipient,
            &["wss://relay.example.org".into()],
            100,
        )
        .unwrap()
    }

    #[test]
    fn intent_binds_encrypted_event_and_fee_without_retaining_email_or_capability() {
        let intent = fixture();
        assert_ne!(intent.capability_hash, "ab".repeat(32));
        assert!(!intent.envelope_json.contains("visitor@example.org"));
        assert_eq!(
            intent.description_hash,
            description_hash(
                intent.id,
                &intent.envelope_id,
                &intent.recipient,
                &intent.reply_email_hash
            )
            .unwrap()
        );
        assert!(validate_envelope(&intent.envelope_json, &"11".repeat(32)).is_err());
        assert!(capability_hash("secret").is_err());
        assert!(capability_hash(&"AB".repeat(32)).is_err());
    }

    #[test]
    fn malformed_email_wrong_recipient_and_caller_selected_fee_are_refused() {
        let intent = fixture();
        for email in [
            "",
            "missing-domain",
            "a@example..org",
            "a@-example.org",
            "a\n@example.org",
        ] {
            let request = Request {
                id: Uuid::new_v4(),
                capability: "ab".repeat(32),
                reply_email: email.into(),
                envelope: serde_json::from_str(&intent.envelope_json).unwrap(),
            };
            assert!(
                Intent::validate(
                    request,
                    &intent.recipient,
                    &["wss://relay.example.org".into()],
                    100
                )
                .is_err(),
                "{email:?}"
            );
        }
        let mut value = serde_json::json!({"id":Uuid::new_v4(),"capability":"ab".repeat(32),"reply_email":"a@example.org","envelope":serde_json::from_str::<serde_json::Value>(&intent.envelope_json).unwrap()});
        value["fee_sats"] = 1.into();
        assert!(serde_json::from_value::<Request>(value).is_err());
        assert!(
            Config {
                recipient: intent.recipient.clone(),
                inbox_relays: vec!["wss://10.8.0.6/".into()]
            }
            .validate(&intent.recipient)
            .is_err()
        );
    }
}
