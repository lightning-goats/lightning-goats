//! A protected-journal restore fixture. Test-only capability constructors model
//! a previously qualified, different-UID core that is currently unavailable.
//! Current intake still uses actual Unix peer checks; no provider is contacted.
use super::*;
use crate::{
    cyberherd::payout_bridge::{Client as Plans, Export},
    payout::runtime::Intake,
};

#[tokio::test]
async fn admitted_plan_recovers_lost_intake_checkpoint_without_core_or_new_provider_work() {
    let f = Fixture::new().await;
    let q = qualified(&f).await;
    let mut plan = f.plan.clone();
    let uid = self_credentials().unwrap().0;
    plan.core_uid = uid + 1;
    let mut prior = copy_evidence(&q, &plan);
    prior.core_uid = uid + 1;
    let policy = journal_policy(&prior);
    let url = format!(
        "sqlite://{}",
        f.root.path().join("restored-payout.db").display()
    );
    let store = Store::connect(&url, policy).await.unwrap();
    store.admit(&plan, &prior, now()).await.unwrap();
    store.initialize_intake(uid).await.unwrap();
    let export = Export {
        sequence: 1,
        hash: plan.hash().unwrap(),
        plan,
        reservation: Some(prior.reservation().clone()),
    };
    let mut operator_plan = export.plan.clone();
    operator_plan.version = 2;
    operator_plan.funding_source = crate::reward_funding::RewardFundingSource::RetainedCashu;
    operator_plan.request_id = Uuid::new_v4();
    operator_plan.rows.clear();
    operator_plan.reserved_sats = 0;
    operator_plan.operations_sats = operator_plan.real_sats;
    let operator_export = Export {
        sequence: 2,
        hash: operator_plan.hash().unwrap(),
        plan: operator_plan.clone(),
        reservation: None,
    };
    let socket = f.root.path().join("intake.sock");
    let listener = tokio::net::UnixListener::bind(&socket).unwrap();
    let task = tokio::spawn(async move {
        loop {
            let (mut stream, _) = listener.accept().await.unwrap();
            let call: serde_json::Value =
                crate::private_ipc::read(&mut stream, 4096).await.unwrap();
            let reply = if call["after"] == 0 {
                json!({"result":"plan","export":export})
            } else if call["after"] == 1 {
                json!({"result":"plan","export":operator_export})
            } else {
                json!({"result":"waiting"})
            };
            crate::private_ipc::write(&mut stream, &reply, 4 * 1024 * 1024 + 4096)
                .await
                .unwrap();
        }
    });
    let intake = Intake {
        store: store.clone(),
        core: Client::new(f.root.path().join("offline-core.sock"), uid + 1).unwrap(),
        plans: Plans::new(socket, uid, owner()).unwrap(),
    };
    let db = sqlx::SqlitePool::connect(&url).await.unwrap();
    sqlx::query("CREATE TRIGGER fail_intake BEFORE INSERT ON payout_intake_receipts BEGIN SELECT RAISE(ABORT,'synthetic checkpoint failure'); END").execute(&db).await.unwrap();
    assert!(intake.tick().await.is_err());
    assert_eq!(store.intake_cursor().await.unwrap(), 0);
    sqlx::query("DROP TRIGGER fail_intake")
        .execute(&db)
        .await
        .unwrap();
    assert!(intake.tick().await.unwrap());
    assert_eq!(store.intake_cursor().await.unwrap(), 1);
    assert!(intake.tick().await.unwrap());
    assert!(!intake.tick().await.unwrap());
    let saved: (i64, i64, i64) = sqlx::query_as(
        "SELECT collected_sats,member_reserved_sats,operator_sats FROM payout_operator_allocations",
    )
    .fetch_one(&db)
    .await
    .unwrap();
    let reopened = Store::connect(&url, store.policy().clone()).await.unwrap();
    let allocation = reopened.operator_allocation(2).await.unwrap().unwrap();
    assert_eq!(allocation.request_id, operator_plan.request_id);
    assert_eq!(allocation.plan_hash, operator_plan.hash().unwrap());
    assert_eq!(allocation.member_grant, None);
    assert_eq!(allocation.operator_sats, operator_plan.real_sats);
    assert!(reopened.operator_allocation(1).await.unwrap().is_none());
    let real = i64::try_from(operator_plan.real_sats).unwrap();
    assert_eq!(saved, (real, 0, real));
    assert!(sqlx::query("INSERT OR REPLACE INTO payout_operator_allocations SELECT sequence,funding_hash,collected_sats,1,collected_sats-1 FROM payout_operator_allocations")
        .execute(&db).await.is_err());
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM payout_grants")
            .fetch_one(&db)
            .await
            .unwrap(),
        1
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM payout_intake_receipts")
            .fetch_one(&db)
            .await
            .unwrap(),
        2
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM payout_execute_intents")
            .fetch_one(&db)
            .await
            .unwrap(),
        0
    );
    assert_eq!(reopened.next_operator_allocation().await.unwrap(), Some(2));
    // A held transfer must yield to a later confirmed feeding, including after restart.
    sqlx::query("INSERT INTO payout_intake_receipts(sequence,plan_hash,request_id) SELECT 3,plan_hash,? FROM payout_intake_receipts WHERE sequence=2")
        .bind(uuid::Uuid::new_v4().to_string()).execute(&db).await.unwrap();
    sqlx::query("INSERT INTO payout_operator_allocations SELECT 3,funding_hash,collected_sats,member_reserved_sats,operator_sats FROM payout_operator_allocations WHERE sequence=2")
        .execute(&db).await.unwrap();
    let restarted = Store::connect(&url, store.policy().clone()).await.unwrap();
    assert_eq!(restarted.next_operator_allocation().await.unwrap(), Some(3));
    assert_eq!(restarted.next_operator_allocation().await.unwrap(), Some(2));
    // Model an invoice already bound before a crash. Settlement must verify
    // the preimage and preserve the actual fee across replay.
    let preimage = [37_u8; 32];
    let hash = hex::encode(<sha2::Sha256 as sha2::Digest>::digest(preimage));
    sqlx::query(
        "INSERT INTO payout_operator_invoices VALUES(2,'sat@lightning-goats.com','fixture',?,1,0)",
    )
    .bind(&hash)
    .execute(&db)
    .await
    .unwrap();
    let legacy = restarted
        .operator_method(2, Some(("p2pk", "new key".into())))
        .await
        .unwrap();
    assert_eq!(
        legacy,
        Some(("lightning".into(), "sat@lightning-goats.com".into()))
    );
    let original = restarted
        .operator_method(3, Some(("p2pk", "first key".into())))
        .await
        .unwrap();
    for changed in [
        Some(("p2pk", "rotated key".into())),
        Some(("lightning", "sat@lightning-goats.com".into())),
        None,
    ] {
        assert_eq!(
            restarted.operator_method(3, changed).await.unwrap(),
            original
        );
    }
    assert!(
        sqlx::query(
            "INSERT INTO payout_operator_invoices VALUES(3,'sat@lightning-goats.com','other',?,1,0)"
        )
        .bind("ee".repeat(32))
        .execute(&db)
        .await
        .is_err()
    );
    let receipt = |payment_preimage| crate::payout::cashu::CollectorResult::Paid {
        quote: "fixture".into(),
        amount_sat: 1,
        fee_sat: 0,
        payment_hash: hash.clone(),
        payment_preimage,
    };
    assert!(
        restarted
            .settle_operator_invoice(2, &receipt("00".repeat(32)))
            .await
            .is_err()
    );
    let socket = f.root.path().join("legacy-reconcile.sock");
    let listener = UnixListener::bind(&socket).unwrap();
    let original_feed = allocation.request_id;
    let settled_hash = hash.clone();
    let collector = tokio::spawn(async move {
        for _ in 0..2 {
            let (mut stream, _) = listener.accept().await.unwrap();
            let request: serde_json::Value =
                crate::private_ipc::read(&mut stream, 65536).await.unwrap();
            assert_eq!(
                request,
                json!({"reconcile_operator":original_feed,"invoice":"fixture"})
            );
            let result = json!({"state":"PAID","quote":"fixture","amount_sat":1,"fee_sat":0,"payment_hash":settled_hash,"payment_preimage":hex::encode(preimage)});
            crate::private_ipc::write(&mut stream, &result, 65536)
                .await
                .unwrap();
        }
    });
    for _ in 0..2 {
        assert!(restarted.reconcile_operator(2, &socket, uid).await.unwrap());
    }
    collector.await.unwrap();
    for _ in 0..2 {
        assert_eq!(restarted.next_operator_allocation().await.unwrap(), Some(3));
    }
    task.abort();
}
