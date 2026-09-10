//! Public synthetic signing key, used only to construct harmless local fixtures.
use bitcoin::{
    hashes::{Hash, sha256},
    secp256k1::{Secp256k1, SecretKey},
};
use lightning_invoice::{Currency, InvoiceBuilder, PaymentSecret};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub fn invoice(amount_msat: u64, description_hash: &str) -> String {
    invoice_at(
        amount_msat,
        description_hash,
        SystemTime::now().duration_since(UNIX_EPOCH).unwrap(),
    )
}

pub fn invoice_at(amount_msat: u64, description_hash: &str, created: Duration) -> String {
    let key = SecretKey::from_slice(&[42; 32]).unwrap();
    let secp = Secp256k1::new();
    InvoiceBuilder::new(Currency::Bitcoin)
        .amount_milli_satoshis(amount_msat)
        .description_hash(
            sha256::Hash::from_slice(&hex::decode(description_hash).unwrap()).unwrap(),
        )
        .payment_hash(sha256::Hash::from_slice(&[0x22; 32]).unwrap())
        .payment_secret(PaymentSecret([0x44; 32]))
        .duration_since_epoch(created)
        .expiry_time(Duration::from_secs(300))
        .min_final_cltv_expiry_delta(144)
        .payee_pub_key(bitcoin::secp256k1::PublicKey::from_secret_key(&secp, &key))
        .build_signed(|hash| secp.sign_ecdsa_recoverable(hash, &key))
        .unwrap()
        .to_string()
}
