//! Native receipt signing only. Never open a ledger, request/pay an invoice or
//! publish the synthetic receipt. Run under the existing non-admin core user.
#![forbid(unsafe_code)]

use anyhow::{Result, anyhow, bail};
use bitcoin::secp256k1::{Keypair, Secp256k1, SecretKey};
use lightning_goats::{config::NostrConfig, nostr::NakClient, secrets::read_systemd_credential};
use sha2::{Digest, Sha256};
use std::{fs, os::unix::fs::MetadataExt, path::PathBuf};

#[path = "../tests/support/zap_receipt.rs"]
mod fixture;

const AUTHOR: &str = "669ebbcccf409ee0467a33660ae88fd17e5379e646e41d7c236ff4963f3c36b6";
const CLIENT: &str = "d7a195216dec31cbdd3a98ab5d99b2865a7e74c7ad6d83144570f98b681c8256";
const NAK_DIGEST: &str = "b44b36c792fbc3fb73b7ba3bbc94beda2219826271aa8d5f130f569c3817c3b9";

async fn check() -> Result<()> {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    if !matches!(arguments.as_slice(), [mode] if mode == "--synthetic-receipt-signing-only") {
        bail!("explicit synthetic-receipt-signing-only mode required");
    }
    let status = fs::read_to_string("/proc/self/status")?;
    let uids = status
        .lines()
        .find_map(|line| line.strip_prefix("Uid:"))
        .ok_or_else(|| anyhow!("process identity unavailable"))?
        .split_whitespace()
        .map(str::parse::<u32>)
        .collect::<std::result::Result<Vec<_>, _>>()?;
    if uids.len() != 4 || uids[0] == 0 || uids.iter().any(|uid| *uid != uids[0]) {
        bail!("one non-admin process identity required");
    }
    let runtime = PathBuf::from(
        std::env::var_os("RUNTIME_DIRECTORY")
            .ok_or_else(|| anyhow!("private systemd runtime directory required"))?,
    );
    let metadata = fs::symlink_metadata(&runtime)?;
    if !metadata.is_dir()
        || metadata.uid() != uids[0]
        || metadata.mode() & 0o077 != 0
        || runtime.canonicalize()? != runtime
    {
        bail!("private canonical runtime directory required");
    }
    let credentials = PathBuf::from(
        std::env::var_os("CREDENTIALS_DIRECTORY")
            .ok_or_else(|| anyhow!("isolated systemd client credential required"))?,
    );
    let entries = fs::read_dir(&credentials)?.collect::<std::result::Result<Vec<_>, _>>()?;
    if entries.len() != 1 || entries[0].file_name() != "nostr-client-key" {
        bail!("only the existing core NIP-46 client credential is permitted");
    }
    let key_metadata = fs::symlink_metadata(credentials.join("nostr-client-key"))?;
    if !key_metadata.is_file() || key_metadata.len() > 128 {
        bail!("bounded regular client credential required");
    }
    let nak = PathBuf::from("/usr/local/bin/nak");
    let metadata = fs::symlink_metadata(&nak)?;
    if !metadata.is_file()
        || metadata.uid() != 0
        || metadata.mode() & 0o022 != 0
        || hex::encode(Sha256::digest(fs::read(&nak)?)) != NAK_DIGEST
    {
        bail!("protected pinned nak required");
    }
    let client_key = read_systemd_credential("nostr-client-key").await?;
    if client_key.len() != 64
        || !client_key
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        bail!("invalid authentication credential encoding");
    }
    let raw = hex::decode(&client_key).map_err(|_| anyhow!("invalid client credential"))?;
    let secret = SecretKey::from_slice(&raw).map_err(|_| anyhow!("invalid client scalar"))?;
    if Keypair::from_secret_key(&Secp256k1::new(), &secret)
        .x_only_public_key()
        .0
        .to_string()
        != CLIENT
    {
        bail!("credential is not the approved existing core client");
    }
    let config = NostrConfig {
        nak_path: nak,
        nak_config_path: runtime.join("nak"),
        bunker_pubkey: AUTHOR.into(),
        bunker_relays: Some(vec!["wss://relay.primal.net".into()]),
        relays: vec!["wss://relay.primal.net".into()],
    };
    let client = NakClient::new(&config, client_key)?;
    let job = fixture::synthetic_job(AUTHOR)?;
    let signed = client.sign_zap_receipt(&job).await?;
    let event = signed.event();
    if event.pubkey != AUTHOR
        || event.kind != 9735
        || event.created_at != job.paid_at
        || !event.content.is_empty()
        || event.tags != job.request.receipt_tags(&job.invoice)?
    {
        bail!("native signer changed the frozen synthetic receipt");
    }
    client.verify_event(event).await?;
    println!(
        "{}",
        serde_json::json!({"accepted":true,"kind_verified":9735,
            "native_sign_zap_receipt":true,"exact_author_bytes_hash_signature":true,
            "synthetic_invoice_only":true,"ledger_opened":false,
            "invoice_requests":0,"provider_operations":0,"project_events_published":0})
    );
    Ok(())
}

#[tokio::main]
async fn main() {
    if check().await.is_err() {
        // Native/helper/filesystem errors can contain private details. Emit no
        // child output, credential, signed event, invoice or underlying chain.
        eprintln!("Native synthetic receipt signing did not qualify; private output withheld.");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn synthetic_job_binds_the_exact_public_request_without_live_store_identity() {
        let job = fixture::synthetic_job(AUTHOR).unwrap();
        let invoice: lightning_invoice::Bolt11Invoice = job.invoice.parse().unwrap();
        invoice.check_signature().unwrap();
        assert_eq!(invoice.amount_milli_satoshis(), Some(10_000));
        assert_eq!(invoice.currency(), lightning_invoice::Currency::Bitcoin);
        assert_eq!(job.request.receipt_signer(), AUTHOR);
        assert_eq!(job.settlement_id, -1);
        assert_eq!(job.source_event_seq, 0);
        assert!(job.signed_json.is_none());
        assert_eq!(
            job.request.raw_json(),
            include_str!("../tests/fixtures/nip57-nak-request.json")
        );
        assert_ne!(job.request.payee(), AUTHOR);
    }
}
