use std::process::Command;

use lightning_goats::ledger::LedgerStore;
use serde_json::Value;
use tempfile::TempDir;

#[tokio::test]
async fn local_cli_requires_no_provider_and_previews_deduplicates_and_rejects_conflicts() {
    let root = TempDir::new().unwrap();
    let database = format!("sqlite://{}", root.path().join("ledger.db").display());
    let config = root.path().join("config.toml");
    std::fs::write(
        &config,
        include_str!("../deploy/config.receive.toml.example")
            .replace(
                "sqlite:///var/lib/lightning-goats/lightning-goats.db",
                &database,
            )
            .replace("https://api.strike.me/", "https://127.0.0.1:9/")
            .replace("http://10.8.0.6:8789/", "http://127.0.0.1:9/"),
    )
    .unwrap();
    let run = |extra: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_lightning-goatsctl"))
            .env_clear()
            .env("TOKIO_WORKER_THREADS", "2")
            .arg("--config")
            .arg(&config)
            .arg("add-credit")
            .args([
                "--id",
                "operator-001",
                "--sats",
                "1000",
                "--reason",
                "Private operator note",
            ])
            .args(extra)
            .output()
            .unwrap()
    };
    let preview = run(&["--dry-run"]);
    assert!(
        preview.status.success(),
        "{}",
        String::from_utf8_lossy(&preview.stderr)
    );
    let value: Value = serde_json::from_slice(&preview.stdout).unwrap();
    assert_eq!(value["status"], "preview");
    let ledger = LedgerStore::connect(&database).await.unwrap();
    assert_eq!(ledger.feed_credit_sats().await.unwrap(), 0);
    let applied = run(&[]);
    assert!(
        applied.status.success(),
        "{}",
        String::from_utf8_lossy(&applied.stderr)
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&applied.stdout).unwrap()["status"],
        "granted"
    );
    let duplicate = run(&[]);
    assert!(duplicate.status.success());
    assert_eq!(
        serde_json::from_slice::<Value>(&duplicate.stdout).unwrap()["status"],
        "duplicate"
    );
    assert!(!run(&["--address", "dexter"]).status.success());
    assert!(!run(&["--address", "unknown"]).status.success());
    assert_eq!(ledger.feed_credit_sats().await.unwrap(), 1000);
    let events = ledger.events_after(0, 10).await.unwrap();
    assert_eq!(events.len(), 1);
    assert!(!events[0].payload_json.contains("Private operator note"));
}
