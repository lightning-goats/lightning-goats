//! One bounded proof-validation boundary for private NUT-18 and signed NIP-61.
//! Successful validation is admission only; it never establishes acquired funds.

use crate::{
    descriptor::RetiredDescriptor,
    journal::{Transport, ValidatedIntake},
    mint_policy::{MintPolicy, MintRegistry},
};
use anyhow::{Result, ensure};
use cdk::nuts::{CurrencyUnit, Proof, PublicKey, nut10, nut18::PaymentRequestPayload};
use lightning_goats_nostr_event::{VerifiedEvent, validate_hex, validate_public_key};
use std::collections::{BTreeMap, BTreeSet};

pub struct Limits {
    pub body_bytes: usize,
    pub proof_count: usize,
    pub receipt_sats: u64,
}
impl Limits {
    fn validate(&self) -> Result<()> {
        ensure!(
            self.body_bytes > 0
                && self.body_bytes <= 1_048_576
                && self.proof_count > 0
                && self.proof_count <= 1024
                && self.receipt_sats > 0
                && self.receipt_sats <= i64::MAX as u64,
            "invalid intake limits"
        );
        Ok(())
    }
}

/// Frozen, operator-qualified keyset evidence. Never populated from token URLs.
#[derive(Default, Clone)]
pub struct PinnedKeys(BTreeMap<(String, String, u64), PublicKey>);
impl PinnedKeys {
    pub(crate) fn key(&self, mint: &str, keyset: cdk::nuts::Id, amount: u64) -> Result<PublicKey> {
        self.0
            .get(&(mint.to_owned(), keyset.to_string(), amount))
            .copied()
            .ok_or_else(|| anyhow::anyhow!("keyset not qualified"))
    }

    pub fn insert(
        &mut self,
        mint_id: &str,
        keyset: cdk::nuts::Id,
        amount: u64,
        key: PublicKey,
    ) -> Result<()> {
        ensure!(!mint_id.is_empty() && amount > 0, "invalid keyset evidence");
        let identity = (mint_id.to_owned(), keyset.to_string(), amount);
        if let Some(previous) = self.0.get(&identity) {
            ensure!(previous == &key, "keyset evidence changed");
        } else {
            self.0.insert(identity, key);
        }
        Ok(())
    }
}

/// Owned validated inputs, kept private. No Debug implementation or public serialization.
pub struct ValidatedContribution {
    mint_id: String,
    policy_version: String,
    transport: Transport,
    source_ref: String,
    proofs: Vec<Proof>,
    author: Option<String>,
}
impl ValidatedContribution {
    pub fn intake(&self) -> ValidatedIntake<'_> {
        ValidatedIntake {
            mint_id: &self.mint_id,
            policy_version: &self.policy_version,
            transport: self.transport,
            source_ref: &self.source_ref,
            source_author: self.author.as_deref(),
            proofs: &self.proofs,
        }
    }
    /// An authenticated nutzap author is not proof of the economic origin of funds.
    pub fn author(&self) -> Option<&str> {
        self.author.as_deref()
    }
}

