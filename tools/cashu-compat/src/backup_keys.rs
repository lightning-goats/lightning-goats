//! Verify copied recovery keys without opening an application or external signer.
use crate::backup_copy::{Frozen, MARKER, protected_directory};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    io::{Read, Seek, SeekFrom},
    path::Path,
};
use zeroize::Zeroizing;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Bindings {
    version: u8,
    owner: String,
    receiving_key: String,
    wallet_key_binding: String,
    cashu_client: String,
    payout_client: String,
    historical: Vec<Historical>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Historical {
    name: String,
    public_key: String,
}
#[derive(Serialize)]
pub struct Receipt {
    pub version: u8,
    pub wallet_key_binding_verified: bool,
    pub signer_clients_verified: u8,
    pub historical_keys_verified: usize,
    pub application_runtime_started: bool,
    pub external_authentication_verified: bool,
    pub restore_authorized: bool,
}

fn read(directory: &Path, name: &str, maximum: u64) -> Result<Zeroizing<Vec<u8>>> {
    let path = directory.join(name);
    let mut frozen = Frozen::open(&path, maximum, false)?;
    frozen.file.seek(SeekFrom::Start(0))?;
    let mut bytes = Zeroizing::new(Vec::new());
    (&mut frozen.file)
        .take(maximum + 1)
        .read_to_end(&mut bytes)?;
    ensure!(bytes.len() as u64 <= maximum, "copied key exceeds bound");
    frozen.unchanged(&path, maximum, false)?;
    Ok(bytes)
}

fn public(bytes: &[u8]) -> Result<cdk::secp256k1::PublicKey> {
    let mut secret = cdk::secp256k1::SecretKey::from_slice(bytes)
        .map_err(|_| anyhow::anyhow!("invalid copied secret scalar"))?;
    let public =
        cdk::secp256k1::PublicKey::from_secret_key(&cdk::secp256k1::Secp256k1::new(), &secret);
    secret.non_secure_erase();
    Ok(public)
}

/// Caller has copied original material into a protected stage. Public bindings
/// come from the protected configuration and normalized wallet journal.
pub fn verify(directory: &Path) -> Result<Receipt> {
    ensure!(
        rustix::process::geteuid().as_raw() == 0,
        "root collector required"
    );
    protected_directory(directory)?;
    ensure!(
        read(directory, "INCOMPLETE", 128)?.as_slice() == MARKER,
        "marked private copy stage required"
    );
    let manifest = read(directory, "key-bindings.json", 16384)?;
    let bindings: Bindings = serde_json::from_slice(&manifest)
        .map_err(|_| anyhow::anyhow!("invalid copied key bindings"))?;
    ensure!(
        bindings.version == 1 && bindings.historical.len() <= 64,
        "invalid recovery key scope"
    );
    lightning_goats_nostr_event::validate_public_key(&bindings.owner)?;
    let seed = read(directory, "wallet-seed.bin", 64)?;
    ensure!(seed.len() == 64, "copied seed length differs");
    let receiver = public(&read(directory, "receiving-key.bin", 32)?)?;
    ensure!(
        receiver.to_string() == bindings.receiving_key,
        "copied receiver differs from configuration"
    );
    let mut digest = Sha256::new();
    digest.update(b"lightning-goats-cashu-runtime-keys-v1\0");
    digest.update(seed.as_slice());
    digest.update(receiver.serialize());
    ensure!(
        hex::encode(digest.finalize()) == bindings.wallet_key_binding,
        "copied seed differs from immutable wallet binding"
    );
    let mut seen = std::collections::BTreeSet::from([
        bindings.owner,
        receiver.x_only_public_key().0.to_string(),
    ]);
    ensure!(seen.len() == 2, "receiver must differ from project owner");
    for (name, expected) in [
        ("cashu-client.hex", bindings.cashu_client),
        ("payout-client.hex", bindings.payout_client),
    ] {
        let raw = read(directory, name, 65)?;
        let raw = raw.strip_suffix(b"\n").unwrap_or(&raw);
        ensure!(raw.len() == 64, "copied client credential length differs");
        let secret = Zeroizing::new(
            hex::decode(raw).map_err(|_| anyhow::anyhow!("invalid copied client encoding"))?,
        );
        let actual = public(&secret)?.x_only_public_key().0.to_string();
        ensure!(
            actual == expected && seen.insert(actual),
            "copied signer credential identity mismatch or reuse"
        );
    }
    let mut names = std::collections::BTreeSet::new();
    for historical in &bindings.historical {
        ensure!(
            (historical.name.starts_with("historical-")
                || historical.name == format!("receiving-key-{}.bin", historical.public_key))
                && historical.name.ends_with(".bin")
                && historical.name.len() <= 96
                && historical
                    .name
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"-._".contains(&b))
                && names.insert(&historical.name),
            "invalid historical key name"
        );
        let actual = public(&read(directory, &historical.name, 32)?)?;
        ensure!(
            actual.to_string() == historical.public_key
                && seen.insert(actual.x_only_public_key().0.to_string()),
            "copied historical receiving identity differs"
        );
    }
    Ok(Receipt {
        version: 1,
        wallet_key_binding_verified: true,
        signer_clients_verified: 2,
        historical_keys_verified: bindings.historical.len(),
        application_runtime_started: false,
        external_authentication_verified: false,
        restore_authorized: false,
    })
}
