//! Actual authenticated listener; synthetic public policy and empty private state.
use super::*;
use crate::descriptor::{Descriptor, RetiredMint};
use lightning_goats_cashu_receipt::public_request::Encoding;
use serde::Deserialize;
use std::{collections::BTreeMap, path::PathBuf};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Bootstrap {
    core_uid: u32,
    ipc_gid: u32,
    wallet_instance: Uuid,
    wallet_epoch: Uuid,
}

#[tokio::test]
#[ignore = "requires isolated public metadata cross-UID runner"]
async fn isolated_public_request_wallet_child() -> Result<()> {
    let root = PathBuf::from(std::env::var("LG_CASHU_PUBLIC_FIXTURE")?);
    let phase = std::env::var("LG_CASHU_PUBLIC_PHASE")?;
    ensure!(
        ["initial", "retired", "paused"].contains(&phase.as_str()),
        "invalid phase"
    );
    let bootstrap: Bootstrap =
        serde_json::from_slice(&std::fs::read(root.join("bootstrap.json"))?)?;
    let state = root.join("wallet");
    let writer = WriterLock::acquire(&state)?;
    let journal = Arc::new(Mutex::new(Journal::open(
        &state.join("journal.sqlite"),
        bootstrap.wallet_epoch,
        64,
    )?));
    let mut mint = crate::mint_policy::MintPolicy {
        id: "fixture".into(),
        advertised_url: "https://mint.example".into(),
        policy_version: "fixture-v1".into(),
        receive_cashu: true,
        receive_nutzap: false,
        bolt11_exit_qualified: true,
    };
    let receiver = cdk::nuts::SecretKey::from_slice(&[3; 32])?.public_key();
    let endpoint = "https://lightning-goats.example/api/v1/cashu";
    let original = Descriptor::prepare(
        &MintRegistry::new(vec![mint.clone()])?,
        endpoint,
        receiver,
        Encoding::CreqA,
    )?;
    let mut retired_descriptors = Vec::new();
    if phase != "initial" {
        let retired = RetiredDescriptor {
            id: original.id().into(),
            endpoint: endpoint.into(),
            receiving_key: None,
            retired_at: 1,
            accept_before: 2,
            mints: vec![RetiredMint {
                id: mint.id.clone(),
                advertised_url: mint.advertised_url.clone(),
                policy_version: mint.policy_version.clone(),
            }],
        };
        retired.verify(receiver)?;
        retired_descriptors.push(retired);
        mint.policy_version = "fixture-v2".into();
    }
    let registry = MintRegistry::new(vec![mint])?;
    let descriptor = Descriptor::prepare(&registry, endpoint, receiver, Encoding::CreqA)?;
    let ids = vec![descriptor.id().to_owned()];
    let mut keys = PinnedKeys::default();
    keys.insert(
        "fixture",
        "00deadbeef123456".parse()?,
        8,
        cdk::nuts::SecretKey::from_slice(&[7; 32])?.public_key(),
    )?;
    let submissions_path = state.join("synthetic-submissions.json");
    if !submissions_path.exists() {
        let submissions: Vec<serde_json::Value> =
            ["first-synthetic-payment", "second-synthetic-payment"]
                .into_iter()
                .map(|secret| {
                    serde_json::json!({
                        "id":original.id(), "mint":"https://mint.example", "unit":"sat",
                        "proofs":[crate::intake::tests::proof(secret.to_owned())]
                    })
                })
                .collect();
        std::fs::write(&submissions_path, serde_json::to_vec(&submissions)?)?;
    }
    let submissions: Vec<serde_json::Value> =
        serde_json::from_slice(&std::fs::read(&submissions_path)?)?;
    let limits = Limits {
        body_bytes: 65536,
        proof_count: 64,
        receipt_sats: 10000,
    };
    let intake = Intake {
        core_uid: bootstrap.core_uid,
        wallet_instance: bootstrap.wallet_instance,
        wallet_epoch: bootstrap.wallet_epoch,
        receive_cashu: phase != "paused",
        receive_nutzap: false,
        registry: &registry,
        keys: &keys,
        limits: &limits,
        receiver,
        recipient: "",
        descriptors: &ids,
        retired_descriptors: &retired_descriptors,
        retained_receivers: &[],
        public_request: Some(&descriptor),
        journal: journal.clone(),
        writer: &writer,
    };
    std::fs::write(
        state.join("ready.json"),
        serde_json::to_vec(&serde_json::json!({
            "socket":root.join("run/intake.sock"),"wallet_uid":rustix::process::geteuid().as_raw(),
            "wallet_instance":bootstrap.wallet_instance,"wallet_epoch":bootstrap.wallet_epoch,
            "descriptor":descriptor.public_metadata()?, "submissions":submissions
        }))?,
    )?;
    intake
        .serve(
            &root.join("run/intake.sock"),
            bootstrap.ipc_gid,
            Duration::from_secs(2),
            64,
            async {
                while !root.join("stop").exists() {
                    tokio::time::sleep(Duration::from_millis(20)).await;
                }
            },
        )
        .await?;
    ensure!(
        journal.lock().unwrap().pending_operations(10)?.len() == 2,
        "two distinct pending admissions were not retained"
    );
    let db = rusqlite::Connection::open(state.join("journal.sqlite"))?;
    let counts: BTreeMap<String, i64> = [
        "acquisitions",
        "treasury_instructions",
        "treasury_melt_requests",
    ]
    .into_iter()
    .map(|table| {
        Ok((
            table.to_owned(),
            db.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))?,
        ))
    })
    .collect::<Result<_>>()?;
    ensure!(
        counts["acquisitions"] == 2
            && counts["treasury_instructions"] == 0
            && counts["treasury_melt_requests"] == 0,
        "metadata changed custody"
    );
    println!("PUBLIC_WALLET_TWO_PENDING_ADMISSIONS_ZERO_SPEND_{phase}");
    Ok(())
}
