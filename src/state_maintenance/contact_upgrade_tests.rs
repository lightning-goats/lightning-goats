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

/// Required separately in disposable CI under actual root file authority.
#[tokio::test]
#[ignore = "required isolated root private-contact upgrade fixture"]
async fn root_contact_upgrade_compares_private_images_and_retains_original_backup() {
    use std::os::unix::fs::PermissionsExt as _;
    assert_eq!(crate::private_ipc::uid().unwrap(), 0);
    let root = tempfile::Builder::new()
        .prefix("lg-contact-upgrade-")
        .tempdir_in("/run")
        .unwrap();
    let (p, _keep) = seed(root.path()).await;
    let policy_path = root.path().join("policy.toml");
    files::write_new(&policy_path, toml::to_string(&p).unwrap().as_bytes()).unwrap();
    let operator = Operator::load(&policy_path).unwrap();
    let before = root.path().join("before");
    let after = root.path().join("after");
    copy_images(&p, &before);
    copy_images(&p, &after);
    let protect = |directory: &Path| {
        std::fs::set_permissions(directory, std::fs::Permissions::from_mode(0o700)).unwrap();
        for entry in std::fs::read_dir(directory).unwrap() {
            std::fs::set_permissions(
                entry.unwrap().path(),
                std::fs::Permissions::from_mode(0o600),
            )
            .unwrap();
        }
    };
    protect(&before);
    protect(&after);
    // Reproduce the preceding native snapshot's manifest using its exact report
    // and image contract. This is synthetic data; no source or live service is
    // stopped and the source-schema exception is not added to ordinary snapshot.
    let mut images = Vec::new();
    for role in Role::ALL {
        for suffix in ["", "-wal"] {
            let path = before.join(format!("{}{suffix}", role.file()));
            if path.exists() {
                images.push(files::private_image(&path, p.max_component_bytes).unwrap());
            }
        }
    }
    let manifest = Manifest {
        version: 1,
        id: uuid::Uuid::new_v4(),
        created_at: crate::cyberherd::calendar::now().unwrap(),
        policy: p.clone(),
        source_policy_sha256: files::hash_bytes(&std::fs::read(&policy_path).unwrap()),
        recovery_required: true,
        report: database::inspect_before_contact(&before, &p).await.unwrap(),
        images,
    };
    files::write_new(
        &before.join("MANIFEST.json"),
        &serde_json::to_vec(&manifest).unwrap(),
    )
    .unwrap();
    let original_manifest = std::fs::read(before.join("MANIFEST.json")).unwrap();
    assert!(operator.verify(&before).await.is_err());
    assert!(
        operator
            .compare_contact_upgrade(&before, &after)
            .await
            .is_err()
    );
    let pool =
        crate::sqlite::connect_durable(&format!("sqlite://{}", after.join("core.db").display()), 1)
            .await
            .unwrap();
    sqlx::migrate!("./migrations").run(&pool).await.unwrap();
    pool.close().await;
    protect(&after);
    assert_eq!(
        operator
            .compare_contact_upgrade(&before, &after)
            .await
            .unwrap(),
        manifest
    );
    assert!(!manifest.report.resume_authorized);
    let alias = root.path().join("aliased-candidate");
    std::os::unix::fs::symlink(&after, &alias).unwrap();
    assert!(
        operator
            .compare_contact_upgrade(&before, &alias)
            .await
            .is_err()
    );
    let hardlink = root.path().join("aliased-core.db");
    std::fs::hard_link(after.join("core.db"), &hardlink).unwrap();
    assert!(
        operator
            .compare_contact_upgrade(&before, &after)
            .await
            .is_err()
    );
    std::fs::remove_file(hardlink).unwrap();
    files::write_new(&after.join("unlisted"), b"unqualified").unwrap();
    assert!(
        operator
            .compare_contact_upgrade(&before, &after)
            .await
            .is_err()
    );
    std::fs::remove_file(after.join("unlisted")).unwrap();
    files::write_new(&after.join("INCOMPLETE"), b"not accepted").unwrap();
    assert!(
        operator
            .compare_contact_upgrade(&before, &after)
            .await
            .is_err()
    );
    std::fs::remove_file(after.join("INCOMPLETE")).unwrap();
    std::fs::set_permissions(
        after.join("core.db"),
        std::fs::Permissions::from_mode(0o644),
    )
    .unwrap();
    assert!(
        operator
            .compare_contact_upgrade(&before, &after)
            .await
            .is_err()
    );
    std::fs::set_permissions(
        after.join("core.db"),
        std::fs::Permissions::from_mode(0o600),
    )
    .unwrap();
    assert_eq!(
        operator
            .compare_contact_upgrade(&before, &after)
            .await
            .unwrap(),
        manifest
    );
    assert_eq!(
        std::fs::read(before.join("MANIFEST.json")).unwrap(),
        original_manifest
    );
    files::check_images(&before, &manifest.images, p.max_component_bytes).unwrap();
    println!("ROOT_CONTACT_UPGRADE_FIXTURE_VERIFIED");
}