fn validate_proofs(
    proofs: &[Proof],
    policy: &MintPolicy,
    keys: &PinnedKeys,
    limits: &Limits,
    receiver: PublicKey,
    nutzap: bool,
) -> Result<()> {
    ensure!(
        !proofs.is_empty() && proofs.len() <= limits.proof_count,
        "invalid proof count"
    );
    let mut total = 0_u64;
    let mut identities = BTreeSet::new();
    for proof in proofs {
        let amount = u64::from(proof.amount);
        ensure!(amount > 0, "invalid proof amount");
        total = total
            .checked_add(amount)
            .ok_or_else(|| anyhow::anyhow!("receipt overflow"))?;
        ensure!(total <= limits.receipt_sats, "receipt limit exceeded");
        ensure!(identities.insert(proof.y()?.to_bytes()), "duplicate proof");
        let key = keys
            .0
            .get(&(policy.id.clone(), proof.keyset_id.to_string(), amount))
            .ok_or_else(|| anyhow::anyhow!("keyset not qualified"))?;
        if nutzap || proof.dleq.is_some() {
            proof
                .verify_dleq(*key)
                .map_err(|_| anyhow::anyhow!("invalid or missing DLEQ"))?;
        }
        ensure!(
            proof.p2pk_e.is_none(),
            "unsupported blinded locking profile"
        );
        let secret = proof.secret.as_bytes();
        // A condition-shaped secret must parse as a supported condition; an
        // unknown/malformed NUT-10 array must never degrade into bearer cash.
        let conditional = secret.iter().find(|b| !b.is_ascii_whitespace()) == Some(&b'[');
        if conditional {
            ensure!(
                proof.dleq.is_some(),
                "receiver P2PK requires DLEQ before admission"
            );
            let condition: nut10::Secret = serde_json::from_slice(secret)
                .map_err(|_| anyhow::anyhow!("unsupported spending condition"))?;
            ensure!(
                condition.kind() == nut10::Kind::P2PK,
                "unsupported spending condition"
            );
            let data = condition.secret_data();
            ensure!(
                data.tags().is_none_or(|tags| tags.is_empty()),
                "only simple receiver P2PK is supported"
            );
            let public = PublicKey::from_hex(data.data())
                .map_err(|_| anyhow::anyhow!("invalid receiver lock"))?;
            ensure!(
                public.x_only_public_key() == receiver.x_only_public_key(),
                "wrong receiver lock"
            );
            if nutzap {
                ensure!(public.to_bytes()[0] == 2, "invalid NIP-61 key encoding");
            }
            // CDK's condition parser ignores unknown JSON fields; reject those
            // before claiming proof identities under this deliberately narrow profile.
            let value: serde_json::Value = serde_json::from_slice(secret)?;
            let fields = value
                .get(1)
                .and_then(|v| v.as_object())
                .ok_or_else(|| anyhow::anyhow!("invalid locking profile"))?;
            ensure!(
                fields
                    .keys()
                    .all(|key| matches!(key.as_str(), "nonce" | "data" | "tags")),
                "unsupported locking field"
            );
        } else {
            ensure!(!nutzap, "nutzap requires receiver P2PK");
        }
    }
    Ok(())
}

pub fn ordinary_json(
    body: &[u8],
    source_ref: &str,
    descriptors: &[String],
    registry: &MintRegistry,
    keys: &PinnedKeys,
    limits: &Limits,
    receiver: PublicKey,
) -> Result<ValidatedContribution> {
    ordinary_json_at(
        body,
        source_ref,
        DescriptorAcceptance {
            active: descriptors,
            retired: &[],
            now: 0,
        },
        registry,
        keys,
        limits,
        receiver,
    )
}

pub(crate) struct DescriptorAcceptance<'a> {
    pub active: &'a [String],
    pub retired: &'a [RetiredDescriptor],
    pub now: u64,
}

pub(crate) fn ordinary_json_at(
    body: &[u8],
    source_ref: &str,
    descriptors: DescriptorAcceptance<'_>,
    registry: &MintRegistry,
    keys: &PinnedKeys,
    limits: &Limits,
    receiver: PublicKey,
) -> Result<ValidatedContribution> {
    limits.validate()?;
    ensure!(
        body.len() <= limits.body_bytes,
        "Cashu payload exceeds limit"
    );
    ensure!(
        !source_ref.is_empty() && source_ref.len() <= 256,
        "invalid admission ID"
    );
    let payload: PaymentRequestPayload =
        serde_json::from_slice(body).map_err(|_| anyhow::anyhow!("invalid Cashu payload"))?;
    ensure!(payload.unit == CurrencyUnit::Sat, "unsupported unit");
    let policy = registry.accepting(&payload.mint.to_string(), false)?;
    if let Some(id) = payload.id {
        ensure!(
            descriptors.active.contains(&id)
                || descriptors
                    .retired
                    .iter()
                    .any(|old| old.accepts(&id, policy, descriptors.now)),
            "receiving descriptor not accepted"
        );
    }
    validate_proofs(&payload.proofs, policy, keys, limits, receiver, false)?;
    Ok(ValidatedContribution {
        mint_id: policy.id.clone(),
        policy_version: policy.policy_version.clone(),
        transport: Transport::Cashu,
        source_ref: source_ref.into(),
        proofs: payload.proofs,
        author: None,
    })
}

