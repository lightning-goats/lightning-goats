//! Synthetic cryptography, real distinct Unix identities and durable broker/wallet journals.
use super::*;
use crate::{
    journal::{Admission, Journal, Transport, ValidatedIntake},
    publication::tests::{announcement, envelope, owner, sign},
    writer_lock::WriterLock,
};
use std::{
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::{io::AsyncWriteExt, net::UnixStream};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Scope {
    signer_uid: u32,
    wallet_uid: u32,
    ipc_gid: u32,
    wallet_instance: Uuid,
    wallet_epoch: Uuid,
    broker_instance: Uuid,
    broker_epoch: Uuid,
}
fn scope() -> Result<(PathBuf, Scope, bool)> {
    let root = PathBuf::from(
        std::env::var_os("LG_CASHU_SIGNER_FIXTURE")
            .ok_or_else(|| anyhow::anyhow!("isolated fixture required"))?,
    );
    ensure!(
        std::env::var_os("CREDENTIALS_DIRECTORY").is_none(),
        "fixture refuses service credentials"
    );
    ensure!(
        std::fs::read_link("/proc/self/ns/net")?
            != PathBuf::from(std::env::var("LG_CASHU_HOST_NETNS")?),
        "fixture requires isolated network namespace"
    );
    let interfaces = std::fs::read_to_string("/proc/net/dev")?;
    ensure!(
        interfaces
            .lines()
            .skip(2)
            .all(|line| line.trim_start().starts_with("lo:")),
        "fixture requires loopback only"
    );
    let scope = serde_json::from_slice(&std::fs::read(root.join("scope.json"))?)?;
    let restart = match std::env::var("LG_CASHU_SIGNER_PHASE")?.as_str() {
        "first" => false,
        "restart" => true,
        _ => anyhow::bail!("invalid fixture phase"),
    };
    Ok((root, scope, restart))
}
fn policy(scope: &Scope) -> Policy {
    let Intent::Announcement {
        receiving_key,
        mints,
        relays,
        ..
    } = announcement()
    else {
        unreachable!()
    };
    Policy {
        broker_instance: scope.broker_instance,
        broker_epoch: scope.broker_epoch,
        wallet_instance: scope.wallet_instance,
        wallet_epoch: scope.wallet_epoch,
        wallet_uid: scope.wallet_uid,
        owner: owner(),
        receiving_key,
        mints,
        relays,
        announcements: true,
        redemptions: true,
        max_history_sats: 100,
        max_new_per_hour: 8,
        max_attempts: 4,
    }
}
struct Synthetic {
    restart: bool,
    signs: AtomicUsize,
    plaintext: Mutex<String>,
}
#[async_trait]
impl Backend for Synthetic {
    async fn encrypt_self(&self, target: &str, plaintext: &str) -> Result<String> {
        ensure!(
            !self.restart && target == owner(),
            "unexpected fixture encryption"
        );
        *self.plaintext.lock().unwrap() = plaintext.into();
        Ok(envelope(9))
    }
    async fn decrypt_self(&self, target: &str, ciphertext: &str) -> Result<String> {
        ensure!(
            !self.restart && target == owner() && ciphertext == envelope(9),
            "unexpected fixture decryption"
        );
        Ok(self.plaintext.lock().unwrap().clone())
    }
    async fn sign(&self, unsigned: Unsigned) -> Result<String> {
        ensure!(!self.restart, "restart must use retained signature");
        self.signs.fetch_add(1, Ordering::SeqCst);
        sign(unsigned)
    }
}
#[tokio::test]
#[ignore = "requires isolated distinct-UID signing runner"]
async fn isolated_signer_child() -> Result<()> {
    let (root, scope, restart) = scope()?;
    ensure!(
        rustix::process::geteuid().as_raw() == scope.signer_uid
            && scope.signer_uid != scope.wallet_uid,
        "wrong fixture identity"
    );
    let state = root.join("signer");
    let mut broker = if restart {
        Broker::open(&state, policy(&scope))?
    } else {
        Broker::initialize(&state, policy(&scope))?
    };
    let backend = Synthetic {
        restart,
        signs: 0.into(),
        plaintext: Mutex::new(String::new()),
    };
    broker
        .serve(
            &root.join("run/sign.sock"),
            scope.ipc_gid,
            &backend,
            async {
                loop {
                    if root.join("stop").exists() {
                        break;
                    }
                    tokio::time::sleep(Duration::from_millis(25)).await;
                }
            },
        )
        .await?;
    let retained: (u64, u64, u64) = broker.connection.query_row(
        "SELECT COUNT(*),SUM(attempts),SUM(signed_event IS NOT NULL) FROM requests",
        [],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    )?;
    ensure!(
        retained == (2, 2, 2),
        "broker duplicated or lost durable work"
    );
    ensure!(
        backend.signs.load(Ordering::SeqCst) == if restart { 0 } else { 2 },
        "broker repeated cryptographic work"
    );
    println!("CASHU_SIGNER_CROSS_UID_DURABLE_REPLAY_VERIFIED");
    Ok(())
}
#[tokio::test]
#[ignore = "requires isolated distinct-UID signing runner"]
async fn isolated_wallet_child() -> Result<()> {
    let (root, scope, restart) = scope()?;
    ensure!(
        rustix::process::geteuid().as_raw() == scope.wallet_uid,
        "wrong fixture wallet identity"
    );
    let state = root.join("wallet");
    crate::private_ipc::private_state(&state)?;
    let _writer = WriterLock::acquire(&state)?;
    let mut journal = Journal::open(&state.join("acquisition.sqlite"), scope.wallet_epoch, 8)?;
    journal.runtime_registration(scope.wallet_instance, &[7; 32], !restart)?;
    let ids = state.join("operations.json");
    let (acquisition, ann): (Uuid, Uuid) = if restart {
        serde_json::from_slice(&std::fs::read(&ids)?)?
    } else {
        let proofs = [crate::intake::tests::proof(
            crate::intake::tests::locked().secret.to_string(),
        )];
        let source = "ab".repeat(32);
        let author = owner();
        let Admission::New(id) = journal.admit(&ValidatedIntake {
            mint_id: "fixture",
            policy_version: "v1",
            transport: Transport::Nutzap,
            source_ref: &source,
            source_author: Some(&author),
            proofs: &proofs,
        })?
        else {
            anyhow::bail!("fixture must be fresh")
        };
        journal.prepare(id, b"synthetic-swap")?;
        journal.start_acquisition(id)?;
        journal.record_acquired(id, 7, 1, b"synthetic-owned-outputs")?;
        journal.prepare_redemption(id, &owner(), 100)?;
        let ann = Uuid::new_v4();
        journal.prepare_announcement(ann, &announcement())?;
        std::fs::write(&ids, serde_json::to_vec(&(id, ann))?)?;
        (id, ann)
    };
    let operations = [
        format!("redemption:{acquisition}"),
        format!("announcement:{ann}"),
    ];
    let request = journal.publication_signing_request(&operations[0])?;
    let prior = operations
        .iter()
        .map(|op| journal.signed_publication(op))
        .collect::<Result<Vec<_>>>()?;
    let journal = Arc::new(Mutex::new(journal));
    let path = root.join("run/sign.sock");
    let client = ipc::Client::new(path.clone(), scope.signer_uid)?;
    if !restart {
        let mut lost = UnixStream::connect(&path).await?;
        ensure!(
            lost.peer_cred()?.uid() == scope.signer_uid,
            "unexpected signer"
        );
        let bytes = serde_json::to_vec(&request)?;
        lost.write_u32(u32::try_from(bytes.len())?).await?;
        lost.write_all(&bytes).await?;
        lost.flush().await?;
        drop(lost); // Broker retains the result even though this reply cannot be delivered.
    }
    for (index, op) in operations.iter().enumerate() {
        client.deliver(&journal, op).await?;
        let signed = journal
            .lock()
            .unwrap()
            .signed_publication(op)?
            .ok_or_else(|| anyhow::anyhow!("signature missing"))?;
        if restart {
            ensure!(
                prior[index].as_ref() == Some(&signed),
                "restart changed signed bytes"
            );
        }
        client.deliver(&journal, op).await?;
        ensure!(
            journal.lock().unwrap().signed_publication(op)?.as_ref() == Some(&signed),
            "retry changed signed bytes"
        );
    }
    let mut conflict = request;
    if let Intent::Redemption { net_sats, .. } = &mut conflict.intent {
        *net_sats += 1;
    }
    ensure!(
        client.sign(&conflict).await.is_err(),
        "conflicting operation accepted"
    );
    ensure!(
        journal.lock().unwrap().state(acquisition)? == "acquired",
        "publication changed financial state"
    );
    println!("CASHU_WALLET_SIGNER_CROSS_UID_LOST_REPLY_VERIFIED");
    Ok(())
}
