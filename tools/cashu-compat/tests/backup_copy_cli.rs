use std::process::Command;
#[test]
fn copy_helper_help_and_refusals_do_not_echo_private_arguments() {
    let program = env!("CARGO_BIN_EXE_lightning-goats-cashu-statectl");
    let help = Command::new(program).arg("--help").output().unwrap();
    assert!(
        help.status.success()
            && String::from_utf8(help.stdout)
                .unwrap()
                .contains("normalize-copy")
    );
    let invalid = Command::new(program).arg("unrecognized").output().unwrap();
    assert_eq!(invalid.status.code(), Some(2));
    let refused = Command::new(program)
        .args([
            "normalize-copy",
            "/missing/private-backup-fixture",
            "raw-private-source-token.db",
            "normalized-private-output-token.db",
            "8388608",
        ])
        .output()
        .unwrap();
    assert_eq!(refused.status.code(), Some(1));
    assert!(refused.stdout.is_empty());
    assert_eq!(
        refused.stderr,
        b"Database copy normalization refused; preserve the incomplete private stage.\n"
    );
}
