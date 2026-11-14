//! Synthetic four-journal fixtures; no production files, providers or feeders.
use super::*;
use crate::cyberherd::reward_cutover::Decision;
use crate::state_maintenance::{
    contact_upgrade::tests::copy_images,
    tests::{payout_policy, policy, signer_policy},
};
use uuid::Uuid;
pub(in crate::state_maintenance) type Keep = (
    crate::ledger::LedgerStore,
    crate::cyberherd::store::Store,
    crate::payout::store::Store,
    crate::payout::signer::Store,
);

pub(in crate::state_maintenance) async fn seed(root: &Path) -> (Policy, Keep) {
    let mut p = policy(root);
    for component in p.components.values() {
        std::fs::create_dir_all(component.database.parent().unwrap()).unwrap();
    }
    let core_url = format!("sqlite://{}", p.components[&Role::Core].database.display());
    let core = crate::ledger::LedgerStore::connect(&core_url)
        .await
        .unwrap();
    p.epoch = core.overlay_stream_id().await.unwrap();
    core.initialize_reward_funding().await.unwrap();
    core.record_payment(&crate::domain::payment::SettledPayment {
        source: "test".into(),
        source_id: "retained-contact-upgrade-payment".into(),
        payment_hash: None,
        address_user: "herd".into(),
        credit_pool: "herd".into(),
        amount_msat: 2_340_000,
        settled_at: None,
        context_json: None,
    })
    .await
    .unwrap();
    let feed = core.begin_feed_attempt(1000).await.unwrap().unwrap();
    core.confirm_feed_attempt(feed).await.unwrap();
    let signed = crate::nostr_event::test_event(1, 1, 100, "Frozen signed payment notice", vec![]);
    core.enqueue_signed_message(1, &signed.id, &serde_json::to_string(&signed).unwrap())
        .await
        .unwrap();
    let membership = crate::cyberherd::store::Store::connect(&format!(
        "sqlite://{}",
        p.components[&Role::Membership].database.display()
    ))
    .await
    .unwrap();
    membership
        .initialize(
            &p.owner,
            &crate::cyberherd::membership::Settings {
                minimum_sats: 10,
                max_members: 3,
                member_percent: 10,
            },
            0,
        )
        .await
        .unwrap();
    membership
        .initialize_core_ingress(
            &p.owner,
            &crate::cyberherd::ingress::CoreBinding {
                core_uid: p.components[&Role::Core].uid,
                epoch: p.epoch,
                start_after: 0,
                credit_pool: "herd".into(),
                payees: [("herd".into(), p.owner.clone())].into(),
            },
        )
        .await
        .unwrap();
    let payout = crate::payout::store::Store::connect(
        &format!(
            "sqlite://{}",
            p.components[&Role::Payout].database.display()
        ),
        payout_policy(&p),
    )
    .await
    .unwrap();
    let signer = crate::payout::signer::Store::connect(
        &format!(
            "sqlite://{}",
            p.components[&Role::Signer].database.display()
        ),
        signer_policy(&p),
    )
    .await
    .unwrap();
    (p, (core, membership, payout, signer))
}

