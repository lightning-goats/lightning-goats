//! Reusable receive instructions only. A descriptor never contains spendable proofs.
use crate::mint_policy::{MintPolicy, MintRegistry};
use anyhow::{Context, Result, ensure};
use base64::{Engine, engine::general_purpose::URL_SAFE};
use cdk::{
    mint_url::MintUrl,
    nuts::{
        CurrencyUnit, PublicKey,
        nut18::{PaymentRequest, SupportedMethod, Transport, TransportType},
    },
};
use sha2::{Digest, Sha256};
use std::str::FromStr;
use url::Url;

pub use lightning_goats_cashu_receipt::public_request::Encoding;

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RetiredDescriptor {
    pub id: String,
    pub endpoint: String,
    #[serde(default)]
    pub receiving_key: Option<String>,
    pub retired_at: u64,
    pub accept_before: u64,
    pub mints: Vec<RetiredMint>,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RetiredMint {
    pub id: String,
    pub advertised_url: String,
    pub policy_version: String,
}

impl RetiredDescriptor {
    pub(crate) fn receiver(&self, current: PublicKey) -> Result<PublicKey> {
        Ok(self
            .receiving_key
            .as_deref()
            .map(PublicKey::from_hex)
            .transpose()?
            .unwrap_or(current))
    }

    pub(crate) fn verify(&self, receiver: PublicKey) -> Result<()> {
        ensure!(
            self.retired_at > 0
                && self.retired_at < self.accept_before
                && self.accept_before <= i64::MAX as u64
                && (1..=16).contains(&self.mints.len()),
            "invalid retired descriptor window or mint inventory"
        );
        let historical = MintRegistry::new(
            self.mints
                .iter()
                .map(|mint| MintPolicy {
                    id: mint.id.clone(),
                    advertised_url: mint.advertised_url.clone(),
                    policy_version: mint.policy_version.clone(),
                    receive_cashu: true,
                    receive_nutzap: false,
                    bolt11_exit_qualified: true,
                })
                .collect(),
        )?;
        let original = Descriptor::prepare(&historical, &self.endpoint, receiver, Encoding::CreqA)?;
        ensure!(
            self.id == original.id(),
            "retired descriptor policy differs"
        );
        Ok(())
    }

    pub(crate) fn accepts(&self, id: &str, mint: &MintPolicy, now: u64) -> bool {
        self.id == id
            && (self.retired_at..self.accept_before).contains(&now)
            && self.mints.iter().any(|original| {
                original.id == mint.id && original.advertised_url == mint.advertised_url
            })
    }
}

/// Immutable snapshot suitable for public display after operator-approved activation.
pub struct Descriptor {
    id: String,
    encoded: String,
    encoding: Encoding,
}
impl Descriptor {
    pub fn public_metadata(
        &self,
    ) -> Result<lightning_goats_cashu_receipt::public_request::Descriptor> {
        let descriptor = lightning_goats_cashu_receipt::public_request::Descriptor {
            version: 1,
            id: self.id.clone(),
            encoding: self.encoding,
            request: self.encoded.clone(),
        };
        descriptor.validate()?;
        Ok(descriptor)
    }
    pub fn id(&self) -> &str {
        &self.id
    }
    pub fn encoded(&self) -> &str {
        &self.encoded
    }

    /// No network/announcement/activation occurs. The caller selects an encoding
    /// only after qualifying actual sender clients and scans for its mint set.
    pub fn prepare(
        registry: &MintRegistry,
        endpoint: &str,
        receiving_key: PublicKey,
        encoding: Encoding,
    ) -> Result<Self> {
        ensure!(endpoint.len() <= 256, "receive endpoint too long");
        let endpoint = Url::parse(endpoint)?;
        ensure!(
            endpoint.scheme() == "https"
                && endpoint.host().is_some()
                && endpoint.username().is_empty()
                && endpoint.password().is_none()
                && endpoint.query().is_none()
                && endpoint.fragment().is_none(),
            "invalid private delivery endpoint"
        );
        let mints = registry.cashu_mints().collect::<Vec<_>>();
        ensure!(
            !mints.is_empty() && mints.len() <= 16,
            "no bounded qualified Cashu mint set"
        );
        let mut policy = Sha256::new();
        policy.update(b"lightning-goats-cashu-descriptor-v1\0");
        policy.update(receiving_key.to_bytes());
        fn field(hash: &mut Sha256, value: &str) {
            hash.update((value.len() as u64).to_be_bytes());
            hash.update(value.as_bytes());
        }
        field(&mut policy, endpoint.as_str());
        for mint in &mints {
            field(&mut policy, &mint.id);
            field(&mut policy, &mint.advertised_url);
            field(&mut policy, &mint.policy_version);
        }
        let digest = policy.finalize();
        let id = format!("lg1-{}", cdk::util::hex::encode(&digest[..16]));
        let request = PaymentRequest::builder()
            .payment_id(id.clone())
            .unit(CurrencyUnit::Sat)
            .single_use(false)
            .mints(
                mints
                    .iter()
                    .map(|mint| MintUrl::from_str(&mint.advertised_url))
                    .collect::<Result<Vec<_>, _>>()?,
            )
            .mint_preferred(false)
            .add_supported_method(SupportedMethod::new("bolt11"))
            .description("Lightning Goats — shared herd contribution")
            .add_transport(Transport {
                _type: TransportType::HttpPost,
                target: endpoint.to_string(),
                tags: vec![],
            })
            .build();
        let encoded = match encoding {
            Encoding::CreqA => encode_amountless_creqa(&request)?,
            Encoding::CreqB => request.to_bech32_string()?,
        };
        Ok(Self {
            id,
            encoded,
            encoding,
        })
    }
}

// CDK 0.18.3 serializes None as a present CBOR null. Independent senders
// require an omitted amount for an amountless request. Keep CDK's typed model
// and omit only this null field; no payment or cryptographic behavior changes.
fn encode_amountless_creqa(request: &PaymentRequest) -> Result<String> {
    ensure!(
        request.amount.is_none(),
        "reusable request must not fix an amount"
    );
    let mut raw = ciborium::value::Value::serialized(request)?;
    let map = raw
        .as_map_mut()
        .context("request must encode as a CBOR map")?;
    let position = map
        .iter()
        .position(|(key, _)| key.as_text() == Some("a"))
        .context("typed request amount field missing")?;
    ensure!(
        matches!(map[position].1, ciborium::value::Value::Null),
        "request amount must be absent"
    );
    map.remove(position);
    let mut bytes = Vec::new();
    ciborium::into_writer(&raw, &mut bytes)?;
    Ok(format!("creqA{}", URL_SAFE.encode(bytes)))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn policy(id: &str) -> MintPolicy {
        MintPolicy {
            id: id.into(),
            advertised_url: format!("https://{id}.example"),
            policy_version: "v1".into(),
            receive_cashu: true,
            receive_nutzap: true,
            bolt11_exit_qualified: true,
        }
    }
    fn key() -> PublicKey {
        PublicKey::from_hex("0279be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798")
            .unwrap()
    }
    #[test]
    fn amountless_creqa_omits_null_amount_and_preserves_exact_typed_request() -> Result<()> {
        let original = PaymentRequest::builder()
            .payment_id("reusable")
            .unit(CurrencyUnit::Sat)
            .single_use(false)
            .mint_preferred(false)
            .mints(vec![MintUrl::from_str("https://mint.example")?])
            .description("Lightning Goats — shared herd contribution")
            .add_transport(Transport {
                _type: TransportType::HttpPost,
                target: "https://lightning-goats.example/c".into(),
                tags: vec![],
            })
            .build();
        let encoded = encode_amountless_creqa(&original)?;
        let bytes = URL_SAFE.decode(encoded.strip_prefix("creqA").unwrap())?;
        let raw: ciborium::value::Value = ciborium::from_reader(bytes.as_slice())?;
        assert!(
            !raw.as_map()
                .unwrap()
                .iter()
                .any(|(key, _)| key.as_text() == Some("a"))
        );
        assert_eq!(PaymentRequest::from_str(&encoded)?, original);
        let mut fixed = original;
        fixed.amount = Some(1u64.into());
        assert!(encode_amountless_creqa(&fixed).is_err());
        Ok(())
    }

    #[test]
    fn both_encodings_are_reusable_amountless_and_strict_without_token_material() -> Result<()> {
        let registry = MintRegistry::new(vec![policy("one"), policy("two")])?;
        for encoding in [Encoding::CreqA, Encoding::CreqB] {
            let descriptor = Descriptor::prepare(
                &registry,
                "https://lightning-goats.example/c",
                key(),
                encoding,
            )?;
            let request = PaymentRequest::from_str(descriptor.encoded())?;
            assert_eq!(request.payment_id.as_deref(), Some(descriptor.id()));
            assert_eq!(request.single_use, Some(false));
            assert_eq!(request.amount, None);
            assert_eq!(request.unit, Some(CurrencyUnit::Sat));
            assert_eq!(request.mint_preferred, Some(false));
            assert_eq!(request.mints.len(), 2);
            assert_eq!(request.transports[0]._type, TransportType::HttpPost);
            assert!(!descriptor.encoded().starts_with("cashu"));
        }
        Ok(())
    }
    #[test]
    fn descriptor_versions_follow_policy_not_donations_and_preserve_case() -> Result<()> {
        let registry = MintRegistry::new(vec![policy("one")])?;
        let a = Descriptor::prepare(
            &registry,
            "https://lightning-goats.example/c",
            key(),
            Encoding::CreqA,
        )?;
        let repeat = Descriptor::prepare(
            &registry,
            "https://lightning-goats.example/c",
            key(),
            Encoding::CreqA,
        )?;
        assert_eq!(a.encoded(), repeat.encoded());
        assert!(a.encoded().starts_with("creqA"));
        assert!(PaymentRequest::from_str(&a.encoded().to_uppercase()).is_err());
        let changed = MintRegistry::new(vec![policy("one"), policy("two")])?;
        let b = Descriptor::prepare(
            &changed,
            "https://lightning-goats.example/c",
            key(),
            Encoding::CreqA,
        )?;
        assert_ne!(a.id(), b.id());
        let c = Descriptor::prepare(
            &registry,
            "https://lightning-goats.example/d",
            key(),
            Encoding::CreqA,
        )?;
        assert_ne!(a.id(), c.id());
        Ok(())
    }
}
