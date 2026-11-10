use super::*;
use crate::{
    cyberherd::runtime::Config as Member,
    payout::{runtime::Config as Payout, signer::Config as Signer},
};

const CHILD: &str = "state_bootstrap::tests::bootstrap_child_uses_actual_runtime_identity_and_never_opens_a_network_worker";

#[tokio::test]
#[ignore = "child of the required isolated root bootstrap fixture"]
async fn bootstrap_child_uses_actual_runtime_identity_and_never_opens_a_network_worker() {
    let base = std::env::var_os("LG_BOOTSTRAP_FIXTURE_ROOT")
        .expect("invoke only through the isolated root parent fixture");
    let base = PathBuf::from(base);
    let role = std::env::var("LG_BOOTSTRAP_FIXTURE_ROLE").unwrap();
    let config = base.join(format!("{role}.toml"));
    let state = base.join(&role);
    let (uid, gid) = crate::core_bridge::unix::self_credentials().unwrap();
    assert_ne!(uid, 0);
    assert_ne!(gid, 0);
    assert!(std::env::var_os("CREDENTIALS_DIRECTORY").is_none());
    let initialize = || async {
        match role.as_str() {
            "membership" => Member::load(&config)?.initialize_state(&config).await,
            "payout" => Payout::load(&config)?.initialize_state(&config).await,
            "signer" => Signer::load(&config)?.initialize_state(&config).await,
            _ => bail!("unsupported fixture role"),
        }
    };
    for suffix in ["enabled", "foreign", "budget"] {
        let invalid = base.join(format!("{role}-{suffix}.toml"));
        let result = match role.as_str() {
            "membership" => {
                Member::load(&invalid)
                    .unwrap()
                    .initialize_state(&invalid)
                    .await
            }
            "payout" => {
                Payout::load(&invalid)
                    .unwrap()
                    .initialize_state(&invalid)
                    .await
            }
            _ => {
                Signer::load(&invalid)
                    .unwrap()
                    .initialize_state(&invalid)
                    .await
            }
        };
        assert!(result.is_err());
        assert_eq!(std::fs::read_dir(&state).unwrap().count(), 0);
    }
    let name = match role.as_str() {
        "membership" => "cyberherd.db",
        "payout" => "payout.db",
        _ => "signer.db",
    };
    let final_db = state.join(name);
    // Orphan recovery/SQLite artifacts must not be reinterpreted as a fresh DB.
    for blocked in [
        state.join(crate::state_maintenance::RECOVERY_MARKER),
        PathBuf::from(format!("{}-wal", final_db.display())),
        PathBuf::from(format!("{}-shm", final_db.display())),
        PathBuf::from(format!("{}-journal", final_db.display())),
    ] {
        std::fs::write(&blocked, b"retain").unwrap();
        assert!(initialize().await.is_err());
        assert!(!final_db.exists());
        assert_eq!(std::fs::read(&blocked).unwrap(), b"retain");
        std::fs::remove_file(blocked).unwrap();
    }
    let missing = state.join("missing-target");
    std::os::unix::fs::symlink(&missing, &final_db).unwrap();
    assert!(initialize().await.is_err());
    assert!(
        std::fs::symlink_metadata(&final_db)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert!(!missing.exists());
    std::fs::remove_file(&final_db).unwrap();
    // Main-file refusal includes an empty file, not only a populated database.
    std::fs::write(&final_db, b"").unwrap();
    assert!(initialize().await.is_err());
    assert_eq!(std::fs::metadata(&final_db).unwrap().len(), 0);
    std::fs::remove_file(&final_db).unwrap();
    initialize().await.unwrap();
    let metadata = std::fs::symlink_metadata(&final_db).unwrap();
    assert_eq!((metadata.uid(), metadata.gid()), (uid, gid));
    assert_eq!(metadata.mode() & 0o777, 0o600);
    assert_eq!(metadata.nlink(), 1);
    let before = std::fs::read(&final_db).unwrap();
    assert!(initialize().await.is_err());
    assert_eq!(std::fs::read(&final_db).unwrap(), before);
    assert_eq!(std::fs::read_dir(&state).unwrap().count(), 1);
    let db = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(
            sqlx::sqlite::SqliteConnectOptions::new()
                .filename(&final_db)
                .read_only(true)
                .create_if_missing(false),
        )
        .await
        .unwrap();
    for table in match role.as_str() {
        "membership" => vec!["ingress", "core_envelopes", "domain_events", "reward_plans"],
        "payout" => vec![
            "payout_grants",
            "payout_execute_intents",
            "payout_payment_bindings",
        ],
        _ => vec!["signer_intents", "signer_results"],
    } {
        assert_eq!(
            sqlx::query_scalar::<_, i64>(&format!("SELECT COUNT(*) FROM {table}"))
                .fetch_one(&db)
                .await
                .unwrap(),
            0
        );
    }
    match role.as_str() {
        "membership" => {
            assert_eq!(
                sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM owners")
                    .fetch_one(&db)
                    .await
                    .unwrap(),
                1
            );
            assert_eq!(
                sqlx::query_scalar::<_, i64>("SELECT received_through FROM core_streams")
                    .fetch_one(&db)
                    .await
                    .unwrap(),
                0
            );
        }
        "payout" => assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM payout_binding")
                .fetch_one(&db)
                .await
                .unwrap(),
            1
        ),
        _ => assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM signer_binding")
                .fetch_one(&db)
                .await
                .unwrap(),
            1
        ),
    }
    db.close().await;
    println!("ACTUAL_RUNTIME_BOOTSTRAP_VERIFIED:{role}");
}

