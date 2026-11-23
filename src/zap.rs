//! Native NIP-57 request validation. The exact raw JSON is invoice-bound and
//! immutable; a request is never itself a settlement or a publicly posted note.
use std::collections::BTreeSet;

use anyhow::{Context, Result, bail};
use sha2::{Digest, Sha256};

use crate::{
    nostr_event::{Coordinate, VerifiedEvent, validate_hex, validate_public_key},
    public_network::validate_destination,
};

pub const MAX_ZAP_REQUEST_BYTES: usize = 8_192;

#[derive(Debug, Clone)]
pub struct ZapRequest {
    raw_json: String,
    event: VerifiedEvent,
    receipt_relays: Vec<String>,
    payee: String,
    receipt_signer: String,
    amount_msat: u64,
}

impl ZapRequest {
    pub fn validate(
        raw_json: &str,
        payee: &str,
        receipt_signer: &str,
        amount_msat: u64,
    ) -> Result<Self> {
        validate_public_key(payee)?;
        validate_public_key(receipt_signer)?;
        if raw_json.len() > MAX_ZAP_REQUEST_BYTES
            || amount_msat == 0
            || !amount_msat.is_multiple_of(1000)
            || amount_msat > i64::MAX as u64
        {
            bail!("invalid zap request size or amount");
        }
        let event = VerifiedEvent::from_json(raw_json)?;
        if event.event().kind != 9734 {
            bail!("LNURL nostr parameter must contain a signed zap request");
        }
        if event.singleton_tag("p", true)? != Some(payee) {
            bail!("zap request targets another payee");
        }
        if let Some(value) = event.singleton_tag("P", false)? {
            if value != receipt_signer {
                bail!("zap request selects another receipt signer");
            }
        }
        if let Some(value) = event.singleton_tag("amount", false)? {
            if value.parse::<u64>()? != amount_msat || value != amount_msat.to_string() {
                bail!("zap request amount differs from the callback amount");
            }
        }
        if let Some(id) = event.singleton_tag("e", false)? {
            validate_hex(id, 32)?;
        }
        if let Some(value) = event.singleton_tag("a", false)? {
            value.parse::<Coordinate>()?;
        }
        if let Some(value) = event.singleton_tag("k", false)? {
            let kind = value.parse::<u16>()?;
            if kind.to_string() != value {
                bail!("invalid zap target kind");
            }
        }
        if let Some(value) = event.singleton_tag("lnurl", false)? {
            // Binding to the actual registered LNURL is enforced by the receive
            // adapter. Bound the optional tag before retaining any request.
            if value.len() > 2048 {
                bail!("zap LNURL exceeds size bound");
            }
        }
        let mut relay_tags = event
            .event()
            .tags
            .iter()
            .filter(|tag| tag.first().is_some_and(|name| name == "relays"));
        let relays = relay_tags
            .next()
            .context("zap request omitted receipt relays")?;
        if relay_tags.next().is_some() || !(2..=9).contains(&relays.len()) {
            bail!("zap request requires one bounded receipt-relays tag");
        }
        let mut receipt_relays = Vec::new();
        let mut seen = BTreeSet::new();
        for raw in &relays[1..] {
            let url = validate_destination(raw, "wss")?.to_string();
            if seen.insert(url.clone()) {
                receipt_relays.push(url);
            }
        }
        Ok(Self {
            raw_json: raw_json.into(),
            event,
            receipt_relays,
            payee: payee.into(),
            receipt_signer: receipt_signer.into(),
            amount_msat,
        })
    }

    pub fn raw_json(&self) -> &str {
        &self.raw_json
    }
    pub fn payee(&self) -> &str {
        &self.payee
    }
    pub fn receipt_signer(&self) -> &str {
        &self.receipt_signer
    }
    pub fn amount_msat(&self) -> u64 {
        self.amount_msat
    }

    pub fn validate_lnurl_binding(
        &self,
        supplied: Option<&str>,
        expected: &[String],
    ) -> Result<()> {
        for encoded in [supplied, self.event.singleton_tag("lnurl", false)?]
            .into_iter()
            .flatten()
        {
            let parsed = bitcoin::bech32::primitives::decode::CheckedHrpstring::new::<
                bitcoin::bech32::Bech32,
            >(encoded)
            .context("invalid LNURL Bech32 encoding")?;
            if !parsed.hrp().as_str().eq_ignore_ascii_case("lnurl") {
                bail!("zap LNURL uses the wrong prefix");
            }
            let bytes: Vec<_> = parsed.byte_iter().collect();
            let decoded = String::from_utf8(bytes).context("LNURL is not UTF-8")?;
            let canonical = validate_destination(&decoded, "https")?.to_string();
            if !expected.contains(&canonical) {
                bail!("zap LNURL targets another Lightning address");
            }
        }
        Ok(())
    }
    pub fn event(&self) -> &VerifiedEvent {
        &self.event
    }
    pub fn receipt_relays(&self) -> &[String] {
        &self.receipt_relays
    }
    pub fn description_hash(&self) -> String {
        hex::encode(Sha256::digest(self.raw_json.as_bytes()))
    }

