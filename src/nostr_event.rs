//! Shared NIP-01 verifier; retain existing receive/CyberHerd regression fixtures.
pub use lightning_goats_nostr_event::*;

#[cfg(test)]
use bitcoin::secp256k1::{Message, Secp256k1};
#[cfg(test)]
use sha2::{Digest, Sha256};

#[cfg(test)]
pub(crate) fn test_event(
    secret: u8,
    kind: u64,
    created_at: u64,
    content: &str,
    tags: Vec<Vec<String>>,
) -> SignedNostrEvent {
    use bitcoin::secp256k1::{Keypair, SecretKey};
    let secp = Secp256k1::new();
    let key = Keypair::from_secret_key(&secp, &SecretKey::from_slice(&[secret; 32]).unwrap());
    let mut event = SignedNostrEvent {
        id: String::new(),
        pubkey: key.x_only_public_key().0.to_string(),
        created_at,
        kind,
        tags,
        content: content.into(),
        sig: String::new(),
    };
    let hash: [u8; 32] = Sha256::digest(canonical_bytes(&event).unwrap()).into();
    event.id = hex::encode(hash);
    event.sig = secp
        .sign_schnorr_no_aux_rand(&Message::from_digest(hash), &key)
        .to_string();
    event
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verifies_canonical_unicode_control_escaping_and_signed_tags() {
        let event = test_event(
            1,
            1,
            100,
            "Goats ⚡\n\"quotes\" \\ tabs\t",
            vec![vec!["t".into(), "CyberHerd".into()]],
        );
        let json = serde_json::to_string_pretty(&event).unwrap();
        let verified = VerifiedEvent::from_json(&json).unwrap();
        assert_eq!(verified.event(), &event);
        assert_eq!(
            verified.singleton_tag("t", true).unwrap(),
            Some("CyberHerd")
        );
        assert_eq!(
            verified.tag_values("t").collect::<Vec<_>>(),
            vec!["CyberHerd"]
        );
        assert!(verified.require_not_future(0).is_ok());
        assert!(verified.require_not_future(-1).is_err());
    }

    #[test]
    fn mutated_fields_recomputed_forged_ids_and_uppercase_fail_authentication() {
        let event = test_event(1, 7, 100, "+", vec![]);
        let mut edited = event.clone();
        edited.content = "-".into();
        assert!(VerifiedEvent::verify(edited.clone()).is_err());
        edited.id = hex::encode(Sha256::digest(canonical_bytes(&edited).unwrap()));
        assert!(VerifiedEvent::verify(edited).is_err());
        let mut edited = event.clone();
        edited.pubkey = test_event(2, 1, 100, "", vec![]).pubkey;
        edited.id = hex::encode(Sha256::digest(canonical_bytes(&edited).unwrap()));
        assert!(VerifiedEvent::verify(edited).is_err());
        let mut edited = event;
        edited.id = edited.id.to_ascii_uppercase();
        assert!(VerifiedEvent::verify(edited).is_err());
    }

    #[test]
    fn bounded_tags_timestamps_duplicate_fields_and_singletons_fail_closed() {
        let json = " ".repeat(MAX_EVENT_BYTES + 1);
        assert!(VerifiedEvent::from_json(&json).is_err());
        assert!(VerifiedEvent::verify(test_event(1, 65_536, 100, "", vec![])).is_err());
        assert!(VerifiedEvent::verify(test_event(1, 1, u64::MAX, "", vec![])).is_err());
        assert!(VerifiedEvent::verify(test_event(1, 1, 100, "", vec![vec![]])).is_err());
        let event = test_event(
            1,
            1,
            100,
            "",
            vec![vec!["p".into(), "one".into()], vec!["p".into()]],
        );
        let verified = VerifiedEvent::verify(event).unwrap();
        assert!(verified.singleton_tag("p", true).is_err());
        assert!(verified.singleton_tag("missing", true).is_err());
        let event = test_event(1, 1, 1000, "", vec![]);
        assert!(
            VerifiedEvent::verify(event.clone())
                .unwrap()
                .require_not_future(0)
                .is_err()
        );
        let json = serde_json::to_string(&event)
            .unwrap()
            .replacen('{', "{\"kind\":1,", 1);
        assert!(VerifiedEvent::from_json(&json).is_err());
    }

    #[test]
    fn coordinates_require_real_keys_canonical_kind_and_replaceable_semantics() {
        let key = test_event(1, 1, 100, "", vec![]).pubkey;
        let value = format!("30311:{key}:goats:stream");
        let coordinate: Coordinate = value.parse().unwrap();
        assert_eq!(coordinate.identifier, "goats:stream");
        assert_eq!(coordinate.to_string(), value);
        for value in [
            format!("1:{key}:note"),
            format!("030311:{key}:goats"),
            format!("10000:{key}:nonempty"),
            format!("30311:{key}"),
            format!("30311:{}:stream", "00".repeat(32)),
        ] {
            assert!(value.parse::<Coordinate>().is_err(), "{value}");
        }
    }
}
