use super::*;
use std::os::unix::fs::PermissionsExt as _;

async fn historical(path: &Path) -> sqlx::SqlitePool {
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(
            sqlx::sqlite::SqliteConnectOptions::new()
                .filename(path)
                .create_if_missing(true)
                .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal)
                .synchronous(sqlx::sqlite::SqliteSynchronous::Full)
                .foreign_keys(true),
        )
        .await
        .unwrap();
    let mut migrations = sqlx::migrate::Migrator::new(Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/migrations"
    )))
    .await
    .unwrap();
    migrations.migrations.to_mut().retain(|m| m.version <= 13);
    migrations.run(&pool).await.unwrap();
    sqlx::query("INSERT INTO overlay_identity(singleton,stream_id) VALUES(1,?)")
        .bind(uuid::Uuid::new_v4().to_string())
        .execute(&pool)
        .await
        .unwrap();
    pool
}

#[tokio::test]
async fn legacy_policy_checks_reject_root_aliases_secrets_before_any_state_access() {
    let root = tempfile::TempDir::new().unwrap();
    let path = root.path().join("policy.toml");
    let text = include_str!("../../../deploy/cyberherd/core-upgrade.toml.example")
        .replace("uid = 0", "uid = 50010")
        .replace("gid = 0", "gid = 50010");
    std::fs::write(&path, &text).unwrap();
    Policy::load_for_check(&path).unwrap();
    for bad in [
        text.replace("uid = 50010", "uid = 0"),
        text.replace("gid = 50010", "gid = 0"),
        format!("api_key='synthetic-forbidden'\n{text}"),
        text.replace("lightning-goats-receive.service", "--untrusted.service"),
    ] {
        std::fs::write(&path, bad).unwrap();
        assert!(Policy::load_for_check(&path).is_err());
    }
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 1);
}

#[tokio::test]
async fn native_legacy_verification_rejects_dirty_skipped_unknown_and_cyberherd_history_without_migration()
 {
    let root = tempfile::TempDir::new().unwrap();
    let pool = historical(&root.path().join("core.db")).await;
    let good = rows::inspect(root.path()).await.unwrap();
    assert_eq!(good.migrations.len(), 13);
    let checksum: Vec<u8> =
        sqlx::query_scalar("SELECT checksum FROM _sqlx_migrations WHERE version=13")
            .fetch_one(&pool)
            .await
            .unwrap();
    for statement in [
        "UPDATE _sqlx_migrations SET success=0 WHERE version=13",
        "UPDATE _sqlx_migrations SET checksum=x'00' WHERE version=13",
        "UPDATE _sqlx_migrations SET version=999 WHERE version=13",
    ] {
        sqlx::query(statement).execute(&pool).await.unwrap();
        assert!(rows::inspect(root.path()).await.is_err());
        sqlx::query(
            "UPDATE _sqlx_migrations SET success=1,version=13,checksum=? WHERE version IN (13,999)",
        )
        .bind(&checksum)
        .execute(&pool)
        .await
        .unwrap();
    }
    assert_eq!(rows::inspect(root.path()).await.unwrap(), good);
    let current = crate::ledger::LedgerStore::connect(&format!(
        "sqlite://{}",
        root.path().join("core.db").display()
    ))
    .await
    .unwrap();
    assert!(rows::inspect(root.path()).await.is_err());
    drop(current);
    pool.close().await;
}

