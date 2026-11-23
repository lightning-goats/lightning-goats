//! Synthetic, isolated distinct-UID transport qualification. No installed ledger/keys.
use super::*;
use crate::cashu::receipt_authority::ImportLimits;
use lightning_goats_cashu_receipt::{
    treasury::Issued,
    treasury_rpc::{Command, Phase},
};
use serde::Deserialize;
use std::collections::BTreeMap;
use uuid::Uuid;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Ready {
    socket: PathBuf,
    wallet_uid: u32,
    issued: Issued,
    alternate: Issued,
}

#[tokio::test]
#[ignore = "requires isolated treasury cross-UID runner"]
async fn isolated_treasury_core_child() -> Result<()> {
    let root = PathBuf::from(std::env::var("LG_CASHU_TREASURY_FIXTURE")?);
    let phase = std::env::var("LG_CASHU_TREASURY_PHASE")?;
    ensure!(
        ["initial", "paused", "rogue"].contains(&phase.as_str()),
        "invalid fixture phase"
    );
    let ready: Ready = serde_json::from_slice(&std::fs::read(root.join("ready.json"))?)?;
    let registration = Arc::new(ReceiptPolicy {
        wallet_uid: ready.wallet_uid,
        wallet_instance: ready.issued.intent.wallet_instance,
        wallet_epoch: ready.issued.intent.wallet_epoch,
        core_epoch: ready.issued.intent.core_epoch,
        import_enabled: false,
        limits: ImportLimits::default(),
        approved: BTreeMap::new(),
    });
    let request = Request {
        version: 1,
        request_id: Uuid::from_u128(700),
        command: Command::Retain(Box::new(ready.issued.clone())),
    };
    let client = Client::new(
        ready.socket.clone(),
        registration.clone(),
        Duration::from_secs(3),
        1,
    )?;
    if phase == "rogue" {
        ensure!(
            client.send(&request).await.is_err(),
            "rogue core peer was accepted"
        );
        println!("TREASURY_ROGUE_PEER_REJECTED");
        return Ok(());
    }
    if phase == "initial" {
        // Disconnect after complete transmission: no acknowledgment may be required
        // to retain the original instruction, and retry cannot create another one.
        let mut stream = UnixStream::connect(&ready.socket).await?;
        registration.check_peer(&KernelPeer::from_socket(&stream)?)?;
        crate::private_ipc::write(&mut stream, &request, MAX_TREASURY_BYTES).await?;
        drop(stream);
    }
    let response = client.send(&request).await?;
    ensure!(
        response.reply.phase == Phase::Bound && response.report.is_none(),
        "admission claimed settlement"
    );
    let query = Request {
        version: 1,
        request_id: Uuid::from_u128(701),
        command: Command::Status(Box::new(ready.issued.intent.clone())),
    };
    ensure!(
        client.send(&query).await?.reply.phase == Phase::Bound,
        "retained status unavailable"
    );
    let mut changed = ready.issued.clone();
    changed.invoice.push('x');
    let conflict = Request {
        version: 1,
        request_id: Uuid::from_u128(702),
        command: Command::Retain(Box::new(changed)),
    };
    ensure!(
        client.send(&conflict).await.is_err(),
        "conflicting invoice replaced original"
    );
    if phase == "paused" {
        let new = Request {
            version: 1,
            request_id: Uuid::from_u128(703),
            command: Command::Retain(Box::new(ready.alternate)),
        };
        ensure!(
            client.send(&new).await.is_err(),
            "paused admission retained a new intent"
        );
    }
    ensure!(
        client.send(&request).await?.reply.phase == Phase::Bound,
        "rejection blocked later exact replay"
    );
    println!("TREASURY_CORE_REPLAY_VERIFIED_{phase}");
    Ok(())
}