pub fn nutzap_json(
    body: &str,
    recipient: &str,
    registry: &MintRegistry,
    keys: &PinnedKeys,
    limits: &Limits,
    receiver: PublicKey,
) -> Result<ValidatedContribution> {
    limits.validate()?;
    ensure!(
        body.len() <= limits.body_bytes,
        "nutzap payload exceeds limit"
    );
    validate_public_key(recipient)?;
    let verified =
        VerifiedEvent::from_json(body).map_err(|_| anyhow::anyhow!("invalid signed nutzap"))?;
    ensure!(
        verified.event().kind == 9321,
        "unexpected nutzap event kind"
    );
    ensure!(
        verified.singleton_tag("p", true)? == Some(recipient),
        "wrong nutzap recipient"
    );
    ensure!(
        verified.singleton_tag("unit", false)?.unwrap_or("sat") == "sat",
        "unsupported unit"
    );
    let url = verified
        .singleton_tag("u", true)?
        .ok_or_else(|| anyhow::anyhow!("missing mint"))?;
    let policy = registry.accepting(url, true)?;
    ensure!(
        url == policy.advertised_url,
        "nutzap mint URL must match announcement exactly"
    );
    if let Some(event) = verified.singleton_tag("e", false)? {
        validate_hex(event, 32)?;
    }
    let mut proofs = Vec::new();
    for tag in &verified.event().tags {
        if tag.first().is_some_and(|key| key == "proof") {
            ensure!(
                tag.len() == 2 && proofs.len() < limits.proof_count,
                "invalid nutzap proofs"
            );
            proofs.push(
                serde_json::from_str::<Proof>(&tag[1])
                    .map_err(|_| anyhow::anyhow!("invalid nutzap proof"))?,
            );
        }
    }
    validate_proofs(&proofs, policy, keys, limits, receiver, true)?;
    Ok(ValidatedContribution {
        mint_id: policy.id.clone(),
        policy_version: policy.policy_version.clone(),
        transport: Transport::Nutzap,
        source_ref: verified.event().id.clone(),
        proofs,
        author: Some(verified.event().pubkey.clone()),
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::journal::{Admission, Journal};
    use cdk::{
        Amount, dhke,
        nuts::{BlindSignature, Id, ProofDleq, SecretKey},
    };
    use lightning_goats_nostr_event::{SignedNostrEvent, canonical_bytes};
    use sha2::{Digest, Sha256};
    use std::str::FromStr;

    fn receiver() -> PublicKey {
        SecretKey::from_slice(&[1; 32]).unwrap().public_key()
    }
    pub(crate) fn proof(secret: String) -> Proof {
        let issuer = SecretKey::from_slice(&[7; 32]).unwrap();
        let (blinded, r) = dhke::blind_message(
            secret.as_bytes(),
            Some(SecretKey::from_slice(&[8; 32]).unwrap()),
        )
        .unwrap();
        let signature = dhke::sign_message(&issuer, &blinded).unwrap();
        let keyset = Id::from_str("00deadbeef123456").unwrap();
        let signed =
            BlindSignature::new(Amount::from(8), signature, keyset, &blinded, &issuer).unwrap();
        let c = dhke::unblind_message(&signature, &r, &issuer.public_key()).unwrap();
        let mut proof = Proof::new(Amount::from(8), keyset, cdk::secret::Secret::new(secret), c);
        let dleq = signed.dleq.unwrap();
        proof.dleq = Some(ProofDleq::new(dleq.e, dleq.s, r));
        proof
    }
    pub(crate) fn locked() -> Proof {
        let key = receiver()
            .x_only_public_key()
            .public_key(cdk::secp256k1::Parity::Even);
        let key = PublicKey::from_slice(&key.serialize()).unwrap();
        proof(serde_json::json!(["P2PK",{"nonce":"fixture","data":key.to_hex()}]).to_string())
    }
    fn policy() -> (MintRegistry, PinnedKeys, Limits) {
        let registry = MintRegistry::new(vec![MintPolicy {
            id: "fixture".into(),
            advertised_url: "https://mint.example".into(),
            policy_version: "v1".into(),
            receive_cashu: true,
            receive_nutzap: true,
            bolt11_exit_qualified: true,
        }])
        .unwrap();
        let mut keys = PinnedKeys::default();
        keys.insert(
            "fixture",
            Id::from_str("00deadbeef123456").unwrap(),
            8,
            SecretKey::from_slice(&[7; 32]).unwrap().public_key(),
        )
        .unwrap();
        (
            registry,
            keys,
            Limits {
                body_bytes: 65536,
                proof_count: 8,
                receipt_sats: 100,
            },
        )
    }
    fn private_body(proofs: &[Proof]) -> Vec<u8> {
        serde_json::to_vec(&serde_json::json!({"id":"reusable","mint":"https://mint.example","unit":"sat","proofs":proofs})).unwrap()
    }
    fn event(proofs: &[Proof], extra: Vec<Vec<String>>) -> SignedNostrEvent {
        use cdk::secp256k1::{Keypair, Message, Secp256k1};
        let secp = Secp256k1::new();
        let key = Keypair::from_secret_key(
            &secp,
            &cdk::secp256k1::SecretKey::from_slice(&[2; 32]).unwrap(),
        );
        let mut tags = vec![
            vec!["p".into(), receiver().x_only_public_key().to_string()],
            vec!["u".into(), "https://mint.example".into()],
            vec!["unit".into(), "sat".into()],
        ];
        tags.extend(
            proofs
                .iter()
                .map(|proof| vec!["proof".into(), serde_json::to_string(proof).unwrap()]),
        );
        tags.extend(extra);
        let mut event = SignedNostrEvent {
            id: String::new(),
            pubkey: key.x_only_public_key().0.to_string(),
            created_at: 1,
            kind: 9321,
            tags,
            content: "fixture".into(),
            sig: String::new(),
        };
        let hash: [u8; 32] = Sha256::digest(canonical_bytes(&event).unwrap()).into();
        event.id = cdk::util::hex::encode(hash);
        event.sig = secp
            .sign_schnorr_no_aux_rand(&Message::from_digest(hash), &key)
            .to_string();
        event
    }
    fn nutzap(
        body: &str,
        registry: &MintRegistry,
        keys: &PinnedKeys,
        limits: &Limits,
    ) -> Result<ValidatedContribution> {
        nutzap_json(
            body,
            &receiver().x_only_public_key().to_string(),
            registry,
            keys,
            limits,
            receiver(),
        )
    }

    #[test]
    fn plain_and_nutzap_share_one_acquisition_but_reusable_descriptor_is_not_a_donation_id()
    -> Result<()> {
        let (registry, keys, limits) = policy();
        let p = locked();
        let plain = ordinary_json(
            &private_body(std::slice::from_ref(&p)),
            "http-1",
            &["reusable".into()],
            &registry,
            &keys,
            &limits,
            receiver(),
        )?;
        assert!(plain.author().is_none());
        let signed = event(&[p], vec![]);
        let zap = nutzap(&serde_json::to_string(&signed)?, &registry, &keys, &limits)?;
        assert_eq!(zap.author(), Some(signed.pubkey.as_str()));
        let dir = crate::test_directory()?;
        let mut journal =
            Journal::open(&dir.path().join("journal.sqlite"), uuid::Uuid::new_v4(), 8)?;
        let first = journal.admit(&plain.intake())?;
        let id = match first {
            Admission::New(id) => id,
            _ => panic!("new acquisition expected"),
        };
        assert_eq!(journal.admit(&zap.intake())?, Admission::Duplicate(id));
        let other = ordinary_json(
            &private_body(&[proof("different-bearer".into())]),
            "http-2",
            &["reusable".into()],
            &registry,
            &keys,
            &limits,
            receiver(),
        )?;
        assert!(matches!(journal.admit(&other.intake())?, Admission::New(_)));
        Ok(())
    }
    #[test]
    fn invalid_crypto_and_unsupported_conditions_never_claim_an_identity() -> Result<()> {
        let (registry, keys, limits) = policy();
        let valid = locked();
        let mut altered = valid.clone();
        altered.c = SecretKey::from_slice(&[9; 32])?.public_key();
        assert!(
            nutzap(
                &serde_json::to_string(&event(&[altered], vec![]))?,
                &registry,
                &keys,
                &limits
            )
            .is_err()
        );
        for tags in [
            serde_json::json!([["locktime", "1"]]),
            serde_json::json!([["pubkeys", receiver().to_hex()]]),
            serde_json::json!([["sigflag", "SIG_ALL"]]),
            serde_json::json!([["refund", receiver().to_hex()]]),
        ] {
            let p=proof(serde_json::json!(["P2PK",{"nonce":"fixture","data":receiver().to_hex(),"tags":tags}]).to_string());
            assert!(
                ordinary_json(
                    &private_body(&[p]),
                    "bad",
                    &["reusable".into()],
                    &registry,
                    &keys,
                    &limits,
                    receiver()
                )
                .is_err()
            );
        }
        let mut missing = valid;
        missing.dleq = None;
        assert!(
            nutzap(
                &serde_json::to_string(&event(&[missing.clone()], vec![]))?,
                &registry,
                &keys,
                &limits
            )
            .is_err()
        );
        assert!(
            ordinary_json(
                &private_body(&[missing]),
                "bad",
                &["reusable".into()],
                &registry,
                &keys,
                &limits,
                receiver()
            )
            .is_err()
        );
        Ok(())
    }
    #[test]
    fn ordinary_bearer_without_dleq_is_only_validated_not_acquired() -> Result<()> {
        let (registry, keys, limits) = policy();
        let mut p = proof("private-bearer".into());
        p.dleq = None;
        let validated = ordinary_json(
            &private_body(&[p]),
            "pending",
            &["reusable".into()],
            &registry,
            &keys,
            &limits,
            receiver(),
        )?;
        let dir = crate::test_directory()?;
        let mut journal =
            Journal::open(&dir.path().join("journal.sqlite"), uuid::Uuid::new_v4(), 8)?;
        let id = match journal.admit(&validated.intake())? {
            Admission::New(id) => id,
            _ => panic!("new expected"),
        };
        assert!(journal.export_receipt(id).is_err());
        Ok(())
    }
    #[test]
    fn forged_events_duplicate_recipient_and_unknown_mints_fail_closed() -> Result<()> {
        let (registry, keys, limits) = policy();
        let valid = event(&[locked()], vec![]);
        let mut forged = valid.clone();
        forged.content = "forged".into();
        assert!(nutzap(&serde_json::to_string(&forged)?, &registry, &keys, &limits).is_err());
        let duplicate = event(
            &[locked()],
            vec![vec!["p".into(), receiver().x_only_public_key().to_string()]],
        );
        assert!(
            nutzap(
                &serde_json::to_string(&duplicate)?,
                &registry,
                &keys,
                &limits
            )
            .is_err()
        );
        let mut body: serde_json::Value =
            serde_json::from_slice(&private_body(&[proof("bearer".into())]))?;
        body["mint"] = "https://unknown.example".into();
        assert!(
            ordinary_json(
                &serde_json::to_vec(&body)?,
                "unknown",
                &["reusable".into()],
                &registry,
                &keys,
                &limits,
                receiver()
            )
            .is_err()
        );
        Ok(())
    }
}
