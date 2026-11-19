//! Invoked only by the isolated two-UID runner; never opens installed configuration.
use super::*;

#[tokio::test]
#[ignore = "requires isolated cross-UID fixture runner"]
async fn isolated_receipt_server_child() -> Result<()> {
    let root = PathBuf::from(std::env::var("LG_CASHU_IPC_FIXTURE")?);
    let state = root.join("core");
    crate::private_ipc::private_state(&state)?;
    let database = state.join("core.sqlite");
    ensure!(!database.exists(), "fixture requires a fresh core journal");
    let ledger = LedgerStore::connect(&format!("sqlite://{}", database.display())).await?;
    ledger.initialize_reward_funding().await?;
    ledger.configure_feeder_credit_cap(1000).await?;
    let mut config: Config = serde_json::from_slice(&std::fs::read(root.join("core.json"))?)?;
    config.core_epoch = ledger.overlay_stream_id().await?;
    let ready = serde_json::json!({"socket":config.socket,"core_uid":crate::private_ipc::uid()?,"core_epoch":config.core_epoch,"wallet_instance":config.wallet_instance,"wallet_epoch":config.wallet_epoch});
    let server = Server::new(config, ledger.clone())?;
    std::fs::write(state.join("ready.json"), serde_json::to_vec(&ready)?)?;
    tokio::time::timeout(
        Duration::from_secs(30),
        server.serve(async {
            while !root.join("stop").exists() {
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        }),
    )
    .await??;
    ensure!(
        ledger.feed_credit_sats().await? == 7,
        "cross-UID replay changed credit"
    );
    let events = ledger.events_after(0, 100).await?;
    ensure!(
        events
            .iter()
            .filter(|event| event.event_type == "payment_received")
            .count()
            == 1,
        "cross-UID replay duplicated payment events"
    );
    println!("CASHU_CORE_CROSS_UID_ONE_CREDIT_VERIFIED");
    Ok(())
}
