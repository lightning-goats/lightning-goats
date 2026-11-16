//! Real Unix credentials and production delivery adapter, synthetic acquired receipt only.
use super::*;
use crate::{
    journal::{Admission, Transport, ValidatedIntake},
    writer_lock::WriterLock,
};
use cdk::{
    nuts::{Id, Proof, SecretKey},
    secret::Secret,
};
use serde::Deserialize;

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
#[ignore = "requires isolated cross-UID fixture runner"]
async fn isolated_receipt_wallet_child() -> Result<()> {
    let root = PathBuf::from(std::env::var("LG_CASHU_IPC_FIXTURE")?);
    let state = root.join("wallet");
    crate::private_ipc::private_state(&state)?;
    ensure!(
        !state.join("acquisition.sqlite").exists(),
        "fixture requires a fresh wallet journal"
    );
    let endpoint: Endpoint = serde_json::from_slice(&std::fs::read(root.join("client.json"))?)?;
    let _writer = WriterLock::acquire(&state)?;
    let mut journal = Journal::open(&state.join("acquisition.sqlite"), endpoint.wallet_epoch, 8)?;
    let proofs = vec![Proof::new(
        8.into(),
        "00deadbeef123456".parse::<Id>()?,
        Secret::new("synthetic-ipc-proof"),
        SecretKey::from_slice(&[3; 32])?.public_key(),
    )];
    let Admission::New(id) = journal.admit(&ValidatedIntake {
        mint_id: "fixture",
        policy_version: "v1",
        transport: Transport::Cashu,
        source_ref: "synthetic-cross-uid",
        source_author: None,
        proofs: &proofs,
    })?
    else {
        anyhow::bail!("fixture was not fresh");
    };
    journal.prepare(id, b"synthetic-original-swap")?;
    journal.start_acquisition(id)?;
    journal.record_acquired(id, 7, 1, b"synthetic-verified-output-fixture")?;
    let wire =
        crate::receipt_wire::from_private(endpoint.wallet_instance, &journal.export_receipt(id)?)?;
    let journal = Arc::new(Mutex::new(journal));
    let client = Client::new(
        endpoint.socket.clone(),
        endpoint.core_uid,
        endpoint.core_epoch,
        Duration::from_secs(3),
    )?;
    // Commit can succeed despite a sender disappearing before reading its reply.
    let mut lost = UnixStream::connect(&endpoint.socket).await?;
    ensure!(
        lost.peer_cred()?.uid() == endpoint.core_uid,
        "wrong fixture core peer"
    );
    let bytes = wire.encode()?;
    lost.write_u32(u32::try_from(bytes.len())?).await?;
    lost.write_all(&bytes).await?;
    lost.flush().await?;
    drop(lost);
    client
        .deliver(&journal, endpoint.wallet_instance, id)
        .await?;
    ensure!(
        journal.lock().unwrap().state(id)? == "credited",
        "authenticated receipt was not acknowledged"
    );
    client
        .deliver(&journal, endpoint.wallet_instance, id)
        .await?;
    let mut conflict = wire.data().clone();
    conflict.net_sats = 8;
    conflict.gross_sats = 9;
    ensure!(
        client
            .send(&WireReceipt::validate(conflict)?)
            .await
            .is_err(),
        "conflicting replay accepted"
    );
    ensure!(
        journal.lock().unwrap().state(id)? == "credited",
        "conflict rewound acknowledgment"
    );
    println!("CASHU_WALLET_CROSS_UID_LOST_ACK_REPLAY_VERIFIED");
    Ok(())
}
