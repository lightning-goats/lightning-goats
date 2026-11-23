//! NIP-98 authentication is request authority only, never payment evidence.
use crate::nostr_event::VerifiedEvent;
use anyhow::{Context as _, Result, bail};
use base64::{
    Engine as _,
    engine::general_purpose::{STANDARD, STANDARD_NO_PAD},
};
use serde::Deserialize;
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub public_origin: String,
    pub operators: Vec<String>,
}
impl Config {
    pub fn validate(&self) -> Result<()> {
        let url = crate::public_network::validate_destination(&self.public_origin, "https")?;
        if url.origin().ascii_serialization() != self.public_origin
            || url.path() != "/"
            || url.query().is_some()
            || url.fragment().is_some()
            || self.operators.len() > 16
        {
            bail!(
                "HTTP authentication requires a canonical public HTTPS origin and bounded operator allowlist"
            );
        }
        let mut keys = std::collections::BTreeSet::new();
        for key in &self.operators {
            crate::nostr_event::validate_public_key(key)?;
            if !keys.insert(key) {
                bail!("duplicate operator identity");
            }
        }
        Ok(())
    }
    pub fn operator(&self, key: &str) -> bool {
        self.operators.iter().any(|p| p == key)
    }
    pub fn verify_operator(
        &self,
        header: &str,
        uri: &str,
        body: &[u8],
        now: i64,
    ) -> Result<OperatorAuthorization> {
        if uri != "/api/v1/cyberherd/operator/command" {
            bail!("invalid operator command URL");
        }
        let proof = self.verify(header, "POST", uri, body, now)?;
        if !self.operator(&proof.actor) {
            bail!("operator authority required");
        }
        Ok(OperatorAuthorization(proof))
    }
    pub fn verify(
        &self,
        header: &str,
        method: &str,
        uri: &str,
        body: &[u8],
        now: i64,
    ) -> Result<Authorization> {
        self.validate()?;
        if header.len() > 8192
            || uri.len() > 2048
            || !uri.starts_with('/')
            || uri.starts_with("//")
            || !matches!(method, "GET" | "POST")
            || body.len() > 16384
            || now < 0
        {
            bail!("invalid HTTP authentication bounds");
        }
        let encoded = header
            .strip_prefix("Nostr ")
            .context("Nostr authentication required")?;
        let bytes = STANDARD
            .decode(encoded)
            .or_else(|_| STANDARD_NO_PAD.decode(encoded))?;
        let verified = VerifiedEvent::from_json(std::str::from_utf8(&bytes)?)?;
        let e = verified.event();
        if e.kind != 27235
            || !e.content.is_empty()
            || (i64::try_from(e.created_at)? - now).abs() > 60
            || e.tags.len() > 4
            || e.tags.iter().any(|t| {
                t.len() != 2 || !matches!(t[0].as_str(), "u" | "method" | "payload" | "nonce")
            })
            || verified.singleton_tag("u", true)?
                != Some(format!("{}{}", self.public_origin, uri).as_str())
            || verified.singleton_tag("method", true)? != Some(method)
        {
            bail!("HTTP authentication does not match request");
        }
        let hash = hex::encode(Sha256::digest(body));
        let payload = verified.singleton_tag("payload", method == "POST")?;
        if (method == "POST" && payload != Some(hash.as_str()))
            || (method == "GET" && (!body.is_empty() || payload.is_some()))
        {
            bail!("HTTP authentication body changed");
        }
        if let Some(nonce) = verified.singleton_tag("nonce", false)? {
            if nonce.len() > 128 {
                bail!("oversized authentication nonce");
            }
        }
        Ok(Authorization {
            actor: e.pubkey.clone(),
            id: e.id.clone(),
            expires: i64::try_from(e.created_at)?.saturating_add(61),
            body_hash: hash,
        })
    }
}
/// Cannot be deserialized or constructed by an HTTP payload.
pub struct Authorization {
    pub(crate) actor: String,
    pub(crate) id: String,
    pub(crate) expires: i64,
    pub(crate) body_hash: String,
}

