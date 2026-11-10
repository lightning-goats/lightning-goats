use super::*;
use std::os::unix::fs::PermissionsExt as _;

fn policy(root: &Path) -> Policy {
    Policy {
        version: 1,
        owner: crate::nostr_event::test_event(1, 0, 100, "", vec![]).pubkey,
        epoch: Uuid::new_v4(),
        provider: crate::payout::funding::ProviderScope {
            environment: "strike-production".into(),
            account_id: Uuid::new_v4(),
            credential_fingerprint: "aa".repeat(32),
        },
        max_component_bytes: 128 * 1024 * 1024,
        components: Role::ALL
            .into_iter()
            .enumerate()
            .map(|(i, role)| {
                (
                    role,
                    Component {
                        database: root.join(role.name()).join(role.restore_file()),
                        uid: 10001 + i as u32,
                        gid: 10001 + i as u32,
                        unit: format!("lg-test-{}.service", role.name()),
                    },
                )
            })
            .collect(),
    }
}
fn payout_policy(p: &Policy) -> crate::payout::store::Policy {
    crate::payout::store::Policy {
        scope: crate::payout::store::Scope {
            core_uid: p.components[&Role::Core].uid,
            epoch: p.epoch,
            owner: p.owner.clone(),
            provider: p.provider.clone(),
        },
        member_percent_ceiling: 0,
        max_sats_per_feed: 0,
        max_sats_per_24h: 0,
        max_fee_sats: 0,
        max_source_parts: 1,
        operation_addresses: vec![],
        receipt_relays: vec!["wss://relay.damus.io/".into()],
        execution_enabled: false,
    }
}
fn signer_policy(p: &Policy) -> crate::payout::signer::Policy {
    crate::payout::signer::Policy {
        owner: p.owner.clone(),
        client_pubkey: crate::nostr_event::test_event(2, 0, 100, "", vec![]).pubkey,
        peer_uid: p.components[&Role::Payout].uid,
        relays: vec!["wss://relay.damus.io/".into()],
        max_net_sats: 0,
        max_new_per_hour: 0,
    }
}

#[tokio::test]
async fn policy_check_has_no_file_authority_and_rejects_missing_roles_shared_owners_secrets_and_bad_scope()
 {
    let root = tempfile::TempDir::new().unwrap();
    let good = policy(root.path());
    good.validate().unwrap();
    for change in 0..5 {
        let mut bad = good.clone();
        match change {
            0 => {
                bad.components.remove(&Role::Signer);
            }
            1 => bad.components.get_mut(&Role::Payout).unwrap().uid = 10001,
            2 => bad.components.get_mut(&Role::Core).unwrap().unit = "--arbitrary.service".into(),
            3 => bad.provider.account_id = Uuid::nil(),
            _ => bad.epoch = Uuid::nil(),
        }
        assert!(bad.validate().is_err());
    }
    let path = root.path().join("policy.toml");
    std::fs::write(&path, toml::to_string(&good).unwrap()).unwrap();
    Policy::load_for_check(&path).unwrap();
    if crate::private_ipc::uid().unwrap() != 0 {
        assert!(Operator::load(&path).is_err());
    }
    let raw = format!(
        "api_key = 'synthetic-forbidden'\n{}",
        toml::to_string(&good).unwrap()
    );
    std::fs::write(&path, raw).unwrap();
    let error = Policy::load_for_check(&path).unwrap_err().to_string();
    assert!(!error.contains("synthetic-forbidden"));
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 1);
}

#[tokio::test]
async fn recovery_marker_blocks_all_four_store_openers_before_creating_or_migrating_state() {
    let root = tempfile::TempDir::new().unwrap();
    let p = policy(root.path());
    let marker = root.path().join(RECOVERY_MARKER);
    std::fs::write(&marker, b"pending review").unwrap();
    let url = |role: Role| {
        format!(
            "sqlite://{}",
            root.path().join(role.restore_file()).display()
        )
    };
    assert!(
        crate::ledger::LedgerStore::connect(&url(Role::Core))
            .await
            .is_err()
    );
    assert!(
        crate::cyberherd::store::Store::connect(&url(Role::Membership))
            .await
            .is_err()
    );
    assert!(
        crate::payout::store::Store::connect(&url(Role::Payout), payout_policy(&p))
            .await
            .is_err()
    );
    assert!(
        crate::payout::signer::Store::connect(&url(Role::Signer), signer_policy(&p))
            .await
            .is_err()
    );
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 1);
    std::fs::remove_file(&marker).unwrap();
    // A dangling marker remains a block; try_exists alone would miss it.
    std::os::unix::fs::symlink(root.path().join("missing"), &marker).unwrap();
    assert!(
        crate::ledger::LedgerStore::connect(&url(Role::Core))
            .await
            .is_err()
    );
    std::fs::remove_file(&marker).unwrap();
    crate::ledger::LedgerStore::connect(&url(Role::Core))
        .await
        .unwrap();
}

