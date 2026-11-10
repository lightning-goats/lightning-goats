//! Invoice requests are frozen in the payout journal before crossing any network
//! boundary. A validated invoice is not execution authority or a paid receipt.
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use super::recipient::Recipient;
use crate::{nostr_event::VerifiedEvent, public_network};

pub const MAX_INVOICE_RESPONSE: usize = 16_384;

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct Document {
    pub id: Uuid,
    pub grant_id: Uuid,
    pub owner: String,
    pub member: String,
    pub address: String,
    pub recipient: Recipient,
    pub net_sats: u64,
    pub created_at: i64,
    pub relays: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct UnsignedZap {
    pub kind: u64,
    pub created_at: u64,
    pub content: String,
    pub tags: Vec<Vec<String>>,
}

#[derive(Clone)]
pub struct Intent(pub(crate) Document);

impl Intent {
    pub fn id(&self) -> Uuid {
        self.0.id
    }
    pub fn grant_id(&self) -> Uuid {
        self.0.grant_id
    }
    pub fn member(&self) -> &str {
        &self.0.member
    }
    pub fn uses_nostr(&self) -> bool {
        self.0.recipient.receipt_pubkey.is_some()
    }
    pub(crate) fn restore(document: Document) -> Result<Self> {
        crate::nostr_event::validate_public_key(&document.owner)?;
        crate::nostr_event::validate_public_key(&document.member)?;
        document.recipient.validate(&document.address)?;
        let amount = document
            .net_sats
            .checked_mul(1000)
            .context("invoice amount overflow")?;
        if document.id.is_nil()
            || document.grant_id.is_nil()
            || document.owner == document.member
            || document.created_at < 0
            || amount == 0
            || amount > i64::MAX as u64
            || amount < document.recipient.min_msat
            || amount > document.recipient.max_msat
            || document.relays != receipt_relays(&document.relays)?
        {
            bail!("invalid frozen payout invoice intent");
        }
        Ok(Self(document))
    }
    pub fn unsigned_zap(&self) -> Result<Option<UnsignedZap>> {
        let Some(signer) = &self.0.recipient.receipt_pubkey else {
            return Ok(None);
        };
        let mut relays = vec!["relays".into()];
        relays.extend(self.0.relays.clone());
        Ok(Some(UnsignedZap {
            kind: 9734,
            created_at: u64::try_from(self.0.created_at)?,
            content: String::new(),
            tags: vec![
                relays,
                vec!["amount".into(), (self.0.net_sats * 1000).to_string()],
                vec!["lnurl".into(), self.lnurl()?],
                vec!["p".into(), self.0.member.clone()],
                vec!["P".into(), signer.clone()],
            ],
        }))
    }
    fn lnurl(&self) -> Result<String> {
        Ok(bitcoin::bech32::encode::<bitcoin::bech32::Bech32>(
            bitcoin::bech32::Hrp::parse("lnurl")?,
            self.0.recipient.lnurl_url.as_bytes(),
        )?)
    }
}

pub(crate) fn receipt_relays(raw: &[String]) -> Result<Vec<String>> {
    if raw.is_empty() || raw.len() > 8 {
        bail!("payout requires bounded receipt relays");
    }
    raw.iter()
        .map(|url| Ok(public_network::validate_destination(url, "wss")?.to_string()))
        .collect::<Result<std::collections::BTreeSet<_>>>()
        .map(|urls| urls.into_iter().collect())
}

/// Only the journal restores this after its raw zap bytes (or explicit
/// unsupported-recipient fallback) have been durably frozen.
#[derive(Clone)]
pub struct Request {
    intent: Intent,
    raw_zap: Option<String>,
    description_hash: String,
}

impl Request {
    pub(crate) fn restore(intent: Intent, raw_zap: Option<String>) -> Result<Self> {
        let description_hash = match (intent.unsigned_zap()?, &raw_zap) {
            (None, None) => intent.0.recipient.metadata_hash.clone(),
            (Some(expected), Some(raw)) => {
                if raw.len() > crate::zap::MAX_ZAP_REQUEST_BYTES {
                    bail!("payout zap exceeds bound");
                }
                let verified = VerifiedEvent::from_json(raw)
                    .map_err(|_| anyhow::anyhow!("invalid payout zap signature"))?;
                let event = verified.event();
                if event.pubkey != intent.0.owner
                    || event.kind != expected.kind
                    || event.created_at != expected.created_at
                    || event.content != expected.content
                    || event.tags != expected.tags
                {
                    bail!("signer changed frozen payout request");
                }
                hex::encode(Sha256::digest(raw.as_bytes()))
            }
            _ => bail!("supported recipient requires its exact signed zap; no downgrade"),
        };
        Ok(Self {
            intent,
            raw_zap,
            description_hash,
        })
    }
    pub fn intent_id(&self) -> Uuid {
        self.intent.id()
    }
    pub fn raw_zap(&self) -> Option<&str> {
        self.raw_zap.as_deref()
    }
    pub fn description_hash(&self) -> &str {
        &self.description_hash
    }
    fn query(&self) -> Result<Vec<(String, String)>> {
        let mut query = vec![("amount".into(), (self.intent.0.net_sats * 1000).to_string())];
        if let Some(raw) = &self.raw_zap {
            query.push(("nostr".into(), raw.clone()));
            query.push(("lnurl".into(), self.intent.lnurl()?));
        }
        Ok(query)
    }
    pub async fn fetch(&self) -> Result<ValidatedInvoice> {
        let raw = public_network::get_json_query(
            &self.intent.0.recipient.callback,
            &self.query()?,
            MAX_INVOICE_RESPONSE,
        )
        .await
        .map_err(|_| anyhow::anyhow!("recipient invoice request failed"))?;
        self.validate_response(&raw, crate::cyberherd::calendar::now()?)
    }
    pub(crate) fn validate_response(&self, raw: &[u8], now: i64) -> Result<ValidatedInvoice> {
        #[derive(Deserialize)]
        struct Wire {
            status: Option<String>,
            pr: String,
        }
        if raw.len() > MAX_INVOICE_RESPONSE {
            bail!("recipient invoice response exceeds bound");
        }
        let wire: Wire = serde_json::from_slice(raw)
            .map_err(|_| anyhow::anyhow!("invalid recipient invoice response"))?;
        if wire.status.is_some() {
            bail!("recipient rejected invoice request");
        }
        self.validate_invoice(&wire.pr, now)
    }
    pub(crate) fn validate_invoice(&self, encoded: &str, now: i64) -> Result<ValidatedInvoice> {
        if encoded.is_empty() || encoded.len() > 8192 || now < 0 {
            bail!("invalid outgoing invoice size/time");
        }
        let parsed: lightning_invoice::Bolt11Invoice = encoded
            .parse()
            .map_err(|_| anyhow::anyhow!("invalid outgoing BOLT11 invoice"))?;
        let payment_hash = parsed.payment_hash().to_string();
        let parsed = crate::bolt11::verify(
            encoded,
            self.intent.0.net_sats * 1000,
            &payment_hash,
            &self.description_hash,
        )
        .map_err(|_| anyhow::anyhow!("outgoing invoice contradicts frozen request"))?;
        let created = parsed.duration_since_epoch().as_secs();
        let expires_at = parsed
            .expires_at()
            .context("outgoing invoice has invalid expiry")?
            .as_secs();
        let now = u64::try_from(now)?;
        if created > now.saturating_add(30) || expires_at < now.saturating_add(30) {
            bail!("outgoing invoice is future-dated or lacks remaining validity");
        }
        Ok(ValidatedInvoice {
            intent_id: self.intent.id(),
            encoded: encoded.into(),
            payment_hash,
            description_hash: self.description_hash.clone(),
            amount_sats: self.intent.0.net_sats,
            expires_at,
            node_pubkey: parsed.recover_payee_pub_key().to_string(),
        })
    }
}

/// Created by signed invoice validation, not by deserializing provider wrappers.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ValidatedInvoice {
    intent_id: Uuid,
    encoded: String,
    payment_hash: String,
    description_hash: String,
    amount_sats: u64,
    expires_at: u64,
    node_pubkey: String,
}

