//! Join a real relay admission to CDK acquisition and the core receipt socket.
use super::*;
use crate::{
    publication::{Intent, tests::sign},
    receipt_delivery::Client,
    relay_intake::tests::{config, event},
    service::Runtime,
    service_config::Config,
};
use serde::Deserialize;
use std::{
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};
use tokio::{io::AsyncWriteExt, net::UnixStream};

fn fixture_root() -> Result<PathBuf> {
    let host_namespace = std::env::var_os("LG_CASHU_WSS_HOST_NETNS")
        .ok_or_else(|| anyhow::anyhow!("fixture host namespace is required"))?;
    let root = PathBuf::from(std::env::var("LG_CASHU_WSS_FIXTURE")?);
    ensure!(
        std::fs::read_link("/proc/self/ns/net")?.as_os_str() != host_namespace
            && std::fs::read_to_string("/proc/net/dev")?
                .lines()
                .skip(2)
                .all(|line| line.trim_start().starts_with("lo:"))
            && root.parent() == Some(Path::new("/run"))
            && root
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("lg-wss-intake-"))
            && root.canonicalize()? == root,
        "fixture requires an isolated network and disposable paths"
    );
    Ok(root)
}

#[test]
#[ignore = "requires the isolated native wallet WSS fixture"]
fn isolated_wss_sender() -> Result<()> {
    let config = config(&fixture_root()?.join("sender"))?;
    let intent = Intent::announcement(
        &config,
        SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs(),
    )?;
    let announcement = intent.verify_signed(None, &sign(intent.unsigned(None)?)?)?;
    let lock = announcement
        .singleton_tag("pubkey", true)?
        .ok_or_else(|| anyhow::anyhow!("fixture announcement omitted a lock"))?;
    let mint = FixtureMint::new(100);
    let proof = mint.issue(
        json!(["P2PK",{"nonce":"0123456789abcdef0123456789abcdef","data":lock,"tags":[]}])
            .to_string(),
    );
    let first = event(&config, std::slice::from_ref(&proof), "first delivery")?;
    let replay = event(
        &config,
        std::slice::from_ref(&proof),
        "same proof in a new event",
    )?;
    let after_credit = event(&config, &[proof], "same proof after credit")?;
    println!(
        "CASHU_WSS_FIXTURE {}",
        json!({
            "recipient": config.recipient,
            "receiving_key": config.receiving_key,
            "mint": &config.mints[0].advertised_url,
            "first": first.event(),
            "replay": replay.event(),
            "after_credit": after_credit.event(),
            "keyset": {"id": mint.keyset.id, "keys": mint.keyset.keys}
        })
    );
    Ok(())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Endpoint {
    socket: PathBuf,
    core_uid: u32,
    core_epoch: Uuid,
    wallet_instance: Uuid,
    wallet_epoch: Uuid,
}

#[tokio::test]
#[ignore = "requires the isolated native wallet WSS fixture"]
async fn isolated_wss_receipt_wallet() -> Result<()> {
    let root = fixture_root()?;
    let config = Config::load(&root.join("wallet.json"))?;
    let endpoint: Endpoint = serde_json::from_slice(&std::fs::read(root.join("client.json"))?)?;
    let id = Uuid::parse_str(&std::env::var("LG_CASHU_WSS_ACQUISITION")?)?;
    let phase = std::env::var("LG_CASHU_WSS_PHASE")?;
    ensure!(
        matches!(phase.as_str(), "acquire" | "replay")
            && config.state_directory == root.join("state")
            && endpoint.socket == root.join("core-run/receipts.sock")
            && endpoint.core_uid == config.core_uid
            && endpoint.wallet_instance == config.wallet_instance
            && endpoint.wallet_epoch == config.wallet_epoch,
        "wallet fixture scope differs from native relay registration"
    );
    let url = config.mints[0].advertised_url.parse()?;
    let runtime = Runtime::open(config).await?;
    let mint = FixtureMint::new(100);
    let mut coordinator = fixture_coordinator(
        &runtime.keys.seed,
        url,
        mint.clone(),
        runtime.mint_databases["fixture"].clone(),
        runtime.journal.clone(),
        runtime.writer.clone(),
        Arc::new(runtime.policy.keys.clone()),
    )?;
    coordinator.set_permissions(Permissions {
        acquisition: phase == "acquire",
        receipt_export: true,
        ..Permissions::default()
    });
    let before = if phase == "acquire" {
        "validated"
    } else {
        "credit_export_pending"
    };
    ensure!(
        runtime.journal.lock().unwrap().state(id)? == before,
        "fixture is missing its retained admission"
    );
    let acquired = receipt(coordinator.acquire_new(id, &runtime.keys.receiving).await?);
    ensure!(
        acquired.acquisition_id == id
            && acquired.transport == "nutzap"
            && acquired.gross_sats == 8
            && acquired.receive_fee_sats == 1
            && acquired.net_sats == 7,
        "CDK acquisition differs from the original NutZap accounting"
    );
    if phase == "acquire" {
        ensure!(
            mint.swap_attempts.load(Ordering::SeqCst) == 1
                && mint.executions.load(Ordering::SeqCst) == 1,
            "synthetic mint did not execute exactly one swap"
        );
        let wire = crate::receipt_wire::from_private(endpoint.wallet_instance, &acquired)?;
        let bytes = wire.encode()?;
        let mut stream = UnixStream::connect(&endpoint.socket).await?;
        ensure!(
            stream.peer_cred()?.uid() == endpoint.core_uid,
            "wrong fixture core peer"
        );
        stream.write_u32(u32::try_from(bytes.len())?).await?;
        stream.write_all(&bytes).await?;
        stream.flush().await?;
        drop(stream); // The driver verifies the durable core commit before restarting this wallet.
        println!("CASHU_WSS_CDK_SWAP_SENT_WITHOUT_READING_ACK");
    } else {
        let client = Client::new(
            endpoint.socket,
            endpoint.core_uid,
            endpoint.core_epoch,
            Duration::from_secs(3),
        )?;
        client
            .deliver(&runtime.journal, endpoint.wallet_instance, id)
            .await?;
        let mut journal = runtime.journal.lock().unwrap();
        ensure!(
            mint.swap_attempts.load(Ordering::SeqCst) == 0
                && journal.state(id)? == "credited"
                && journal.export_receipt(id)? == *acquired,
            "wallet replay changed the original acquisition or receipt"
        );
        println!("CASHU_WSS_RECEIPT_REPLAY_WITHOUT_SECOND_SWAP");
    }
    Ok(())
}
