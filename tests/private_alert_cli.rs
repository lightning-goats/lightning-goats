#![cfg(unix)]
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Command, Output},
};
use tempfile::TempDir;

struct Fixture {
    root: TempDir,
    runtime: serde_json::Value,
}
impl Fixture {
    fn new() -> Self {
        let root = TempDir::new().unwrap();
        let runtime = serde_json::json!({
            "database_url":format!("sqlite://{}",root.path().join("alerts.db").display()),
            "strike_api_url":"http://127.0.0.1:9/v1/",
            "nak_path":"/definitely-missing-nak",
            "nak_config_path":root.path().join("nak"),
            "bunker_pubkey":"ab".repeat(32), "bunker_relays":["wss://signer.invalid"]
        });
        let f = Self { root, runtime };
        f.write(
            "private-alert-runtime",
            &serde_json::to_vec(&f.runtime).unwrap(),
        );
        f.write(
            "private-alert-policy",
            &serde_json::to_vec(&serde_json::json!({
                "threshold_sats":100, "recipient":"cd".repeat(32),
                "inbox_relays":["wss://inbox.invalid"],"account_binding":"synthetic-account"
            }))
            .unwrap(),
        );
        f
    }
    fn write(&self, name: &str, data: &[u8]) {
        let p = self.root.path().join(name);
        let _ = fs::remove_file(&p);
        fs::write(&p, data).unwrap();
        fs::set_permissions(&p, fs::Permissions::from_mode(0o400)).unwrap();
    }
    fn run(&self, action: &str) -> Output {
        Command::new(env!("CARGO_BIN_EXE_lightning-goatsctl"))
            .env_clear()
            .env("CREDENTIALS_DIRECTORY", self.root.path())
            .args([
                "--config",
                "/definitely-missing-financial-config",
                "private-alert",
                action,
            ])
            .output()
            .unwrap()
    }
    fn db(&self) -> PathBuf {
        self.root.path().join("alerts.db")
    }
}
fn success(output: Output) {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stdout.is_empty());
}
fn failure(output: Output) {
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert_eq!(
        String::from_utf8_lossy(&output.stderr).trim(),
        "Error: private alert command failed; review protected configuration and state"
    );
}

#[test]
fn offline_lifecycle_never_requires_financial_config_provider_or_signer() {
    let f = Fixture::new();
    success(f.run("initialize"));
    assert!(f.db().is_file());
    success(f.run("check"));
    failure(f.run("initialize"));
    failure(f.run("run")); // dedicated keys absent: stop before client/subprocess construction.
    success(f.run("check"));
    assert!(!f.root.path().join("nak").exists());
    fs::remove_file(f.db()).unwrap();
    failure(f.run("check")); // lost state never silently bootstraps.
}

#[test]
fn provider_and_signer_reassignment_require_reconciliation() {
    let mut f = Fixture::new();
    success(f.run("initialize"));
    for field in ["strike_api_url", "bunker_pubkey", "bunker_relays"] {
        let saved = f.runtime.clone();
        f.runtime[field] = match field {
            "strike_api_url" => "http://127.0.0.1:10/v1/".into(),
            "bunker_pubkey" => "ef".repeat(32).into(),
            _ => serde_json::json!(["wss://replacement.invalid"]),
        };
        f.write(
            "private-alert-runtime",
            &serde_json::to_vec(&f.runtime).unwrap(),
        );
        failure(f.run("check"));
        f.runtime = saved;
        f.write(
            "private-alert-runtime",
            &serde_json::to_vec(&f.runtime).unwrap(),
        );
        success(f.run("check"));
    }
}

#[test]
fn invalid_runtime_is_redacted_before_state_creation() {
    let mut f = Fixture::new();
    for (field, value) in [
        ("nak_path", serde_json::json!("relative/SECRET-SENTINEL")),
        (
            "strike_api_url",
            serde_json::json!("https://SECRET-SENTINEL:password@example.invalid/"),
        ),
        ("bunker_pubkey", serde_json::json!("SECRET-SENTINEL")),
        ("bunker_relays", serde_json::json!([])),
        ("unknown_SECRET_SENTINEL", serde_json::json!(true)),
    ] {
        let original = f.runtime.clone();
        f.runtime[field] = value;
        f.write(
            "private-alert-runtime",
            &serde_json::to_vec(&f.runtime).unwrap(),
        );
        failure(f.run("initialize"));
        assert!(!Path::new(&f.db()).exists());
        f.runtime = original;
    }
}

#[tokio::test]
async fn actual_cli_polls_mock_provider_delivers_once_and_preserves_episode_on_restart() {
    use axum::{Json, Router, routing::get};
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    use std::time::Duration;
    let reads = Arc::new(AtomicUsize::new(0));
    let observed = reads.clone();
    let app = Router::new().route(
        "/v1/balances",
        get(move || {
            let reads = observed.clone();
            async move {
                reads.fetch_add(1, Ordering::SeqCst);
                Json(serde_json::json!([
                    {"currency":"BTC","current":"0.00000100"}
                ]))
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let mut f = Fixture::new();
    let nak = f.root.path().join("nak-stub");
    fs::write(&nak, include_str!("fixtures/private_nak_stub.py")).unwrap();
    fs::set_permissions(&nak, fs::Permissions::from_mode(0o700)).unwrap();
    let nak_state = f.root.path().join("nak");
    fs::create_dir(&nak_state).unwrap();
    fs::write(nak_state.join("mode"), "success").unwrap();
    f.runtime["strike_api_url"] = format!("http://{address}/v1/").into();
    f.runtime["nak_path"] = nak.to_str().unwrap().into();
    f.write(
        "private-alert-runtime",
        &serde_json::to_vec(&f.runtime).unwrap(),
    );
    f.write("private-alert-strike-key", b"synthetic-balance-read");
    f.write("private-alert-nostr-key", b"synthetic-nip46-client");
    success(f.run("initialize"));
    let calls = || -> Vec<serde_json::Value> {
        fs::read_to_string(nak_state.join("calls.jsonl"))
            .unwrap_or_default()
            .lines()
            .filter_map(|s| serde_json::from_str(s).ok())
            .collect()
    };
    for restart in [false, true] {
        let before = reads.load(Ordering::SeqCst);
        let mut command = tokio::process::Command::new(env!("CARGO_BIN_EXE_lightning-goatsctl"));
        command
            .env_clear()
            .env("CREDENTIALS_DIRECTORY", f.root.path())
            .args(["private-alert", "run"])
            .kill_on_drop(true)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());
        let child = command.spawn().unwrap();
        let id = child.id().unwrap();
        let reached = tokio::time::timeout(Duration::from_secs(25), async {
            loop {
                if (restart && reads.load(Ordering::SeqCst) > before)
                    || (!restart && calls().iter().any(|c| c["phase"] == "publish"))
                {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        })
        .await
        .is_ok();
        let stopped = Command::new("/usr/bin/kill")
            .args(["-TERM", &id.to_string()])
            .status()
            .unwrap();
        assert!(stopped.success());
        let output = tokio::time::timeout(Duration::from_secs(10), child.wait_with_output())
            .await
            .unwrap()
            .unwrap();
        assert!(reached, "mock observation/delivery deadline");
        success(output);
    }
    server.abort();
    assert_eq!(calls().iter().filter(|c| c["phase"] == "wrap").count(), 1);
    assert_eq!(
        calls().iter().filter(|c| c["phase"] == "publish").count(),
        1
    );
    success(f.run("check"));
}
