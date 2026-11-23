//! Receiver-locked ecash and its NIP-17 transport have independent keys.
use anyhow::{Context, Result, ensure};
use bitcoin::bech32::{Bech32, primitives::decode::CheckedHrpstring};
use cashu::nuts::{
    CurrencyUnit,
    nut10::Kind,
    nut18::{PaymentRequest, TransportType},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::str::FromStr;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrivateDestination {
    pub mint: String,
    pub request_digest: String,
    pub payment_id: Option<String>,
    pub receiving_key: String,
    pub transport_key: String,
    pub relays: Vec<String>,
}

impl PrivateDestination {
    pub fn from_request(text: &str, mint: &str) -> Result<Self> {
        ensure!(
            text.len() <= 8192 && text.is_ascii(),
            "invalid request size or encoding"
        );
        let req = PaymentRequest::from_str(text)?;
        ensure!(
            req.unit == Some(CurrencyUnit::Sat)
                && req.amount.is_none()
                && req.single_use != Some(true)
                && req.supported_methods.is_empty(),
            "expected reusable amountless sat request"
        );
        ensure!(
            req.mints.is_empty()
                || req
                    .mints
                    .iter()
                    .any(|m| m.to_string().trim_end_matches('/') == mint.trim_end_matches('/')),
            "request does not accept project mint"
        );
        let lock = req.nut10.context("missing wallet lock")?;
        ensure!(
            lock.kind == Kind::P2PK && lock.tags.is_none() && lock.data.len() == 66,
            "expected recipient-only compressed P2PK key"
        );
        let receiving_key = bitcoin::secp256k1::PublicKey::from_str(&lock.data)?.to_string();
        let mut transports = req.transports.iter().filter(|t| {
            t._type == TransportType::Nostr && t.tags.iter().any(|g| g.as_slice() == ["n", "17"])
        });
        let transport = transports.next().context("missing NIP-17 transport")?;
        ensure!(transports.next().is_none(), "ambiguous NIP-17 transport");
        let profile = CheckedHrpstring::new::<Bech32>(&transport.target)?;
        ensure!(profile.hrp().as_str() == "nprofile", "expected nprofile");
        profile.validate_segwit_padding()?;
        let bytes: Vec<_> = profile.byte_iter().collect();
        let mut rest = bytes.as_slice();
        let mut recipient = None;
        let mut relays = Vec::new();
        while !rest.is_empty() {
            let (&[tag, len], tail) = rest
                .split_first_chunk::<2>()
                .context("short nprofile header")?;
            let (value, tail) = tail
                .split_at_checked(usize::from(len))
                .context("short nprofile field")?;
            match tag {
                0 => {
                    ensure!(recipient.is_none(), "ambiguous nprofile identity");
                    let key = hex::encode(value);
                    crate::nostr_event::validate_public_key(&key)?;
                    recipient = Some(key);
                }
                1 => {
                    let url = crate::public_network::validate_destination(
                        std::str::from_utf8(value)?,
                        "wss",
                    )?
                    .to_string();
                    if !relays.contains(&url) {
                        relays.push(url);
                    }
                    ensure!(relays.len() <= 8, "too many relay hints");
                }
                _ => {}
            }
            rest = tail;
        }
        ensure!(!relays.is_empty(), "missing wallet relay");
        Ok(Self {
            mint: mint.trim_end_matches('/').to_owned(),
            request_digest: hex::encode(Sha256::digest(text)),
            payment_id: req.payment_id,
            receiving_key,
            transport_key: recipient.context("missing transport identity")?,
            relays,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};
    const MINT: &str = "https://mint.example.com";
    fn profile(key: &str, relay: &str) -> String {
        let mut data = vec![0, 32];
        data.extend(hex::decode(key).unwrap());
        data.extend([1, relay.len() as u8]);
        data.extend(relay.bytes());
        bitcoin::bech32::encode::<Bech32>(bitcoin::bech32::Hrp::parse("nprofile").unwrap(), &data)
            .unwrap()
    }

    #[test]
    fn reusable_requests_bind_distinct_keys_in_both_encodings() {
        let wallet = format!(
            "03{}",
            crate::nostr_event::test_event(1, 0, 1, "", vec![]).pubkey
        );
        let identity = crate::nostr_event::test_event(2, 0, 1, "", vec![]).pubkey;
        let request = json!({"u":"sat","m":[MINT],"i":"reward","nut10":{"k":"P2PK","d":wallet},
            "t":[{"t":"nostr","a":profile(&identity,"wss://relay.example.com"),"g":[["n","17"]]}]});
        let parse = |v: Value| serde_json::from_value::<PaymentRequest>(v).unwrap();
        let req = parse(request.clone());
        let b = req.to_bech32_string().unwrap();
        for text in [req.to_string(), b.to_lowercase(), b] {
            let parsed = PrivateDestination::from_request(&text, MINT).unwrap();
            assert_eq!(parsed.receiving_key, wallet);
            assert_eq!(parsed.transport_key, identity);
            assert_eq!(parsed.relays, ["wss://relay.example.com/"]);
            assert_eq!(parsed.payment_id.as_deref(), Some("reward"));
        }
        for (field, value) in [
            ("u", json!("usd")),
            ("s", json!(true)),
            ("a", json!(1)),
            ("m", json!(["https://other.example.com"])),
            ("nut10", Value::Null),
            (
                "nut10",
                json!({"k":"P2PK","d":wallet,"t":[["locktime","30"]]}),
            ),
            (
                "t",
                json!([{"t":"nostr","a":profile(&identity,"wss://127.0.0.1"),"g":[["n","17"]]}]),
            ),
        ] {
            let mut bad = request.clone();
            bad[field] = value;
            assert!(PrivateDestination::from_request(&parse(bad).to_string(), MINT).is_err());
        }
    }
}
