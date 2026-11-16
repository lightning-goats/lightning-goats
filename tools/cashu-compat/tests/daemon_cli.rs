use std::process::Command;

#[test]
fn executable_help_usage_and_config_failure_are_bounded_and_redacted() {
    let executable = env!("CARGO_BIN_EXE_lightning-goats-cashud");
    let help = Command::new(executable).arg("--help").output().unwrap();
    assert!(help.status.success());
    assert!(
        String::from_utf8(help.stdout)
            .unwrap()
            .contains("check-config|initialize|run")
    );
    let missing = Command::new(executable).output().unwrap();
    assert_eq!(missing.status.code(), Some(2));
    let prepare = Command::new(executable)
        .args([
            "prepare-announcement",
            "/missing/private-token-fixture",
            "00000000-0000-0000-0000-000000000001",
        ])
        .output()
        .unwrap();
    assert_eq!(prepare.status.code(), Some(1));
    assert!(prepare.stdout.is_empty());
    assert_eq!(prepare.stderr, b"wallet configuration rejected\n");
    for command in ["check-config", "initialize", "run", "status"] {
        let failed = Command::new(executable)
            .args([command, "/missing/private-token-fixture"])
            .output()
            .unwrap();
        assert_eq!(failed.status.code(), Some(1));
        assert!(failed.stdout.is_empty());
        assert_eq!(failed.stderr, b"wallet configuration rejected\n");
    }
}