#[tokio::test]
async fn cutoff_comparison_accepts_only_audit_append_and_preserves_original_bytes() {
    let root = tempfile::tempdir().unwrap();
    let (p, keep) = seed(root.path()).await;
    let member_url = format!(
        "sqlite://{}",
        p.components[&Role::Membership].database.display()
    );
    let member = crate::sqlite::connect_durable(&member_url, 1)
        .await
        .unwrap();
    let original_key = Uuid::new_v4().to_string();
    let original_input = "{ \"retained\": true }";
    sqlx::query("INSERT INTO operator_commands(rowid,owner,command_id,input_json,result_json) VALUES(37,?,?,?,?)")
        .bind(&p.owner).bind(&original_key).bind(original_input).bind("{}").execute(&member).await.unwrap();
    member.close().await;
    let batch = keep
        .0
        .core_event_batch(Some(p.epoch), 0, 128)
        .await
        .unwrap();
    let before = root.path().join("before");
    let after = root.path().join("after");
    copy_images(&p, &before);
    copy_images(&p, &after);
    let core = rows::open_named(&before, "core.db").await.unwrap();
    let binding = CoreBinding {
        core_uid: p.components[&Role::Core].uid,
        epoch: p.epoch,
        start_after: 0,
        credit_pool: "herd".into(),
        payees: [("herd".into(), p.owner.clone())].into(),
    };
    let historical = derive_history(&core, &binding, batch.latest).await.unwrap();
    core.close().await;
    assert_eq!(historical.len(), 1);
    let id = Uuid::new_v4();
    let preview = Preview {
        version: 1,
        id,
        owner: p.owner.clone(),
        bundle_id: Uuid::new_v4(),
        source_policy_sha256: "ab".repeat(32),
        decision: Decision {
            id,
            epoch: p.epoch,
            through: batch.latest,
            backup_manifest_sha256: "cd".repeat(32),
            decided_at: crate::cyberherd::calendar::now().unwrap(),
            historical,
        },
        cutover_applied: false,
        activation_authorized: false,
    };
    let after_url = format!("sqlite://{}", after.join("membership.db").display());
    let member = crate::cyberherd::store::Store::connect(&after_url)
        .await
        .unwrap();
    member
        .record_initial_reward_cutover(&p.owner, &preview.decision)
        .await
        .unwrap();
    compare_images(&before, &after, &p, &preview).await.unwrap();
    let pool = crate::sqlite::connect_durable(&after_url, 1).await.unwrap();
    sqlx::query("UPDATE operator_commands SET input_json=? WHERE command_id=?")
        .bind("{\"retained\":true}")
        .bind(&original_key)
        .execute(&pool)
        .await
        .unwrap();
    assert!(compare_images(&before, &after, &p, &preview).await.is_err());
    sqlx::query("UPDATE operator_commands SET input_json=? WHERE command_id=?")
        .bind(original_input)
        .bind(&original_key)
        .execute(&pool)
        .await
        .unwrap();
    compare_images(&before, &after, &p, &preview).await.unwrap();
    sqlx::query("CREATE INDEX unexpected_cutoff_index ON operator_commands(owner)")
        .execute(&pool)
        .await
        .unwrap();
    assert!(compare_images(&before, &after, &p, &preview).await.is_err());
    sqlx::query("DROP INDEX unexpected_cutoff_index")
        .execute(&pool)
        .await
        .unwrap();
    pool.close().await;
    compare_images(&before, &after, &p, &preview).await.unwrap();
    let payout_url = format!("sqlite://{}", after.join("payout.db").display());
    let payout = crate::sqlite::connect_durable(&payout_url, 1)
        .await
        .unwrap();
    let description: String =
        sqlx::query_scalar("SELECT description FROM _sqlx_migrations WHERE version=1")
            .fetch_one(&payout)
            .await
            .unwrap();
    sqlx::query("UPDATE _sqlx_migrations SET description='unexpected' WHERE version=1")
        .execute(&payout)
        .await
        .unwrap();
    assert!(compare_images(&before, &after, &p, &preview).await.is_err());
    sqlx::query("UPDATE _sqlx_migrations SET description=? WHERE version=1")
        .bind(description)
        .execute(&payout)
        .await
        .unwrap();
    payout.close().await;
    compare_images(&before, &after, &p, &preview).await.unwrap();
    let core_url = format!("sqlite://{}", after.join("core.db").display());
    let core = crate::sqlite::connect_durable(&core_url, 1).await.unwrap();
    sqlx::query("UPDATE message_outbox SET signed_event_json=signed_event_json || ' '")
        .execute(&core)
        .await
        .unwrap();
    core.close().await;
    assert!(compare_images(&before, &after, &p, &preview).await.is_err());
}