/// Only a pinned allowlist and an exact command request create this capability.
pub struct OperatorAuthorization(Authorization);
impl OperatorAuthorization {
    pub(crate) fn authorization(&self) -> &Authorization {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nostr_event::test_event;
    fn config() -> Config {
        Config {
            public_origin: "https://example.com".into(),
            operators: vec![test_event(1, 0, 100, "", vec![]).pubkey],
        }
    }
    fn header(kind: u64, at: u64, tags: Vec<Vec<String>>) -> String {
        format!(
            "Nostr {}",
            STANDARD.encode(serde_json::to_vec(&test_event(1, kind, at, "", tags)).unwrap())
        )
    }
    #[test]
    fn native_http_signature_binds_exact_origin_method_query_payload_time_and_operator() {
        let c = config();
        let body = b"{\"id\":1}";
        let path = "/api/v1/cyberherd/operator/command";
        let tags = vec![
            vec!["u".into(), format!("{}{}", c.public_origin, path)],
            vec!["method".into(), "POST".into()],
            vec!["payload".into(), hex::encode(Sha256::digest(body))],
        ];
        let good = header(27235, 100, tags.clone());
        assert!(c.verify_operator(&good, path, body, 100).is_ok());
        for (method, uri, data, now) in [
            ("GET", path, body.as_slice(), 100),
            (
                "POST",
                "/api/v1/cyberherd/operator/command?x=1",
                body.as_slice(),
                100,
            ),
            ("POST", path, b"different".as_slice(), 100),
            ("POST", path, body.as_slice(), 161),
            ("POST", path, body.as_slice(), 39),
        ] {
            assert!(c.verify(&good, method, uri, data, now).is_err());
        }
        let mut wrong = config();
        wrong.public_origin = "https://other.example.com".into();
        assert!(wrong.verify_operator(&good, path, body, 100).is_err());
        let mut nonoperator = config();
        nonoperator.operators.clear();
        assert!(nonoperator.verify_operator(&good, path, body, 100).is_err());
        let mut duplicate = tags.clone();
        duplicate.push(tags[0].clone());
        assert!(
            c.verify_operator(&header(27235, 100, duplicate), path, body, 100)
                .is_err()
        );
        assert!(
            c.verify_operator(&header(1, 100, tags.clone()), path, body, 100)
                .is_err()
        );
        assert!(
            c.verify_operator(&header(27235, 100, tags[..2].to_vec()), path, body, 100)
                .is_err()
        );
        assert!(c.verify("Nostr !!!", "GET", "/", &[], 100).is_err());
        let mut event = test_event(1, 27235, 100, "", tags);
        event.sig.replace_range(..2, "ff");
        assert!(
            c.verify_operator(
                &format!(
                    "Nostr {}",
                    STANDARD.encode(serde_json::to_vec(&event).unwrap())
                ),
                path,
                body,
                100
            )
            .is_err()
        );
    }
    #[test]
    fn get_has_no_body_and_canonical_origin_never_trusts_forwarded_host() {
        let c = config();
        let h = header(
            27235,
            100,
            vec![
                vec!["u".into(), "https://example.com/api/v1/cyberherd/me".into()],
                vec!["method".into(), "GET".into()],
            ],
        );
        assert!(
            c.verify(&h, "GET", "/api/v1/cyberherd/me", &[], 100)
                .is_ok()
        );
        assert!(
            c.verify(&h, "HEAD", "/api/v1/cyberherd/me", &[], 100)
                .is_err()
        );
        assert!(
            c.verify(&h, "GET", "/api/v1/cyberherd/me", b"x", 100)
                .is_err()
        );
        for origin in [
            "http://example.com",
            "https://example.com/",
            "https://example.com/path",
            "https://example.com?x=1",
            "https://10.8.0.6",
            "https://user@example.com",
        ] {
            let mut invalid = c.clone();
            invalid.public_origin = origin.into();
            assert!(invalid.validate().is_err(), "{origin}");
        }
    }
}

#[derive(Debug)]
pub(crate) struct Rejected;
impl std::fmt::Display for Rejected {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("HTTP authorization expired or replayed")
    }
}
impl std::error::Error for Rejected {}
