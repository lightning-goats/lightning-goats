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
    let lots = ledger
        .operator_funding_page(&rewards, epoch, feed, 0)
        .await?
        .allocations;
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
    let fee = inputs["fee"].as_i64().unwrap();
    let plan = serde_json::json!({"version":2,"owner":owner,"epoch":epoch,"core_uid":uid,"request_id":feed,
        "barrier_hash":"aa".repeat(32),"cycle":1,"roster_revision":1,"funding_hash":grant.funding_hash,
        "real_sats":32,"threshold_sats":32,"member_percent":50,"budget_sats":16,"reserved_sats":14,
        "operations_sats":18,"root_policy_hash":grant.policy_hash,"created_at":grant.created_at,"rows":[],
        "planning_policy":{"fee_basis_points":0,"minimum_fee_sats":fee,"minimum_net_sats":1,"maximum_net_sats":32,
            "profile_max_age_seconds":3600,"operation_addresses":[]}});
    let qualification = serde_json::json!({"execution_provider":"cashu","reservation":grant,"retained_allocations":lots});
    sqlx::query("INSERT INTO payout_grants(id,request_id,plan_hash,plan_json,qualification_json,reserved_sats,admitted_at) VALUES(?,?,?,?,?,14,0)")
        .bind(grant.id.to_string()).bind(feed.to_string()).bind(&grant.plan_hash).bind(plan.to_string())
        .bind(qualification.to_string()).execute(&db).await?;
    if let Some(members) = inputs["members"].as_array() {
        for member in members {
            let d = &member["destination"];
            sqlx::query("INSERT INTO payout_rows(grant_id,member,address,recipient_json,net_sats,fee_limit_sats,gross_sats) VALUES(?,?,?,?,?,?,7)")
                .bind(grant.id.to_string()).bind(member["pubkey"].as_str().unwrap()).bind(d["receiving_key"].as_str().unwrap())
                .bind(d.to_string()).bind(7-fee).bind(fee).execute(&db).await?;
        }
    }
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
    let worker = intake(Store::connect(&url, policy.clone()).await?)?;
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
    if let Some(members) = inputs["members"].as_array() {
        use crate::payout::{
            signer::Client as Signer,
            worker::{CashuWorker, Step},
        };
        assert_eq!(members.len(), 2);
        let mut lost = 0;
        let mut ready = 0;
        // A second member can be selected while the first member's swap
        // still holds the shared input. It must resume after change is restored.
        let core = Client::new(config.socket.clone(), uid)?
            .with_request_spacing(std::time::Duration::from_secs(1));
        for _ in 0..10 {
            if ready == 2 {
                break;
            }
            let w = CashuWorker {
                reclaim_failed: false,
                collector_socket: Some(socket.clone()),
                store: Store::connect(&url, policy.clone()).await?,
                core: core.clone(),
                signer: Signer::new(root.join("unused"), uid)?,
                plans: Plans::new(root.join("unused"), uid, owner.clone())?,
            };
            match w.tick_with_collector_uid(uid).await {
                Ok(Step::Observed) => ready += 1,
                Err(_) => lost += 1,
                result => anyhow::bail!("unexpected member worker step {result:?}"),
            }
        }
        assert_eq!(ready, 2);
        assert!(lost >= 2);
        let claims: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM member_ecash_execution_claims")
            .fetch_one(&ledger.pool)
            .await?;
        let sums: (i64, i64, i64) =
            sqlx::query_as("SELECT COUNT(*),SUM(amount_sat),SUM(fee_sat) FROM member_ecash_ready")
                .fetch_one(&db)
                .await?;
        assert_eq!(claims, 2);
        assert_eq!(sums, (2, 14 - 2 * fee, 2 * fee));
        assert_eq!(ledger.feed_credit_sats().await?, 0);
    }
    std::fs::write(root.join("feed"), feed.to_string())?;
    task.abort();
    Ok(())
}