impl ValidatedInvoice {
    pub fn intent_id(&self) -> Uuid {
        self.intent_id
    }
    pub fn encoded(&self) -> &str {
        &self.encoded
    }
    pub fn payment_hash(&self) -> &str {
        &self.payment_hash
    }
    pub fn description_hash(&self) -> &str {
        &self.description_hash
    }
    pub fn amount_sats(&self) -> u64 {
        self.amount_sats
    }
    pub fn expires_at(&self) -> u64 {
        self.expires_at
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::time::Duration;

    fn intent(nostr: bool) -> Intent {
        let owner = crate::nostr_event::test_event(1, 1, 1000, "", vec![]).pubkey;
        let member = crate::nostr_event::test_event(2, 1, 1000, "", vec![]).pubkey;
        let signer = crate::nostr_event::test_event(3, 1, 1000, "", vec![]).pubkey;
        let wire = json!({"tag":"payRequest","minSendable":1000,"maxSendable":1000000,
            "callback":"https://pay.example.com/callback?token=synthetic-private",
            "metadata":"[[\"text/plain\",\"CyberHerd treats\"]]","allowsNostr":nostr,"nostrPubkey":signer});
        Intent::restore(Document {
            id: Uuid::new_v4(),
            grant_id: Uuid::new_v4(),
            owner,
            member,
            address: "member@example.com".into(),
            recipient: Recipient::parse("member@example.com", &serde_json::to_vec(&wire).unwrap())
                .unwrap(),
            net_sats: 5,
            created_at: 1000,
            relays: receipt_relays(&["wss://relay.damus.io/".into(), "wss://nos.lol/".into()])
                .unwrap(),
        })
        .unwrap()
    }
    fn signed(i: &Intent) -> String {
        let u = i.unsigned_zap().unwrap().unwrap();
        serde_json::to_string(&crate::nostr_event::test_event(
            1,
            u.kind,
            u.created_at,
            &u.content,
            u.tags,
        ))
        .unwrap()
    }
    #[test]
    fn explicit_ordinary_fallback_uses_raw_metadata_and_checks_exact_invoice_amount_hash_and_time()
    {
        let i = intent(false);
        assert!(!i.uses_nostr());
        assert!(i.unsigned_zap().unwrap().is_none());
        let request = Request::restore(i.clone(), None).unwrap();
        assert_eq!(request.description_hash(), i.0.recipient.metadata_hash);
        assert_eq!(
            request.query().unwrap(),
            vec![("amount".into(), "5000".into())]
        );
        let invoice = crate::test_invoices::invoice_at(
            5000,
            request.description_hash(),
            Duration::from_secs(1000),
        );
        let valid = request
            .validate_response(&serde_json::to_vec(&json!({"pr":invoice})).unwrap(), 1000)
            .unwrap();
        assert_eq!(valid.intent_id(), i.id());
        assert_eq!(valid.amount_sats(), 5);
        assert_eq!(valid.expires_at(), 1300);
        assert!(request.validate_invoice(valid.encoded(), 1271).is_err());
        for (amount, hash, time) in [
            (6000, request.description_hash().to_owned(), 1000),
            (5000, "ab".repeat(32), 1000),
            (5000, request.description_hash().to_owned(), 1031),
        ] {
            let wrong = crate::test_invoices::invoice_at(amount, &hash, Duration::from_secs(time));
            assert!(request.validate_invoice(&wrong, 1000).is_err());
        }
        assert!(Request::restore(i, Some("{}".into())).is_err());
    }
    #[test]
    fn supported_zap_preserves_exact_signed_json_and_never_downgrades_or_changes_intent() {
        let i = intent(true);
        let raw = signed(&i);
        assert!(Request::restore(i.clone(), None).is_err());
        let request = Request::restore(i.clone(), Some(raw.clone())).unwrap();
        assert_eq!(request.raw_zap(), Some(raw.as_str()));
        assert_eq!(
            request.description_hash(),
            hex::encode(Sha256::digest(raw.as_bytes()))
        );
        let pretty =
            serde_json::to_string_pretty(&serde_json::from_str::<serde_json::Value>(&raw).unwrap())
                .unwrap();
        let pretty_request = Request::restore(i.clone(), Some(pretty.clone())).unwrap();
        assert_eq!(pretty_request.raw_zap(), Some(pretty.as_str()));
        assert_ne!(
            request.description_hash(),
            pretty_request.description_hash()
        );
        let query = request.query().unwrap();
        assert_eq!(query[1], ("nostr".into(), raw.clone()));
        let zap = crate::zap::ZapRequest::validate(
            &raw,
            &i.0.member,
            i.0.recipient.receipt_pubkey.as_deref().unwrap(),
            5000,
        )
        .unwrap();
        zap.validate_lnurl_binding(Some(&query[2].1), &[i.0.recipient.lnurl_url.clone()])
            .unwrap();
        let invoice = crate::test_invoices::invoice_at(
            5000,
            request.description_hash(),
            Duration::from_secs(1000),
        );
        assert!(request.validate_invoice(&invoice, 1000).is_ok());
        assert!(pretty_request.validate_invoice(&invoice, 1000).is_err());
        let mut changed = i.unsigned_zap().unwrap().unwrap();
        changed.content = "changed".into();
        let wrong = crate::nostr_event::test_event(
            1,
            changed.kind,
            changed.created_at,
            &changed.content,
            changed.tags.clone(),
        );
        assert!(Request::restore(i.clone(), Some(serde_json::to_string(&wrong).unwrap())).is_err());
        let wrong =
            crate::nostr_event::test_event(2, changed.kind, changed.created_at, "", changed.tags);
        assert!(Request::restore(i, Some(serde_json::to_string(&wrong).unwrap())).is_err());
    }
    #[test]
    fn malformed_oversized_error_and_private_contexts_fail_without_echoing_provider_body() {
        let i = intent(false);
        let request = Request::restore(i.clone(), None).unwrap();
        for raw in [
            b"not-json synthetic-private".to_vec(),
            vec![b' '; MAX_INVOICE_RESPONSE + 1],
            serde_json::to_vec(
                &json!({"status":"ERROR","pr":"synthetic-private","reason":"synthetic-private"}),
            )
            .unwrap(),
        ] {
            let error = request.validate_response(&raw, 1000).unwrap_err();
            assert!(!format!("{error:#}").contains("synthetic-private"));
        }
        let mut doc = i.0;
        doc.recipient.callback = "https://10.8.0.6/callback".into();
        assert!(Intent::restore(doc).is_err());
        assert!(receipt_relays(&[]).is_err());
        assert!(receipt_relays(&["wss://192.168.1.1/".into()]).is_err());
    }
}
