use std::process::Command;
#[test]
fn signer_cli_never_echoes_private_paths_and_requires_an_explicit_mode() {
    let executable = env!("CARGO_BIN_EXE_lightning-goats-cashu-signerd");
    let help = Command::new(executable).arg("--help").output().unwrap();
    assert!(help.status.success());
    assert!(
        String::from_utf8(help.stdout)
            .unwrap()
            .contains("check-config|initialize|run")
    );
    for args in [
        vec![],
        vec!["pay", "invoice"],
        vec!["run"],
        vec!["run", "path", "extra"],
    ] {
        assert_eq!(
            Command::new(executable)
                .args(args)
                .output()
                .unwrap()
                .status
                .code(),
            Some(2)
        );
    }
    for mode in ["check-config", "initialize", "run"] {
        let output = Command::new(executable)
            .args([mode, "/missing/private-client-secret"])
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(1));
        assert!(output.stdout.is_empty());
        assert_eq!(output.stderr, b"signer configuration rejected\n");
    }
}