#[tokio::test]
#[ignore = "required root fixture runs explicitly in a disposable network namespace"]
async fn root_offline_bootstrap_qualifies_three_actual_runtime_uids_and_refuses_replacing_journals()
{
    assert_eq!(
        ipc::uid().unwrap(),
        0,
        "invoke through the required isolated root harness"
    );
    let base = tempfile::Builder::new()
        .prefix("lg-bootstrap-fixture-")
        .tempdir_in("/run")
        .unwrap();
    std::fs::set_permissions(base.path(), std::fs::Permissions::from_mode(0o755)).unwrap();
    let binary = base.path().join("native-test");
    std::fs::copy(std::env::current_exe().unwrap(), &binary).unwrap();
    std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o755)).unwrap();
    let owner = crate::nostr_event::test_event(1, 0, 100, "", vec![]).pubkey;
    let client = crate::nostr_event::test_event(4, 0, 100, "", vec![]).pubkey;
    for (role, uid, text) in [
        (
            "membership",
            50011,
            include_str!("../../deploy/cyberherd/config.toml.example"),
        ),
        (
            "payout",
            50012,
            include_str!("../../deploy/cyberherd/payout.toml.example"),
        ),
        (
            "signer",
            50013,
            include_str!("../../deploy/cyberherd/signer.toml.example"),
        ),
    ] {
        let state = base.path().join(role);
        std::fs::DirBuilder::new()
            .mode(0o700)
            .create(&state)
            .unwrap();
        std::os::unix::fs::chown(&state, Some(uid), Some(uid)).unwrap();
        let mut text = text
            .replace(&"0".repeat(64), &owner)
            .replace("REPLACE_WITH_CLIENT_PUBLIC_KEY", &client)
            .replace(
                "00000000-0000-0000-0000-000000000000",
                "11111111-1111-4111-8111-111111111111",
            )
            .replace("core_uid = 0", "core_uid = 50010")
            .replace("intake_uid = 0", "intake_uid = 50011")
            .replace("signer_uid = 0", "signer_uid = 50013")
            .replace("peer_uid = 0", "peer_uid = 50012")
            .replace("group_gid = 0", "group_gid = 50014");
        text = text.replace(
            match role {
                "membership" => "/var/lib/cyberherd",
                "payout" => "/var/lib/cyberherd-payout",
                _ => "/var/lib/cyberherd-signer",
            },
            state.to_str().unwrap(),
        );
        let config = base.path().join(format!("{role}.toml"));
        std::fs::write(&config, &text).unwrap();
        std::fs::set_permissions(&config, std::fs::Permissions::from_mode(0o644)).unwrap();
        let foreign = match role {
            "membership" => text.replace("core_uid = 50010", "core_uid = 50011"),
            "payout" => text.replace("intake_uid = 50011", "intake_uid = 50012"),
            _ => text.replace("peer_uid = 50012", "peer_uid = 50013"),
        };
        let budget = match role {
            "membership" => format!(
                "{text}\n[messaging]\nenabled = true\nstart_after = 0\npoll_millis = 1000\nrelays = [\"wss://relay.damus.io/\"]\n"
            ),
            "payout" => text.replace("max_sats_per_24h = 0", "max_sats_per_24h = 1"),
            _ => text.replace("max_new_per_hour = 0", "max_new_per_hour = 1"),
        };
        for (suffix, contents) in [
            ("enabled", text.replace("enabled = false", "enabled = true")),
            ("foreign", foreign),
            ("budget", budget),
        ] {
            let path = base.path().join(format!("{role}-{suffix}.toml"));
            std::fs::write(&path, contents).unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        }
        let output = tokio::process::Command::new("/usr/bin/setpriv")
            .args([
                format!("--reuid={uid}"),
                format!("--regid={uid}"),
                "--clear-groups".into(),
                "--no-new-privs".into(),
                "--".into(),
            ])
            .arg(&binary)
            .args([CHILD, "--exact", "--ignored", "--nocapture"])
            .env_clear()
            .env("LG_BOOTSTRAP_FIXTURE_ROOT", base.path())
            .env("LG_BOOTSTRAP_FIXTURE_ROLE", role)
            .kill_on_drop(true)
            .output()
            .await
            .unwrap();
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            String::from_utf8_lossy(&output.stdout)
                .contains(&format!("ACTUAL_RUNTIME_BOOTSTRAP_VERIFIED:{role}"))
        );
    }
    println!("ROOT_OFFLINE_BOOTSTRAP_FIXTURE_VERIFIED");
}

#[test]
fn initialization_never_reuses_existing_main_sidecars_or_recovery_markers() {
    let root = tempfile::TempDir::new().unwrap();
    let main = root.path().join("payout.db");
    absent_final(&main).unwrap();
    for name in [
        "payout.db",
        "payout.db-wal",
        "payout.db-shm",
        "payout.db-journal",
        crate::state_maintenance::RECOVERY_MARKER,
    ] {
        let path = root.path().join(name);
        std::fs::write(&path, b"retain exactly").unwrap();
        assert!(absent_final(&main).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"retain exactly");
        std::fs::remove_file(&path).unwrap();
        std::os::unix::fs::symlink(root.path().join("missing"), &path).unwrap();
        assert!(absent_final(&main).is_err());
        std::fs::remove_file(&path).unwrap();
    }
    absent_final(&main).unwrap();
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
}
