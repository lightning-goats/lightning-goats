//! Run by direct_operator_acceptance.py with disposable mint and collector sockets.
use super::*;
use crate::{
    core_bridge::{
        BridgeConfig,
        unix::{Client, Server, self_credentials},
    },
    cyberherd::payout_bridge::Client as Plans,
    payout::{
        funding::ProviderScope,
        inclusive_fees::Mode,
        operator::DirectWallet,
        runtime::Intake,
        store::{Policy, Scope, Store},
    },
    reward_funding::{RewardFundingSource, RewardPolicy},
};
use std::{os::unix::fs::PermissionsExt, path::PathBuf};
use uuid::Uuid;

#[tokio::test]
#[ignore = "started by the funded local mint fixture"]
async fn direct_operator_worker_fixture() -> anyhow::Result<()> {
    let root = PathBuf::from(std::env::var("LG_DIRECT_FIXTURE")?);
    let inputs: serde_json::Value =
        serde_json::from_slice(&std::fs::read(root.join("worker.json"))?)?;
    let (uid, gid) = self_credentials()?;
    let url = format!("sqlite://{}", root.join("core.db").display());
    let ledger = LedgerStore::connect(&url).await?;
    ledger.initialize_reward_funding().await?;
    let id: Uuid = inputs["claim"].as_str().unwrap().parse()?;
    ledger
        .prepare_breez_invoice(
            id,
            &serde_json::json!({"amount_sat":32,"user":"herd","credit_pool":"herd","zap":false}),
        )
        .await?;
    ledger
        .bind_breez_invoice(id, "fixture", inputs["payment_hash"].as_str().unwrap())
        .await?;
    let socket = root.join("collector.sock");
    assert!(
        ledger
            .poll_breez_claim(&BreezCollectorConfig {
                socket: socket.clone(),
                peer_uid: uid
            })
            .await?
    );
    let feed = ledger.begin_feed_attempt(32).await?.unwrap();
    ledger.confirm_feed_attempt(feed).await?;
    let epoch = ledger.overlay_stream_id().await?;
    let owner = crate::nostr_event::test_event(1, 1, 100, "", vec![]).pubkey;
    let rewards = RewardPolicy {
        owner: owner.clone(),
        funding_source: RewardFundingSource::RetainedCashu,
        member_percent_ceiling: 50,
        max_sats_per_feed: 16,
        max_sats_per_24h: 64,
    };
    let grant = ledger
        .reserve_reward(&rewards, epoch, feed, &"aa".repeat(32), 14)
        .await?;
    let bridge = root.join("bridge");
    std::fs::create_dir(&bridge)?;
    std::fs::set_permissions(&bridge, std::fs::Permissions::from_mode(0o750))?;
    let config = BridgeConfig {
        socket: bridge.join("core.sock"),
        peer_uid: uid + 1,
        group_gid: gid,
        reward_policy: Some(rewards),
        payout_peer_uid: Some(uid),
        delegated_feeder_messages: false,
        payout_execution_enabled: true,
        payout_account_id: None,
    };
    let server = Server::bind(&config).await?;
    let task = tokio::spawn(server.run(ledger.clone(), crate::config::RuntimeMode::Active));
    let policy = Policy {
        scope: Scope {
            core_uid: uid,
            epoch,
            owner: owner.clone(),
            provider: ProviderScope {
                environment: "retained-cashu".into(),
                account_id: epoch,
                credential_fingerprint: "aa".repeat(32),
            },
        },
        member_percent_ceiling: 50,
        max_sats_per_feed: 16,
        max_sats_per_24h: 64,
        max_fee_sats: 1,
        fee_mode: Mode::FixedAllowance,
        max_net_sats: 32,
        max_source_parts: 32,
        operation_addresses: vec![],
        receipt_relays: vec!["wss://relay.example.com".into()],
        execution_enabled: true,
    };
    let url = format!("sqlite://{}", root.join("payout.db").display());
    let store = Store::connect(&url, policy.clone()).await?;
    let db = sqlx::SqlitePool::connect(&url).await?;
    // Seed the journal at its admitted-plan boundary, with a real core reservation.
    sqlx::query("INSERT INTO payout_grants(id,request_id,plan_hash,plan_json,qualification_json,reserved_sats,admitted_at) VALUES(?,?,?,'{}','{}',14,0)").bind(grant.id.to_string()).bind(feed.to_string()).bind(&grant.plan_hash).execute(&db).await?;
    sqlx::query("INSERT INTO payout_intake_receipts VALUES(1,?,?,?)")
        .bind(&grant.plan_hash)
        .bind(feed.to_string())
        .bind(grant.id.to_string())
        .execute(&db)
        .await?;
    sqlx::query("INSERT INTO payout_operator_allocations VALUES(1,?,32,14,18)")
        .bind(&grant.funding_hash)
        .execute(&db)
        .await?;
    let intake = |store| -> anyhow::Result<Intake> {
        Ok(Intake {
            store,
            core: Client::new(config.socket.clone(), uid)?,
            plans: Plans::new(root.join("unused"), uid, owner.clone())?,
            verifier: None,
        })
    };
    let wallet = DirectWallet {
        mint: "https://mint.lightning-goats.com".into(),
        recipient: inputs["recipient"].as_str().unwrap().into(),
    };
    let mut disabled = policy.clone();
    disabled.execution_enabled = false;
    intake(Store::connect(&url, disabled).await?)?
        .operator_tick(&socket, None, Some(&wallet), uid)
        .await?;
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM payout_operator_methods")
            .fetch_one(&db)
            .await?,
        0
    );
    assert!(
        intake(store)?
            .operator_tick(&socket, None, Some(&wallet), uid)
            .await
            .is_err()
    );
    // The mint accepted the swap and the fixture discarded its response. Restart with legacy config.
    let worker = intake(Store::connect(&url, policy).await?)?;
    worker
        .operator_tick(&socket, Some("sat@lightning-goats.com"), None, uid)
        .await?;
    let result: (String, i64, i64) =
        sqlx::query_as("SELECT state,amount_sat,fee_sat FROM payout_operator_ecash_status")
            .fetch_one(&db)
            .await?;
    let fee = inputs["fee"].as_i64().unwrap();
    assert_eq!(result, ("ready".to_string(), 18 - fee, fee));
    assert_eq!(ledger.feed_credit_sats().await?, 0);
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM operator_p2pk_claims")
            .fetch_one(&ledger.pool)
            .await?,
        1
    );
    assert!(worker.store.next_operator_allocation().await?.is_none());
    std::fs::write(root.join("feed"), feed.to_string())?;
    task.abort();
    Ok(())
}
