//! Invoice requests are frozen in the payout journal before crossing any network
//! boundary. A validated invoice is not execution authority or a paid receipt.
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use super::recipient::Recipient;
use crate::{nostr_event::VerifiedEvent, public_network};

#[derive(Debug)]
pub(crate) struct DescriptionHashMismatch;
impl std::fmt::Display for DescriptionHashMismatch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("invoice description hash matches neither zap nor recipient metadata")
    }
}
impl std::error::Error for DescriptionHashMismatch {}

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
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub content: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub joining: Option<crate::cyberherd::conversation::PublishedReference>,
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
            || document.content.len() > 2048
        {
            bail!("invalid frozen payout invoice intent");
        }
        if let Some(joining) = &document.joining {
            joining.validate(&document.owner, &document.member)?;
        }
        Ok(Self(document))
    }
    pub fn unsigned_zap(&self) -> Result<Option<UnsignedZap>> {
        let Some(signer) = &self.0.recipient.receipt_pubkey else {
            return Ok(None);
        };
        let mut relays = vec!["relays".into()];
        relays.extend(self.0.relays.clone());
        let mut tags = vec![
            relays,
            vec!["amount".into(), (self.0.net_sats * 1000).to_string()],
            vec!["lnurl".into(), self.lnurl()?],
            vec!["p".into(), self.0.member.clone()],
            vec!["P".into(), signer.clone()],
        ];
        if let Some(joining) = &self.0.joining {
            tags.push(vec![
                "e".into(),
                joining.event.id.clone(),
                joining.relay.clone(),
            ]);
            tags.push(vec!["k".into(), joining.event.kind.to_string()]);
        }
        Ok(Some(UnsignedZap {
            kind: 9734,
            created_at: u64::try_from(self.0.created_at)?,
            content: self.0.content.clone(),
            tags,
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
    pub(crate) fn uses_primal_callback(&self) -> bool {
        let r = &self.intent.0.recipient;
        r.lnurl_url.starts_with("https://primal.net/")
            && r.callback.starts_with("https://primal.net/")
    }

    pub(crate) fn restore(intent: Intent, raw_zap: Option<String>) -> Result<Self> {
        let description_hash = match (intent.unsigned_zap()?, &raw_zap) {
            (_, None) => intent.0.recipient.metadata_hash.clone(),
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
            _ => bail!("recipient does not support the supplied zap"),
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
    pub fn amount_sats(&self) -> u64 {
        self.intent.0.net_sats
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
        if self.intent.0.recipient.comment_allowed > 0 {
            let memo = "CyberHerd Treats!"
                .chars()
                .take(self.intent.0.recipient.comment_allowed.min(256) as usize)
                .collect::<String>();
            query.push(("comment".into(), memo));
        }
        Ok(query)
    }
    pub async fn fetch(&self) -> Result<ValidatedInvoice> {
        let first = self.fetch_query(&self.query()?).await;
        if first.is_ok() || self.raw_zap.is_none() {
            return first;
        }
        // No invoice/quote/execute was retained: retry the same recipient and
        // frozen amount as ordinary Lightning, without changing the signed zap.
        let ordinary: Vec<_> = self
            .query()?
            .into_iter()
            .filter(|(key, _)| key == "amount" || key == "comment")
            .collect();
        self.fetch_query(&ordinary).await
    }
    async fn fetch_query(&self, query: &[(String, String)]) -> Result<ValidatedInvoice> {
        let raw = public_network::get_json_query(
            &self.intent.0.recipient.callback,
            query,
            MAX_INVOICE_RESPONSE,
        )
        .await
        .map_err(|_| {
            tracing::warn!("recipient invoice HTTP request failed");
            anyhow::anyhow!("recipient invoice request failed")
        })?;
        self.validate_response(&raw, crate::cyberherd::calendar::now()?)
            .inspect_err(|error| {
                let reason = match error.to_string().as_str() {
                    "invoice description hash matches neither zap nor recipient metadata" => {
                        "description_hash"
                    }
                    "outgoing invoice is future-dated or lacks remaining validity" => "timestamp",
                    "invalid outgoing BOLT11 invoice" => "invalid_bolt11",
                    "outgoing invoice contradicts frozen request" => "invoice_terms",
                    "invalid recipient invoice response" => "response_format",
                    "recipient rejected invoice request" => "recipient_rejection",
                    _ => "other",
                };
                tracing::warn!(reason, "recipient response failed invoice validation");
            })
    }
    pub(crate) fn validate_response(&self, raw: &[u8], now: i64) -> Result<ValidatedInvoice> {
        let invoice = response_invoice(raw)?;
        self.validate_invoice(&invoice, now)
    }
    pub(crate) fn validate_invoice(&self, encoded: &str, now: i64) -> Result<ValidatedInvoice> {
        validate_signed_invoice(
            self.intent.id(),
            self.intent.0.net_sats,
            &self.description_hash,
            &self.intent.0.recipient.metadata_hash,
            self.raw_zap.is_some(),
            encoded,
            now,
        )
    }
}

fn response_invoice(raw: &[u8]) -> Result<String> {
    #[derive(Deserialize)]
    struct Wire {
        status: Option<String>,
        pr: Option<String>,
    }
    if raw.len() > MAX_INVOICE_RESPONSE {
        bail!("recipient invoice response exceeds bound");
    }
    let wire: Wire = serde_json::from_slice(raw)
        .map_err(|_| anyhow::anyhow!("invalid recipient invoice response"))?;
    if wire.status.as_deref().is_some_and(|status| status != "OK") {
        bail!("recipient rejected invoice request");
    }
    wire.pr.context("invalid recipient invoice response")
}

/// Ordinary LNURL-pay to the configured operator wallet, bound to one feeding.
pub async fn fetch_operator_invoice(
    feed: Uuid,
    address: &str,
    recipient: &Recipient,
    amount_sats: u64,
) -> Result<ValidatedInvoice> {
    recipient.validate(address)?;
    let amount_msat = amount_sats
        .checked_mul(1000)
        .context("operator invoice amount overflow")?;
    if feed.is_nil() || !(recipient.min_msat..=recipient.max_msat).contains(&amount_msat) {
        bail!("operator invoice outside recipient limits");
    }
    let raw = public_network::get_json_query(
        &recipient.callback,
        &[("amount".into(), amount_msat.to_string())],
        MAX_INVOICE_RESPONSE,
    )
    .await
    .map_err(|_| anyhow::anyhow!("operator invoice request failed"))?;
    validate_signed_invoice(
        feed,
        amount_sats,
        &recipient.metadata_hash,
        &recipient.metadata_hash,
        false,
        &response_invoice(&raw)?,
        crate::cyberherd::calendar::now()?,
    )
}

fn validate_signed_invoice(
    intent_id: Uuid,
    amount_sats: u64,
    expected_hash: &str,
    metadata_hash: &str,
    requested_zap: bool,
    encoded: &str,
    now: i64,
) -> Result<ValidatedInvoice> {
    if encoded.is_empty() || encoded.len() > 8192 || now < 0 {
        bail!("invalid outgoing invoice size/time");
    }
    let parsed: lightning_invoice::Bolt11Invoice = encoded
        .parse()
        .map_err(|_| anyhow::anyhow!("invalid outgoing BOLT11 invoice"))?;
    let payment_hash = parsed.payment_hash().to_string();
    use lightning_invoice::Bolt11InvoiceDescriptionRef;
    let (description_hash, nip57) = match parsed.description() {
        Bolt11InvoiceDescriptionRef::Hash(h) if h.0.to_string() == expected_hash => {
            (expected_hash.to_owned(), requested_zap)
        }
        Bolt11InvoiceDescriptionRef::Hash(h) if h.0.to_string() == metadata_hash => {
            (metadata_hash.to_owned(), false)
        }
        Bolt11InvoiceDescriptionRef::Direct(text) => (
            hex::encode(Sha256::digest(text.to_string().as_bytes())),
            false,
        ),
        _ => return Err(DescriptionHashMismatch.into()),
    };
    let parsed = if nip57 {
        crate::bolt11::verify(
            encoded,
            amount_sats * 1000,
            &payment_hash,
            &description_hash,
        )
    } else {
        crate::bolt11::verify_payout(
            encoded,
            amount_sats * 1000,
            &payment_hash,
            &description_hash,
        )
    }
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
        intent_id,
        encoded: encoded.into(),
        payment_hash,
        description_hash,
        nip57,
        amount_sats,
        expires_at,
        node_pubkey: parsed.recover_payee_pub_key().to_string(),
    })
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
    // Derived again from retained signed bytes on every restore; preserve legacy JSON.
    #[serde(skip)]
    nip57: bool,
}

impl ValidatedInvoice {
    pub fn intent_id(&self) -> Uuid {
        self.intent_id
    }
    pub(crate) fn nip57(&self) -> bool {
        self.nip57
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
            content: String::new(),
            joining: None,
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
    fn outgoing_zap_freezes_editable_wording_and_verified_joining_chat_kind_without_publishing_it()
    {
        let mut document = intent(true).0;
        document.content = "🐐 Frozen operator wording: 5 sats".into();
        let joining = crate::nostr_event::test_event(
            1,
            1311,
            999,
            "Welcome Alice",
            vec![
                vec!["p".into(), document.member.clone()],
                vec![
                    "a".into(),
                    format!("30311:{}:saved-stream", document.owner),
                    String::new(),
                    "root".into(),
                ],
            ],
        );
        document.joining = Some(crate::cyberherd::conversation::PublishedReference {
            event: joining.clone(),
            relay: "wss://relay.damus.io/".into(),
            acknowledged_at: 1000,
        });
        let frozen = Intent::restore(document).unwrap();
        let unsigned = frozen.unsigned_zap().unwrap().unwrap();
        assert_eq!(unsigned.content, "🐐 Frozen operator wording: 5 sats");
        assert!(unsigned.tags.contains(&vec![
            "e".into(),
            joining.id.clone(),
            "wss://relay.damus.io/".into()
        ]));
        assert!(unsigned.tags.contains(&vec!["k".into(), "1311".into()]));
        let raw = signed(&frozen);
        let request = Request::restore(frozen.clone(), Some(raw.clone())).unwrap();
        assert_eq!(
            request.description_hash(),
            hex::encode(Sha256::digest(raw.as_bytes()))
        );
        let mut changed = frozen.0.clone();
        changed.content = "new wording".into();
        assert!(Request::restore(Intent::restore(changed).unwrap(), Some(raw)).is_err());
        let ordinary = intent(true).unsigned_zap().unwrap().unwrap();
        assert!(
            !ordinary
                .tags
                .iter()
                .any(|t| t.first().is_some_and(|name| name == "e"))
        );
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
    fn explicit_success_wrapper_still_requires_the_exact_signed_invoice() {
        for nostr in [false, true] {
            let intent = intent(nostr);
            let signed = nostr.then(|| signed(&intent));
            let request = Request::restore(intent, signed).unwrap();
            let invoice = crate::test_invoices::invoice_at(
                5000,
                request.description_hash(),
                Duration::from_secs(1000),
            );
            let ordinary = request
                .validate_response(&serde_json::to_vec(&json!({"pr":invoice})).unwrap(), 1000)
                .unwrap();
            let explicit = request
                .validate_response(
                    &serde_json::to_vec(&json!({"status":"OK","pr":invoice})).unwrap(),
                    1000,
                )
                .unwrap();
            assert_eq!(explicit, ordinary);
            for status in ["ERROR", "PENDING", "ok", "OK ", ""] {
                assert!(
                    request
                        .validate_response(
                            &serde_json::to_vec(&json!({"status":status,"pr":invoice})).unwrap(),
                            1000
                        )
                        .is_err(),
                    "{status}"
                );
            }
            for (amount, description, created) in [
                (6000, request.description_hash().to_owned(), 1000),
                (5000, "ab".repeat(32), 1000),
                (5000, request.description_hash().to_owned(), 1031),
                (5000, request.description_hash().to_owned(), 500),
            ] {
                let wrong = crate::test_invoices::invoice_at(
                    amount,
                    &description,
                    Duration::from_secs(created),
                );
                assert!(
                    request
                        .validate_response(
                            &serde_json::to_vec(&json!({"status":"OK","pr":wrong})).unwrap(),
                            1000
                        )
                        .is_err()
                );
            }
            assert!(
                request
                    .validate_response(br#"{"status":"OK","pr":"invalid-invoice"}"#, 1000)
                    .is_err()
            );
        }
    }

    fn plain_invoice(amount: u64) -> String {
        use bitcoin::{
            hashes::{Hash as _, sha256},
            secp256k1::{Secp256k1, SecretKey},
        };
        let key = SecretKey::from_slice(&[42; 32]).unwrap();
        lightning_invoice::InvoiceBuilder::new(lightning_invoice::Currency::Bitcoin)
            .amount_milli_satoshis(amount)
            .description("CyberHerd Treats!".into())
            .payee_pub_key(bitcoin::secp256k1::PublicKey::from_secret_key(
                &Secp256k1::new(),
                &key,
            ))
            .payment_hash(sha256::Hash::from_slice(&[0x22; 32]).unwrap())
            .payment_secret(lightning_invoice::PaymentSecret([0x44; 32]))
            .duration_since_epoch(Duration::from_secs(1000))
            .expiry_time(Duration::from_secs(300))
            .min_final_cltv_expiry_delta(144)
            .build_signed(|hash| Secp256k1::new().sign_ecdsa_recoverable(hash, &key))
            .unwrap()
            .to_string()
    }
    #[test]
    fn zap_preference_accepts_valid_ordinary_invoices_without_fabricating_zap_support() {
        let i = intent(true);
        let raw = signed(&i);
        let request = Request::restore(i.clone(), Some(raw)).unwrap();
        let plain = plain_invoice(5000);
        let invoice = request.validate_invoice(&plain, 1000).unwrap();
        assert!(!invoice.nip57());
        assert_eq!(
            invoice.description_hash(),
            hex::encode(Sha256::digest(b"CyberHerd Treats!"))
        );
        assert!(
            crate::bolt11::verify(
                &plain,
                5000,
                invoice.payment_hash(),
                invoice.description_hash()
            )
            .is_err()
        );
        assert!(
            crate::bolt11::verify_payout(
                &plain,
                5000,
                invoice.payment_hash(),
                invoice.description_hash()
            )
            .is_ok()
        );
        assert!(
            request
                .validate_invoice(&plain_invoice(6000), 1000)
                .is_err()
        );
        assert!(request.validate_invoice(&plain, 1271).is_err());
        let parsed: lightning_invoice::Bolt11Invoice = plain.parse().unwrap();
        let (original, _, _) = parsed.into_signed_raw().into_parts();
        let key = bitcoin::secp256k1::SecretKey::from_slice(&[42; 32]).unwrap();
        let mut testnet = original.clone();
        testnet.hrp.currency = lightning_invoice::Currency::BitcoinTestnet;
        let wrong_network = testnet
            .sign(|hash| {
                Ok::<_, std::convert::Infallible>(
                    bitcoin::secp256k1::Secp256k1::new().sign_ecdsa_recoverable(hash, &key),
                )
            })
            .unwrap()
            .to_string();
        assert!(request.validate_invoice(&wrong_network, 1000).is_err());
        let wrong_key = bitcoin::secp256k1::SecretKey::from_slice(&[43; 32]).unwrap();
        let wrong_signature = original
            .sign(|hash| {
                Ok::<_, std::convert::Infallible>(
                    bitcoin::secp256k1::Secp256k1::new().sign_ecdsa_recoverable(hash, &wrong_key),
                )
            })
            .unwrap()
            .to_string();
        assert!(request.validate_invoice(&wrong_signature, 1000).is_err());

        let metadata = crate::test_invoices::invoice_at(
            5000,
            &i.0.recipient.metadata_hash,
            Duration::from_secs(1000),
        );
        assert!(!request.validate_invoice(&metadata, 1000).unwrap().nip57());
        let zap = crate::test_invoices::invoice_at(
            5000,
            request.description_hash(),
            Duration::from_secs(1000),
        );
        assert!(request.validate_invoice(&zap, 1000).unwrap().nip57());
        assert!(
            serde_json::to_value(invoice)
                .unwrap()
                .get("nip57")
                .is_none()
        );
    }
    #[test]
    fn comments_respect_recipient_advertised_limit_and_old_recipient_bytes() {
        let mut i = intent(false);
        let original = serde_json::to_string(&i.0.recipient).unwrap();
        assert!(!original.contains("comment_allowed"));
        i.0.recipient.comment_allowed = 8;
        let request = Request::restore(i, None).unwrap();
        assert!(
            request
                .query()
                .unwrap()
                .contains(&("comment".into(), "CyberHer".into()))
        );
    }
    #[test]
    fn supported_zap_prefers_exact_signed_json_and_allows_unsigned_fallback() {
        let i = intent(true);
        let raw = signed(&i);
        assert!(Request::restore(i.clone(), None).is_ok());
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
        assert_eq!(
            response_invoice(br#"{"status":"ERROR","reason":"synthetic-private"}"#)
                .unwrap_err()
                .to_string(),
            "recipient rejected invoice request"
        );
        assert!(response_invoice(br#"{"status":"OK"}"#).is_err());
        let mut doc = i.0;
        doc.recipient.callback = "https://10.8.0.6/callback".into();
        assert!(Intent::restore(doc).is_err());
        assert!(receipt_relays(&[]).is_err());
        assert!(receipt_relays(&["wss://192.168.1.1/".into()]).is_err());
    }
}