    pub fn receipt_tags(&self, invoice: &str) -> Result<Vec<Vec<String>>> {
        if invoice.is_empty() || invoice.len() > 8_192 {
            bail!("invalid receipt invoice length");
        }
        let mut tags = Vec::new();
        for name in ["p", "e", "a", "k"] {
            if let Some(value) = self.event.singleton_tag(name, false)? {
                tags.push(vec![name.into(), value.into()]);
            }
        }
        tags.push(vec!["P".into(), self.event.event().pubkey.clone()]);
        tags.push(vec!["bolt11".into(), invoice.into()]);
        tags.push(vec!["description".into(), self.raw_json.clone()]);
        Ok(tags)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nostr_event::test_event;

    #[test]
    fn accepts_independently_signed_real_nak_fixture_with_unicode_and_frozen_timestamp() {
        let raw = include_str!("../tests/fixtures/nip57-nak-request.json");
        let event = VerifiedEvent::from_json(raw).unwrap();
        let payee = event.singleton_tag("p", true).unwrap().unwrap();
        let signer = test_event(3, 1, 100, "", vec![]).pubkey;
        let request = ZapRequest::validate(raw, payee, &signer, 10000).unwrap();
        assert_eq!(
            request.event().event().id,
            "a749d6a0132f69e5e4d8318706909066ef3e4e45be4a5cafa1a85c05747f4c75"
        );
        assert_eq!(request.event().event().created_at, 1700000000);
        assert_eq!(
            request.event().event().content,
            "Goats ⚡\n\"quoted\" \\ path\t"
        );
        assert_eq!(request.raw_json(), raw);
    }

    fn fixture(extra: Vec<Vec<String>>) -> (String, String, String) {
        let payee = test_event(2, 1, 100, "", vec![]).pubkey;
        let signer = test_event(3, 1, 100, "", vec![]).pubkey;
        let mut tags = vec![
            vec!["p".into(), payee.clone()],
            vec!["relays".into(), "wss://relay.damus.io/".into()],
            vec!["amount".into(), "10000".into()],
        ];
        tags.extend(extra);
        (
            serde_json::to_string(&test_event(1, 9734, 100, "Thank you ⚡", tags)).unwrap(),
            payee,
            signer,
        )
    }

    #[test]
    fn exact_raw_json_not_reserialized_metadata_is_the_invoice_description() {
        let (raw, payee, signer) = fixture(vec![]);
        let pretty =
            serde_json::to_string_pretty(&serde_json::from_str::<serde_json::Value>(&raw).unwrap())
                .unwrap();
        let a = ZapRequest::validate(&raw, &payee, &signer, 10000).unwrap();
        let b = ZapRequest::validate(&pretty, &payee, &signer, 10000).unwrap();
        assert_eq!(a.event.event(), b.event.event());
        assert_ne!(a.description_hash(), b.description_hash());
        assert_eq!(b.raw_json(), pretty);
        let tags = b.receipt_tags("issued_invoice").unwrap();
        assert!(tags.contains(&vec!["description".into(), pretty]));
        assert!(tags.contains(&vec!["P".into(), a.event.event().pubkey.clone()]));
    }

    #[test]
    fn duplicates_conflicting_payees_amounts_signers_coordinates_and_private_relays_fail() {
        let (raw, payee, signer) = fixture(vec![]);
        assert!(ZapRequest::validate(&raw, &signer, &signer, 10000).is_err());
        assert!(ZapRequest::validate(&raw, &payee, &signer, 11000).is_err());
        for extra in [
            vec![vec!["p".into(), payee.clone()]],
            vec![vec!["p".into()]],
            vec![vec!["amount".into(), "10000".into()]],
            vec![vec!["P".into(), payee.clone()]],
            vec![vec!["a".into(), "not-a-coordinate".into()]],
            vec![vec!["e".into(), "not-an-id".into()]],
            vec![vec!["relays".into(), "wss://10.8.0.6/".into()]],
        ] {
            let (raw, payee, signer) = fixture(extra);
            assert!(ZapRequest::validate(&raw, &payee, &signer, 10000).is_err());
        }
        let raw = serde_json::to_string(&test_event(1, 1, 100, "", vec![])).unwrap();
        assert!(ZapRequest::validate(&raw, &payee, &signer, 10000).is_err());
    }

    #[test]
    fn target_tags_are_preserved_and_plain_profile_zaps_are_allowed() {
        let (raw, payee, signer) = fixture(vec![]);
        assert!(ZapRequest::validate(&raw, &payee, &signer, 10000).is_ok());
        let coordinate = format!("30311:{payee}:goats");
        let id = "ab".repeat(32);
        let (raw, payee, signer) = fixture(vec![
            vec!["a".into(), coordinate.clone()],
            vec!["e".into(), id.clone()],
            vec!["k".into(), "30311".into()],
            vec!["P".into(), signer],
        ]);
        let request = ZapRequest::validate(&raw, &payee, &signer, 10000).unwrap();
        let tags = request.receipt_tags("invoice").unwrap();
        assert!(tags.contains(&vec!["a".into(), coordinate]));
        assert!(tags.contains(&vec!["e".into(), id]));
    }
}