/// Repeated explicitly as root by CI in its disposable network namespace. An
/// unprivileged negative result cannot satisfy the required positive marker.
#[tokio::test]
async fn root_four_journal_snapshot_preserves_wal_paid_pending_and_signed_state_and_blocks_restore_startup()
 {
    if crate::private_ipc::uid().unwrap() != 0 {
        let root = tempfile::TempDir::new().unwrap();
        let path = root.path().join("policy.toml");
        std::fs::write(&path, toml::to_string(&policy(root.path())).unwrap()).unwrap();
        assert!(Operator::load(&path).is_err());
        return;
    }
    let root = tempfile::Builder::new()
        .prefix("lg-state-fixture-")
        .tempdir_in("/run")
        .unwrap();
    std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let mut p = policy(root.path());
    for c in p.components.values() {
        std::fs::create_dir(c.database.parent().unwrap()).unwrap();
    }
    let core_url = format!("sqlite://{}", p.components[&Role::Core].database.display());
    let core = crate::ledger::LedgerStore::connect(&core_url)
        .await
        .unwrap();
    p.epoch = core.overlay_stream_id().await.unwrap();
    core.record_payment(&crate::domain::payment::SettledPayment {
        source: "test".into(),
        source_id: "paid-before-snapshot".into(),
        payment_hash: None,
        address_user: "herd".into(),
        credit_pool: "herd".into(),
        amount_msat: 1_000_000,
        settled_at: None,
        context_json: None,
    })
    .await
    .unwrap();
    let feed = core.begin_feed_attempt(1000).await.unwrap().unwrap();
    let signed = crate::nostr_event::test_event(1, 1, 100, "Preserve exact signed bytes", vec![]);
    let raw = serde_json::to_string(&signed).unwrap();
    core.enqueue_signed_message(1, &signed.id, &raw)
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
    // Keep the real SQLite connections open so committed pages remain in WAL.
    let _keep_open = (&core, &membership, &payout, &signer);
    for c in p.components.values() {
        for path in [
            c.database.clone(),
            files::sidecar(&c.database, "-wal"),
            files::sidecar(&c.database, "-shm"),
        ] {
            if path.exists() {
                std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
                std::os::unix::fs::chown(&path, Some(c.uid), Some(c.gid)).unwrap();
            }
        }
        std::fs::set_permissions(
            c.database.parent().unwrap(),
            std::fs::Permissions::from_mode(0o700),
        )
        .unwrap();
        std::os::unix::fs::chown(c.database.parent().unwrap(), Some(c.uid), Some(c.gid)).unwrap();
    }
    let path = root.path().join("policy.toml");
    files::write_new(&path, toml::to_string(&p).unwrap().as_bytes()).unwrap();
    let mut operator = Operator::load(&path).unwrap();
    let ready = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
    operator.control = control::Control::Fixture(ready.clone());
    let bundle = root.path().join("bundle");
    let manifest = operator.snapshot(&bundle).await.unwrap();
    assert!(
        manifest
            .images
            .iter()
            .any(|i| i.name.ends_with("-wal") && i.bytes > 0)
    );
    assert_eq!(manifest.report.pending_feeds, 1);
    assert_eq!(
        manifest.report.retention,
        "retain_all_financial_replay_and_signed_history"
    );
    assert!(!manifest.report.resume_authorized);
    assert_eq!(operator.verify(&bundle).await.unwrap(), manifest);
    assert!(operator.snapshot(&bundle).await.is_err());
    let destination = root.path().join("restore");
    assert_eq!(
        operator.stage_restore(&bundle, &destination).await.unwrap(),
        manifest
    );
    let restored = destination.join("core/lightning-goats.db");
    assert!(
        crate::ledger::LedgerStore::connect(&format!("sqlite://{}", restored.display()))
            .await
            .is_err()
    );
    let original = files::read_private(&bundle.join("core.db"), p.max_component_bytes).unwrap();
    let db = SqliteFixture::read(&bundle.join("core.db")).await;
    let retained_feed: String = sqlx::query_scalar("SELECT id FROM feed_attempts")
        .fetch_one(&db)
        .await
        .unwrap();
    assert_eq!(retained_feed, feed.to_string());
    let retained_signed: String =
        sqlx::query_scalar("SELECT signed_event_json FROM message_outbox")
            .fetch_one(&db)
            .await
            .unwrap();
    assert_eq!(retained_signed, raw);
    db.close().await;
    assert_eq!(
        files::read_private(&bundle.join("core.db"), p.max_component_bytes).unwrap(),
        original
    );
    ready.store(false, std::sync::atomic::Ordering::SeqCst);
    let blocked = root.path().join("blocked");
    assert!(operator.snapshot(&blocked).await.is_err());
    assert!(!blocked.exists());
    ready.store(true, std::sync::atomic::Ordering::SeqCst);
    std::fs::write(bundle.join("core.db"), b"corrupt fixture").unwrap();
    assert!(operator.verify(&bundle).await.is_err());
    let not_created = root.path().join("corrupt-restore");
    assert!(operator.stage_restore(&bundle, &not_created).await.is_err());
    assert!(!not_created.exists());
    println!("ROOT_FOUR_JOURNAL_FIXTURE_VERIFIED");
}

struct SqliteFixture;
impl SqliteFixture {
    async fn read(path: &Path) -> sqlx::SqlitePool {
        sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(
                sqlx::sqlite::SqliteConnectOptions::new()
                    .filename(path)
                    .read_only(true)
                    .create_if_missing(false),
            )
            .await
            .unwrap()
    }
}
