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
        verifier: None,
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
    task.abort();
}
