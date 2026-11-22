//! Deliver only core-acknowledged retained receipts to the systemd-owned collector listener.
use crate::service::Runtime;
use anyhow::{Context, Result, ensure};
use cdk::cdk_database::WalletDatabase;
use serde_json::{Value, json};
use std::path::Path;
use std::time::Duration;
use uuid::Uuid;

async fn deliver(runtime: &Runtime, id: Uuid) -> Result<()> {
    let (receipt, derivation) = {
        let mut journal = runtime
            .journal
            .lock()
            .map_err(|_| anyhow::anyhow!("wallet journal unavailable"))?;
        (journal.export_receipt(id)?, journal.derivation(id)?)
    };
    ensure!(
        receipt.mint_id == "goats"
            && derivation.mint_url.to_string().trim_end_matches('/')
                == "https://mint.lightning-goats.com",
        "invalid retained project mint"
    );
    let database = runtime
        .mint_databases
        .get(&receipt.mint_id)
        .context("missing project wallet")?;
    let ys = derivation.output_ys(&runtime.keys.seed)?;
    let database: &(dyn WalletDatabase<cdk::cdk_database::Error> + Send + Sync) = database.as_ref();
    let owned = database.get_proofs_by_ys(ys.clone()).await?;
    ensure!(owned.len() == ys.len(), "missing original project proofs");
    let mut expected: std::collections::BTreeSet<_> = ys.into_iter().collect();
    let mut sats = 0u64;
    let mut proofs = Vec::new();
    for info in owned {
        ensure!(
            info.mint_url == derivation.mint_url && expected.remove(&info.proof.y()?),
            "unexpected acquisition proof"
        );
        sats = sats
            .checked_add(u64::from(info.proof.amount))
            .context("handoff value overflow")?;
        proofs.push(info.proof);
    }
    ensure!(
        sats == receipt.net_sats && expected.is_empty(),
        "handoff value changed"
    );
    let evidence = cdk::util::hex::encode(&receipt.evidence_digest);
    ensure!(
        rustix::process::geteuid().as_raw() != 0,
        "wallet must run under a dedicated uid"
    );
    let reply: Value = crate::private_ipc::request(
        Path::new("/run/lightning-goats-project-acquisitions.sock"),
        0,
        &json!({"acquisition_id":id,"evidence_digest":evidence,"proofs":proofs}),
        1048576,
        45,
    )
    .await?;
    ensure!(
        reply == json!({"acquisition_id":id,"evidence_digest":evidence}),
        "handoff acknowledgment differs"
    );
    runtime
        .journal
        .lock()
        .map_err(|_| anyhow::anyhow!("wallet journal unavailable"))?
        .finish_collector_handoff(id)
}

pub(crate) async fn run(runtime: &Runtime) -> Result<()> {
    if !runtime
        .config
        .receipt_delivery
        .as_ref()
        .is_some_and(|c| c.enabled && c.collector_handoff)
    {
        return std::future::pending().await;
    }
    loop {
        let id = runtime
            .journal
            .lock()
            .map_err(|_| anyhow::anyhow!("wallet journal unavailable"))?
            .claim_collector_handoff()?;
        if let Some(id) = id {
            if deliver(runtime, id).await.is_err() {
                eprintln!("project Cashu handoff deferred");
            }
        } else {
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    }
}
