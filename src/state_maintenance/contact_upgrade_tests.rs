use super::*;
use crate::state_maintenance::tests::{payout_policy, policy, signer_policy};

type Keep = (
    crate::ledger::LedgerStore,
    crate::cyberherd::store::Store,
    crate::payout::store::Store,
    crate::payout::signer::Store,
);

async fn seed(root: &Path) -> (Policy, Keep) {
    let mut p = policy(root);
    for component in p.components.values() {
        std::fs::create_dir_all(component.database.parent().unwrap()).unwrap();
    }
    let core_url = format!("sqlite://{}", p.components[&Role::Core].database.display());
    let core = crate::ledger::LedgerStore::connect(&core_url)
        .await
        .unwrap();
    p.epoch = core.overlay_stream_id().await.unwrap();
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
    core.begin_feed_attempt(1000).await.unwrap().unwrap();
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
                max_members: 10,
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
    // Test-only construction of the preceding schema. Migration 24 is additive;
    // remove ONLY its empty definitions/history row after native paid-state seed.
    // No runtime or maintenance command offers this fixture-only transformation.
    let pool = crate::sqlite::connect_durable(&core_url, 1).await.unwrap();
    for table in [
        "private_contact_delivery",
        "private_contact_fees",
        "private_contact_intents",
    ] {
        assert_eq!(
            sqlx::query_scalar::<_, i64>(&format!("SELECT COUNT(*) FROM {table}"))
                .fetch_one(&pool)
                .await
                .unwrap(),
            0
        );
        sqlx::query(&format!("DROP TABLE {table}"))
            .execute(&pool)
            .await
            .unwrap();
    }
    sqlx::query("DELETE FROM _sqlx_migrations WHERE version=24")
        .execute(&pool)
        .await
        .unwrap();
    pool.close().await;
    (p, (core, membership, payout, signer))
}

fn copy_images(p: &Policy, destination: &Path) {
    std::fs::create_dir(destination).unwrap();
    for role in Role::ALL {
        for suffix in ["", "-wal"] {
            let source = std::path::PathBuf::from(format!(
                "{}{suffix}",
                p.components[&role].database.display()
            ));
            if source.exists() {
                std::fs::copy(source, destination.join(format!("{}{suffix}", role.file())))
                    .unwrap();
            }
        }
    }
}

#[tokio::test]
async fn additive_contact_upgrade_retains_all_four_journals_and_refuses_row_or_ddl_changes() {
    let root = tempfile::TempDir::new().unwrap();
    let (p, _keep) = seed(root.path()).await;
    let before = root.path().join("before");
    let after = root.path().join("after");
    copy_images(&p, &before);
    copy_images(&p, &after);
    let original_bytes = std::fs::read(before.join("core.db")).unwrap();
    let original_wal = std::fs::read(before.join("core.db-wal")).ok();
    database::inspect_before_contact(&before, &p).await.unwrap();
    assert!(database::inspect(&before, &p).await.is_err());
    let pool =
        crate::sqlite::connect_durable(&format!("sqlite://{}", after.join("core.db").display()), 1)
            .await
            .unwrap();
    sqlx::migrate!("./migrations").run(&pool).await.unwrap();
    pool.close().await;
    compare_images(&before, &after, &p).await.unwrap();
    assert_eq!(
        std::fs::read(before.join("core.db")).unwrap(),
        original_bytes
    );
    assert_eq!(std::fs::read(before.join("core.db-wal")).ok(), original_wal);

    let pool =
        crate::sqlite::connect_durable(&format!("sqlite://{}", after.join("core.db").display()), 1)
            .await
            .unwrap();
    sqlx::query("UPDATE ledger_entries SET delta_sats=delta_sats+1 WHERE id=1")
        .execute(&pool)
        .await
        .unwrap();
    assert!(compare_images(&before, &after, &p).await.is_err());
    sqlx::query("UPDATE ledger_entries SET delta_sats=delta_sats-1 WHERE id=1")
        .execute(&pool)
        .await
        .unwrap();
    compare_images(&before, &after, &p).await.unwrap();
    sqlx::query("CREATE TRIGGER unqualified_contact_trigger AFTER INSERT ON private_contact_fees BEGIN SELECT 1; END").execute(&pool).await.unwrap();
    assert!(compare_images(&before, &after, &p).await.is_err());
    sqlx::query("DROP TRIGGER unqualified_contact_trigger")
        .execute(&pool)
        .await
        .unwrap();
    pool.close().await;

    let member = crate::sqlite::connect_durable(
        &format!("sqlite://{}", after.join("membership.db").display()),
        1,
    )
    .await
    .unwrap();
    sqlx::query("UPDATE _sqlx_migrations SET description=description||' changed' WHERE version=1")
        .execute(&member)
        .await
        .unwrap();
    assert!(compare_images(&before, &after, &p).await.is_err());
    member.close().await;
    assert_eq!(
        std::fs::read(before.join("core.db")).unwrap(),
        original_bytes
    );
    assert_eq!(std::fs::read(before.join("core.db-wal")).ok(), original_wal);
}

#[tokio::test]
async fn contact_upgrade_refuses_unknown_checksums_or_current_source_as_schema_23() {
    let root = tempfile::TempDir::new().unwrap();
    let (p, _keep) = seed(root.path()).await;
    let before = root.path().join("before");
    copy_images(&p, &before);
    let pool = crate::sqlite::connect_durable(
        &format!("sqlite://{}", before.join("core.db").display()),
        1,
    )
    .await
    .unwrap();
    let checksum: Vec<u8> =
        sqlx::query_scalar("SELECT checksum FROM _sqlx_migrations WHERE version=23")
            .fetch_one(&pool)
            .await
            .unwrap();
    sqlx::query("UPDATE _sqlx_migrations SET checksum=zeroblob(48) WHERE version=23")
        .execute(&pool)
        .await
        .unwrap();
    assert!(database::inspect_before_contact(&before, &p).await.is_err());
    sqlx::query("UPDATE _sqlx_migrations SET checksum=? WHERE version=23")
        .bind(checksum)
        .execute(&pool)
        .await
        .unwrap();
    database::inspect_before_contact(&before, &p).await.unwrap();
    sqlx::migrate!("./migrations").run(&pool).await.unwrap();
    database::inspect(&before, &p).await.unwrap();
    assert!(database::inspect_before_contact(&before, &p).await.is_err());
    pool.close().await;
}

#[tokio::test]
async fn additive_upgrade_refuses_prepopulated_private_state_before_activation() {
    let root = tempfile::TempDir::new().unwrap();
    let (p, _keep) = seed(root.path()).await;
    let before = root.path().join("before");
    let after = root.path().join("after");
    copy_images(&p, &before);
    copy_images(&p, &after);
    let ledger = crate::ledger::LedgerStore::connect(&format!(
        "sqlite://{}",
        after.join("core.db").display()
    ))
    .await
    .unwrap();
    ledger
        .create_contact_intent(&crate::contact::tests::fixture())
        .await
        .unwrap();
    assert!(compare_images(&before, &after, &p).await.is_err());
}