#[tokio::test]
#[ignore = "required isolated root legacy upgrade fixture"]
async fn root_legacy_core_upgrade_preserves_wal_pending_receipts_epoch_signed_bytes_and_compares_migrated_copies()
 {
    assert_eq!(crate::private_ipc::uid().unwrap(), 0);
    let root = tempfile::Builder::new()
        .prefix("lg-legacy-upgrade-")
        .tempdir_in("/run")
        .unwrap();
    std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let source = root.path().join("source");
    std::fs::create_dir(&source).unwrap();
    let database = source.join("lightning-goats.db");
    let pool = historical(&database).await;
    let pending = uuid::Uuid::new_v4();
    let signed =
        crate::nostr_event::test_event(1, 1, 100, "Keep exact original signed bytes", vec![]);
    let signed_json = serde_json::to_string(&signed).unwrap();
    sqlx::query("INSERT INTO settled_payments(source,source_id,address_user,credit_pool,amount_msat) VALUES('strike','original-receipt','herd','herd',2340000)").execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO ledger_entries(entry_type,source_key,delta_sats,payment_source,payment_source_id,address_user,credit_pool) VALUES('CREDIT','original-receipt',2340,'strike','original-receipt','herd','herd')").execute(&pool).await.unwrap();
    sqlx::query(
        "INSERT INTO feed_attempts(id,status,threshold_sats) VALUES(?,'intent_committed',1000)",
    )
    .bind(pending.to_string())
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query("INSERT INTO event_log(event_type,payload_json) VALUES('payment_received','{}')")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO message_outbox(event_id,signed_event_json,status,source_event_seq) VALUES(?,?,'pending',1)").bind(&signed.id).bind(&signed_json).execute(&pool).await.unwrap();
    sqlx::query("CREATE TABLE probe_values (n INTEGER,real_value REAL,text_value TEXT,blob_value BLOB,null_value TEXT)").execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO probe_values VALUES(?,-1.25,?,?,NULL)")
        .bind(i64::MAX)
        .bind("exact\0text")
        .bind(vec![0u8, 255, 17, 0])
        .execute(&pool)
        .await
        .unwrap();
    let mut originals = Vec::new();
    for path in [
        database.clone(),
        files::sidecar(&database, "-wal"),
        files::sidecar(&database, "-shm"),
    ] {
        if path.exists() {
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
            std::os::unix::fs::chown(&path, Some(50010), Some(50010)).unwrap();
            originals.push((path.clone(), std::fs::read(path).unwrap()));
        }
    }
    std::fs::set_permissions(&source, std::fs::Permissions::from_mode(0o700)).unwrap();
    std::os::unix::fs::chown(&source, Some(50010), Some(50010)).unwrap();
    let policy = Policy {
        version: 1,
        core: Component {
            database,
            uid: 50010,
            gid: 50010,
            unit: "legacy-core-fixture.service".into(),
        },
        max_component_bytes: 128 * 1024 * 1024,
    };
    let config = root.path().join("policy.toml");
    files::write_new(&config, toml::to_string(&policy).unwrap().as_bytes()).unwrap();
    let mut operator = Operator::load(&config).unwrap();
    let ready = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
    operator.control = Control::Fixture(ready.clone());
    let bundle = root.path().join("bundle");
    let manifest = operator.snapshot(&bundle).await.unwrap();
    assert_eq!(manifest.report.migrations.len(), 13);
    assert_eq!(manifest.report.tables["settled_payments"].rows, 1);
    assert_eq!(manifest.report.tables["message_outbox"].rows, 1);
    assert!(
        manifest
            .images
            .iter()
            .any(|i| i.name.ends_with("-wal") && i.bytes > 0)
    );
    assert_eq!(operator.verify(&bundle).await.unwrap(), manifest);
    for (path, bytes) in &originals {
        assert_eq!(std::fs::read(path).unwrap(), *bytes);
    }
    let candidate = root.path().join("candidate");
    files::new_private_directory(&candidate).unwrap();
    for image in &manifest.images {
        files::copy_private(
            &bundle.join(&image.name),
            &candidate.join(&image.name),
            image,
        )
        .unwrap();
    }
    assert!(operator.compare_upgrade(&bundle, &candidate).await.is_err()); // still old schema
    let url = format!("sqlite://{}", candidate.join("core.db").display());
    let current = crate::ledger::LedgerStore::connect(&url).await.unwrap();
    assert_eq!(
        current.overlay_stream_id().await.unwrap(),
        manifest.report.epoch
    );
    assert_eq!(
        current.unresolved_feed_attempt().await.unwrap().unwrap().id,
        pending
    );
    assert_eq!(
        operator.compare_upgrade(&bundle, &candidate).await.unwrap(),
        manifest
    );
    let changed = sqlx::SqlitePool::connect(&url).await.unwrap();
    assert_eq!(
        sqlx::query_scalar::<_, String>("SELECT signed_event_json FROM message_outbox")
            .fetch_one(&changed)
            .await
            .unwrap(),
        signed_json
    );
    sqlx::query("UPDATE probe_values SET text_value=?")
        .bind("exact\0CHANGED")
        .execute(&changed)
        .await
        .unwrap();
    assert!(operator.compare_upgrade(&bundle, &candidate).await.is_err());
    sqlx::query("UPDATE probe_values SET text_value=?")
        .bind("exact\0text")
        .execute(&changed)
        .await
        .unwrap();
    assert_eq!(
        operator.compare_upgrade(&bundle, &candidate).await.unwrap(),
        manifest
    );
    sqlx::query("UPDATE overlay_identity SET stream_id=?")
        .bind(uuid::Uuid::new_v4().to_string())
        .execute(&changed)
        .await
        .unwrap();
    assert!(operator.compare_upgrade(&bundle, &candidate).await.is_err());
    sqlx::query("UPDATE overlay_identity SET stream_id=?")
        .bind(manifest.report.epoch.to_string())
        .execute(&changed)
        .await
        .unwrap();
    files::write_new(&candidate.join("INCOMPLETE"), b"incomplete").unwrap();
    assert!(operator.compare_upgrade(&bundle, &candidate).await.is_err());
    std::fs::remove_file(candidate.join("INCOMPLETE")).unwrap();
    assert_eq!(
        operator.compare_upgrade(&bundle, &candidate).await.unwrap(),
        manifest
    );
    ready.store(false, std::sync::atomic::Ordering::SeqCst);
    let refused = root.path().join("refused");
    assert!(operator.snapshot(&refused).await.is_err());
    assert!(!refused.exists());
    std::fs::write(bundle.join("core.db"), b"corrupt").unwrap();
    assert!(operator.verify(&bundle).await.is_err());
    changed.close().await;
    drop(current);
    pool.close().await;
    println!("ROOT_LEGACY_CORE_UPGRADE_FIXTURE_VERIFIED");
}
