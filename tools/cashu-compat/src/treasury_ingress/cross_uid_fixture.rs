//! Real runtime listener and kernel credentials, synthetic invoices and keys only.
use super::*;
use crate::{service::Runtime, service_config::Config};
use cdk::nuts::SecretKey;
use lightning_goats_cashu_receipt::treasury::Issued;
use serde::Deserialize;
use std::{os::unix::fs::PermissionsExt, path::PathBuf};
use uuid::Uuid;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Bootstrap {
    core_uid: u32,
    ipc_gid: u32,
    wallet_instance: Uuid,
    wallet_epoch: Uuid,
    core_epoch: Uuid,
}

#[tokio::test]
#[ignore = "requires isolated treasury cross-UID runner"]
async fn isolated_treasury_wallet_child() -> Result<()> {
    let root = PathBuf::from(std::env::var("LG_CASHU_TREASURY_FIXTURE")?);
    let phase = std::env::var("LG_CASHU_TREASURY_PHASE")?;
    ensure!(
        ["initial", "paused"].contains(&phase.as_str()),
        "invalid fixture phase"
    );
    let bootstrap: Bootstrap =
        serde_json::from_slice(&std::fs::read(root.join("bootstrap.json"))?)?;
    let state = root.join("wallet");
    crate::private_ipc::private_state(&state)?;
    let socket = root.join("run/treasury.sock");
    let (mut binding, mut issued) = crate::treasury::tests::fixture();
    binding.core_epoch = bootstrap.core_epoch;
    binding.wallet_instance = bootstrap.wallet_instance;
    binding.wallet_epoch = bootstrap.wallet_epoch;
    let config = Config::decode(&serde_json::to_vec(&serde_json::json!({
        "version":1,"runtime_uid":rustix::process::geteuid().as_raw(),"core_uid":bootstrap.core_uid,
        "ipc_gid":bootstrap.ipc_gid,"wallet_instance":bootstrap.wallet_instance,"wallet_epoch":bootstrap.wallet_epoch,
        "state_directory":state,"intake_socket":root.join("run/intake.sock"),
        "receiving_key":SecretKey::from_slice(&[3;32])?.public_key().to_hex(),
        "recipient":SecretKey::from_slice(&[4;32])?.public_key().x_only_public_key().to_string(),
        "descriptors":[],"mints":[{"id":binding.mint_id,"advertised_url":"https://mint.example",
            "policy_version":"receive-v1","receive_cashu":false,"receive_nutzap":false,"bolt11_exit_qualified":true}],
        "keysets":[],"body_bytes":65536,"proof_count":64,"receipt_sats":10000,
        "deadline_seconds":3,"connections_per_second":64,
        "treasury_intake":{"enabled":true,"accept_new":phase=="initial","socket":socket,
            "core_epoch":binding.core_epoch,"destination":binding.destination,
            "policies":{(binding.mint_id.clone()):binding.policy_version}}
    }))?)?;
    let runtime = if phase == "initial" {
        for (name, bytes) in [
            ("wallet-seed.bin", vec![7; 64]),
            ("receiving-key.bin", vec![3; 32]),
        ] {
            let path = state.join(name);
            ensure!(!path.exists(), "fixture keys already exist");
            std::fs::write(&path, bytes)?;
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
        }
        Runtime::initialize(config).await?
    } else {
        Runtime::open(config).await?
    };
    let invoices = state.join("invoices.json");
    let (issued, alternate): (Issued, Issued) = if phase == "initial" {
        issued.intent.core_epoch = binding.core_epoch;
        issued.intent.wallet_instance = binding.wallet_instance;
        issued.intent.wallet_epoch = binding.wallet_epoch;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs();
        issued.invoice = crate::treasury::tests::signed_invoice_hash(
            cdk::lightning_invoice::Currency::Bitcoin,
            100_000,
            now,
            300,
            "ab",
        );
        let mut alternate = issued.clone();
        alternate.intent.id = Uuid::from_u128(900);
        alternate.receive_request_id = Uuid::from_u128(901);
        alternate.payment_hash = "cd".repeat(32);
        alternate.invoice = crate::treasury::tests::signed_invoice_hash(
            cdk::lightning_invoice::Currency::Bitcoin,
            100_000,
            now,
            300,
            "cd",
        );
        std::fs::write(
            &invoices,
            serde_json::to_vec(&(issued.clone(), alternate.clone()))?,
        )?;
        (issued, alternate)
    } else {
        serde_json::from_slice(&std::fs::read(invoices)?)?
    };
    std::fs::write(
        state.join("ready.json"),
        serde_json::to_vec(&serde_json::json!({
            "socket":socket,"wallet_uid":rustix::process::geteuid().as_raw(),"issued":issued,"alternate":alternate
        }))?,
    )?;
    tokio::time::timeout(
        Duration::from_secs(45),
        runtime.serve_treasury(async {
            while !root.join("stop").exists() {
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        }),
    )
    .await??;
    let db = rusqlite::Connection::open(state.join("acquisition.sqlite"))?;
    let counts: (i64,i64,i64,i64) = db.query_row("SELECT (SELECT COUNT(*) FROM treasury_instructions),(SELECT COUNT(*) FROM treasury_source_holds),(SELECT COUNT(*) FROM treasury_quote_requests),(SELECT COUNT(*) FROM treasury_melt_requests)", [], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?)))?;
    ensure!(
        counts == (1, 0, 0, 0),
        "treasury fixture changed financial state or duplicated admission"
    );
    println!("TREASURY_WALLET_ONE_RETAINED_ZERO_SPEND_{phase}");
    Ok(())
}
