//! Disposable actual-root/member-UID round trip; never uses systemd or network.
use super::*;
use std::os::unix::fs::PermissionsExt as _;

#[tokio::test]
async fn member_cutover_child_fixture() {
    let Some(root) = std::env::var_os("LG_CUTOFF_CHILD_FIXTURE") else {
        return;
    };
    let root = PathBuf::from(root);
    assert_ne!(crate::private_ipc::uid().unwrap(), 0);
    let config = crate::cyberherd::runtime::Config::load(&root.join("member.toml")).unwrap();
    let pin = std::env::var("LG_CUTOFF_CHILD_PIN").unwrap();
    let receipt = config
        .record_initial_reward_cutover(
            &root.join("member.toml"),
            &root.join("reviewed.json"),
            &pin,
            true,
        )
        .await
        .unwrap();
    assert!(!receipt.activation_authorized);
    assert_eq!(receipt.historical_feedings, 1);
    let retried = config
        .record_initial_reward_cutover(
            &root.join("member.toml"),
            &root.join("reviewed.json"),
            &pin,
            true,
        )
        .await
        .unwrap();
    assert_eq!(receipt, retried);
    assert!(
        config
            .record_initial_reward_cutover(
                &root.join("member.toml"),
                &root.join("reviewed.json"),
                &"00".repeat(32),
                true
            )
            .await
            .is_err()
    );
    assert!(
        config
            .record_initial_reward_cutover(
                &root.join("member.toml"),
                &root.join("reviewed.json"),
                &pin,
                false
            )
            .await
            .is_err()
    );
    println!("MEMBER_UID_CUTOFF_CHILD_VERIFIED");
}

#[tokio::test]
async fn root_cutover_round_trip_preserves_four_journals_and_refuses_changed_writer_guard() {
    if crate::private_ipc::uid().unwrap() != 0 {
        return;
    }
    let root = tempfile::Builder::new()
        .prefix("lg-cutoff-fixture-")
        .tempdir_in("/run")
        .unwrap();
    std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o711)).unwrap();
    let (p, (_core, member, _payout, _signer)) =
        reward_cutover_compare::tests::seed(root.path()).await;
    member.close_after_initialization().await;
    for c in p.components.values() {
        let directory = c.database.parent().unwrap();
        for entry in std::fs::read_dir(directory).unwrap() {
            let path = entry.unwrap().path();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
            std::os::unix::fs::chown(&path, Some(c.uid), Some(c.gid)).unwrap();
        }
        std::fs::set_permissions(directory, std::fs::Permissions::from_mode(0o700)).unwrap();
        std::os::unix::fs::chown(directory, Some(c.uid), Some(c.gid)).unwrap();
    }
    let policy_path = root.path().join("policy.toml");
    files::write_new(&policy_path, toml::to_string(&p).unwrap().as_bytes()).unwrap();
    let ready = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
    let mut operator = Operator::load(&policy_path).unwrap();
    operator.control = control::Control::Fixture(ready.clone());
    let bundle = root.path().join("bundle");
    operator.snapshot(&bundle).await.unwrap();
    let preview_dir = root.path().join("preview");
    operator
        .preview_initial_reward_cutover(&bundle, &preview_dir, Uuid::new_v4())
        .await
        .unwrap();
    let bytes = std::fs::read(preview_dir.join("PREVIEW.json")).unwrap();
    let pin = files::hash_bytes(&bytes);
    let preview_path = root.path().join("reviewed.json");
    files::write_new(&preview_path, &bytes).unwrap();
    let member = &p.components[&Role::Membership];
    std::os::unix::fs::chown(&preview_path, Some(0), Some(member.gid)).unwrap();
    std::fs::set_permissions(&preview_path, std::fs::Permissions::from_mode(0o640)).unwrap();
    let config = format!(
        r#"enabled = true
state_dir = "{}"
core_socket = "/run/nonexistent-cutoff-core.sock"
project_pubkey = "{}"
tracked_tags = ["CyberHerd"]
timezone = "America/Denver"
midnight_reset = true
relays = ["wss://relay.damus.io/"]
required_relays = 1
poll_seconds = 5
[settings]
minimum_sats = 10
max_members = 3
member_percent = 10
[core]
core_uid = {}
epoch = "{}"
start_after = 0
credit_pool = "herd"
[core.payees]
herd = "{}"
"#,
        member.database.parent().unwrap().display(),
        p.owner,
        p.components[&Role::Core].uid,
        p.epoch,
        p.owner
    );
    let config_path = root.path().join("member.toml");
    files::write_new(&config_path, config.as_bytes()).unwrap();
    std::fs::set_permissions(&config_path, std::fs::Permissions::from_mode(0o644)).unwrap();
    let loaded = crate::cyberherd::runtime::Config::load(&config_path).unwrap();
    assert!(
        loaded
            .record_initial_reward_cutover(&config_path, &preview_path, &pin, true)
            .await
            .is_err()
    );
    // Executable is disk-backed, root-protected and separate from /run's noexec mount.
    let executable_root = tempfile::Builder::new()
        .prefix("lg-cutoff-child-")
        .tempdir_in("/var/lib")
        .unwrap();
    std::fs::set_permissions(
        executable_root.path(),
        std::fs::Permissions::from_mode(0o755),
    )
    .unwrap();
    let executable = executable_root.path().join("native-test");
    std::fs::copy(std::env::current_exe().unwrap(), &executable).unwrap();
    std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o755)).unwrap();
    let output = tokio::process::Command::new("/usr/bin/setpriv")
        .args([
            "--reuid",
            &member.uid.to_string(),
            "--regid",
            &member.gid.to_string(),
            "--clear-groups",
            "--",
        ])
        .arg(&executable)
        .args([
            "--exact",
            "state_maintenance::reward_cutover_apply_tests::member_cutover_child_fixture",
            "--nocapture",
        ])
        .env_clear()
        .env("LG_CUTOFF_CHILD_FIXTURE", root.path())
        .env("LG_CUTOFF_CHILD_PIN", &pin)
        .output()
        .await
        .unwrap();
    assert!(
        output.status.success(),
        "{} {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("MEMBER_UID_CUTOFF_CHILD_VERIFIED"));
    let candidate = root.path().join("candidate");
    contact_upgrade::tests::copy_images(&p, &candidate);
    std::fs::set_permissions(&candidate, std::fs::Permissions::from_mode(0o700)).unwrap();
    for entry in std::fs::read_dir(&candidate).unwrap() {
        std::fs::set_permissions(
            entry.unwrap().path(),
            std::fs::Permissions::from_mode(0o600),
        )
        .unwrap();
    }
    operator
        .compare_initial_reward_cutover(&bundle, &preview_dir, &pin, &candidate)
        .await
        .unwrap();
    ready.store(false, std::sync::atomic::Ordering::SeqCst);
    assert!(
        operator
            .compare_initial_reward_cutover(&bundle, &preview_dir, &pin, &candidate)
            .await
            .is_err()
    );
    println!("ROOT_INITIAL_REWARD_CUTOVER_FIXTURE_VERIFIED");
}
