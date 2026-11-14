//! Read-only LUD-06/NIP-57 recipient discovery. Keep exact metadata hashes, not
//! embedded images, and never confuse a provider's receipt key with a member.
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{cyberherd::membership::validate_lud16, public_network};

pub const MAX_RESPONSE: usize = 196_608;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Recipient {
    pub lnurl_url: String,
    pub callback: String,
    pub min_msat: u64,
    pub max_msat: u64,
    pub metadata_hash: String,
    /// None records ordinary-Lightning fallback; malformed advertised support
    /// is an error, never grounds to downgrade silently.
    pub receipt_pubkey: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Wire {
    status: Option<String>,
    tag: String,
    callback: String,
    min_sendable: u64,
    max_sendable: u64,
    metadata: String,
    allows_nostr: Option<bool>,
    nostr_pubkey: Option<String>,
}

pub fn address_url(address: &str) -> Result<String> {
    validate_lud16(address)?;
    let (user, domain) = address.split_once('@').context("missing address domain")?;
    let url = public_network::validate_destination(
        &format!("https://{domain}/.well-known/lnurlp/{user}"),
        "https",
    )?;
    Ok(url.into())
}

/// Conservative duplicate/operator comparison only. Preserve the original
/// local part for the actual case-sensitive LNURL request.
pub fn address_identity(address: &str) -> Result<String> {
    address_url(address)?;
    Ok(address.to_ascii_lowercase())
}

impl Recipient {
    pub async fn fetch(address: &str) -> Result<Self> {
        let url = address_url(address)?;
        let raw = public_network::get_json(&url, MAX_RESPONSE).await?;
        Self::parse(address, &raw)
    }

    pub fn parse(address: &str, raw: &[u8]) -> Result<Self> {
        if raw.len() > MAX_RESPONSE {
            bail!("recipient metadata response too large");
        }
        let wire: Wire = serde_json::from_slice(raw).context("invalid recipient metadata")?;
        // Some LNURL services (including Primal) include an explicit success
        // status alongside the normal payRequest fields. Only that exact
        // success value is compatible; error/unknown statuses still fail closed.
        if wire.status.as_deref().is_some_and(|status| status != "OK")
            || wire.tag != "payRequest"
            || wire.min_sendable == 0
            || wire.max_sendable < wire.min_sendable
        {
            bail!("recipient is not an available LNURL pay request");
        }
        let rows: Vec<Vec<serde_json::Value>> =
            serde_json::from_str(&wire.metadata).context("invalid raw LNURL metadata string")?;
        let mut descriptions = 0;
        let mut images = 0;
        for row in rows {
            let kind = row
                .first()
                .and_then(serde_json::Value::as_str)
                .context("LNURL metadata requires typed array entries")?;
            if kind == "text/plain" {
                if row.len() != 2 || !row[1].is_string() {
                    bail!("invalid LNURL description");
                }
                descriptions += 1;
            }
            if matches!(kind, "image/png;base64" | "image/jpeg;base64") {
                if row.len() != 2 || row[1].as_str().is_none_or(|s| s.len() > 136_536) {
                    bail!("invalid LNURL thumbnail");
                }
                images += 1;
            }
        }
        if descriptions != 1 || images > 1 {
            bail!("ambiguous LNURL metadata");
        }
        let receipt_pubkey = if wire.allows_nostr == Some(true) {
            let key = wire
                .nostr_pubkey
                .context("advertised Nostr support lacks receipt key")?;
            crate::nostr_event::validate_public_key(&key)?;
            Some(key)
        } else {
            None
        };
        let result = Self {
            lnurl_url: address_url(address)?,
            callback: wire.callback,
            min_msat: wire.min_sendable,
            max_msat: wire.max_sendable,
            metadata_hash: hex::encode(Sha256::digest(wire.metadata.as_bytes())),
            receipt_pubkey,
        };
        result.validate(address)?;
        Ok(result)
    }

    pub fn validate(&self, address: &str) -> Result<()> {
        if self.lnurl_url != address_url(address)?
            || self.min_msat == 0
            || self.max_msat < self.min_msat
        {
            bail!("recipient binding or amount range mismatch");
        }
        let url = public_network::validate_destination(&self.callback, "https")?;
        for (key, _) in url.query_pairs() {
            if ["amount", "nostr", "lnurl", "comment"].contains(&key.as_ref()) {
                bail!("recipient callback contains reserved payment parameters");
            }
        }
        if let Some(key) = &self.receipt_pubkey {
            crate::nostr_event::validate_public_key(key)?;
        }
        crate::nostr_event::validate_hex(&self.metadata_hash, 32)
    }

    pub fn callback_identity(&self) -> Result<String> {
        Ok(public_network::validate_destination(&self.callback, "https")?.into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn wire() -> serde_json::Value {
        json!({"tag":"payRequest","callback":"https://pay.example.com/invoice?token=private",
            "minSendable":1000,"maxSendable":1000000,
            "metadata":"[[\"text/plain\",\"Treats\"],[\"future\",{\"any\":true}]]"})
    }
    #[test]
    fn exact_raw_metadata_hash_and_explicit_fallback_without_exporting_images() {
        let mut w = wire();
        let r = Recipient::parse("Member@Example.com", &serde_json::to_vec(&w).unwrap()).unwrap();
        assert_eq!(r.lnurl_url, "https://example.com/.well-known/lnurlp/Member");
        assert_eq!(r.receipt_pubkey, None);
        assert_eq!(
            r.metadata_hash,
            hex::encode(Sha256::digest(w["metadata"].as_str().unwrap().as_bytes()))
        );
        w["metadata"] = json!("[ [\"text/plain\", \"Treats\"], [\"future\",{\"any\":true}] ]");
        assert_ne!(
            Recipient::parse("Member@Example.com", &serde_json::to_vec(&w).unwrap())
                .unwrap()
                .metadata_hash,
            r.metadata_hash
        );
        w["metadata"] = json!(format!(
            "[[\"text/plain\",\"Treats\"],[\"image/png;base64\",\"{}\"]]",
            "A".repeat(136536)
        ));
        let raw = serde_json::to_vec(&w).unwrap();
        let r = Recipient::parse("a@example.com", &raw).unwrap();
        assert!(serde_json::to_vec(&r).unwrap().len() < 1024);
    }
    #[test]
    fn explicit_success_status_preserves_recipient_validation_and_error_refusal() {
        let address = "member@example.com";
        let baseline = Recipient::parse(address, &serde_json::to_vec(&wire()).unwrap()).unwrap();
        let mut success = wire();
        success["status"] = json!("OK");
        assert_eq!(
            Recipient::parse(address, &serde_json::to_vec(&success).unwrap()).unwrap(),
            baseline
        );
        let receipt_key = crate::nostr_event::test_event(2, 0, 100, "{}", vec![]).pubkey;
        success["allowsNostr"] = json!(true);
        success["nostrPubkey"] = json!(receipt_key.clone());
        assert_eq!(
            Recipient::parse(address, &serde_json::to_vec(&success).unwrap())
                .unwrap()
                .receipt_pubkey,
            Some(receipt_key)
        );
        for status in ["ERROR", "error", "ok", "", "PENDING", "OK "] {
            let mut rejected = success.clone();
            rejected["status"] = json!(status);
            assert!(
                Recipient::parse(address, &serde_json::to_vec(&rejected).unwrap()).is_err(),
                "{status}"
            );
        }
        for (field, value) in [
            ("tag", json!("withdrawRequest")),
            ("minSendable", json!(0)),
            ("maxSendable", json!(999)),
            ("callback", json!("https://10.8.0.6/invoice")),
            (
                "callback",
                json!("https://pay.example.com/invoice?amount=1000"),
            ),
            (
                "metadata",
                json!("[[\"text/plain\",\"a\"],[\"text/plain\",\"b\"]]"),
            ),
            ("nostrPubkey", json!("00".repeat(32))),
        ] {
            let mut rejected = success.clone();
            rejected[field] = value;
            assert!(
                Recipient::parse(address, &serde_json::to_vec(&rejected).unwrap()).is_err(),
                "{field}"
            );
        }
    }

    #[test]
    fn malformed_supported_recipients_private_callbacks_ambiguous_fields_and_errors_fail_closed() {
        let public = crate::nostr_event::test_event(2, 0, 100, "{}", vec![]).pubkey;
        for (field, value) in [
            ("status", json!("ERROR")),
            ("tag", json!("withdrawRequest")),
            ("minSendable", json!(0)),
            ("maxSendable", json!(999)),
            (
                "metadata",
                json!("[[\"text/plain\",\"a\"],[\"text/plain\",\"b\"]]"),
            ),
            ("metadata", json!("[[\"text/plain\",[]]]")),
            ("metadata", json!("[\"not an array\"]")),
            ("callback", json!("http://pay.example.com/")),
            ("callback", json!("https://10.8.0.6/")),
            ("callback", json!("https://pay.example.com/?%61mount=1000")),
            (
                "callback",
                json!("https://pay.example.com/?nostr=already-selected"),
            ),
            ("allowsNostr", json!(true)),
        ] {
            let mut w = wire();
            w[field] = value;
            assert!(
                Recipient::parse("a@example.com", &serde_json::to_vec(&w).unwrap()).is_err(),
                "{field}"
            );
        }
        let mut w = wire();
        w["allowsNostr"] = json!(true);
        w["nostrPubkey"] = json!(public.clone());
        assert_eq!(
            Recipient::parse("a@example.com", &serde_json::to_vec(&w).unwrap())
                .unwrap()
                .receipt_pubkey,
            Some(public)
        );
        w["nostrPubkey"] = json!("00".repeat(32));
        assert!(Recipient::parse("a@example.com", &serde_json::to_vec(&w).unwrap()).is_err());
        let duplicate =
            serde_json::to_string(&wire())
                .unwrap()
                .replacen('{', "{\"minSendable\":1,", 1);
        assert!(Recipient::parse("a@example.com", duplicate.as_bytes()).is_err());
        assert!(Recipient::parse("a@example.com", &vec![b' '; MAX_RESPONSE + 1]).is_err());
        for address in [
            "x@10.8.0.6",
            "x@host.local",
            "x@host.test",
            "../a@example.com",
            "x@host.onion",
        ] {
            assert!(address_url(address).is_err());
        }
    }
}
