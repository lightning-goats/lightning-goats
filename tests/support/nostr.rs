//! Public synthetic fixture keys. This module never reads a production secret.
use bitcoin::secp256k1::{Keypair, Message, Secp256k1, SecretKey};
use lightning_goats::nostr::SignedNostrEvent;
use sha2::{Digest, Sha256};

pub fn signed(
    secret: u8,
    kind: u64,
    created_at: u64,
    content: &str,
    tags: Vec<Vec<String>>,
) -> SignedNostrEvent {
    let secp = Secp256k1::new();
    let key = Keypair::from_secret_key(&secp, &SecretKey::from_slice(&[secret; 32]).unwrap());
    let pubkey = key.x_only_public_key().0.to_string();
    let bytes = serde_json::to_vec(&serde_json::json!([
        0, pubkey, created_at, kind, tags, content
    ]))
    .unwrap();
    let digest: [u8; 32] = Sha256::digest(bytes).into();
    SignedNostrEvent {
        id: hex::encode(digest),
        pubkey,
        created_at,
        kind,
        tags,
        content: content.into(),
        sig: secp
            .sign_schnorr_no_aux_rand(&Message::from_digest(digest), &key)
            .to_string(),
    }
}
