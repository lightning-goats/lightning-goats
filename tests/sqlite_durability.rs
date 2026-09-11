use std::process::Command;

use lightning_goats::{
    config::AppConfig,
    domain::payment::SettledPayment,
    gateway::GatewayServerConfig,
    ledger::{LedgerStore, SettlementOutcome},
};
use tempfile::TempDir;

const UNSAFE_URLS: &[&str] = &[
    "sqlite::memory:",
    "sqlite://:memory:",
    "sqlite://%3Amemory%3A",
    "sqlite://",
    "sqlite://?cache=private",
    "sqlite://volatile?mode=memory",
    "sqlite://volatile?%6Dode=%6Demory",
    "sqlite://volatile?mode=memory&mode=rw",
    "sqlite://sqlite::memory:",
    "sqlite://file::memory:",
    "sqlite://file:volatile%3Fmode=memory",
    "sqlite://file:%253Amemory%253A",
    "sqlite:///volatile?vfs=memdb",
    "sqlite:///volatile?%76fs=mem%64b",
    "sqlite://file:/volatile%3Fvfs=memdb",
];

fn config(url: &str) -> String {
    include_str!("../deploy/config.canary.toml.example").replace(
        "sqlite:///var/lib/lightning-goats/lightning-goats-canary.db",
        url,
    )
}

#[test]
fn both_configs_reject_volatile_sqlite_representations() {
    let mut accepted = Vec::new();
    for url in UNSAFE_URLS {
        let daemon: AppConfig = toml::from_str(&config(url)).unwrap();
        if daemon.validate().is_ok() {
            accepted.push(format!("daemon: {url}"));
        }
        let mut gateway: GatewayServerConfig =
            toml::from_str(include_str!("../deploy/gateway/config.canary.toml.example")).unwrap();
        gateway.database.url = (*url).to_owned();
        if gateway.validate().is_ok() {
            accepted.push(format!("gateway: {url}"));
        }
    }
    assert!(
        accepted.is_empty(),
        "accepted volatile config: {accepted:?}"
    );
}

#[tokio::test]
async fn direct_ledger_cannot_bypass_config_durability() {
    let mut accepted = Vec::new();
    for url in UNSAFE_URLS {
        if LedgerStore::connect(url).await.is_ok() {
            accepted.push(*url);
        }
    }
    assert!(
        accepted.is_empty(),
        "accepted volatile ledger: {accepted:?}"
    );
}

#[test]
fn real_operator_cli_rejects_volatile_storage() {
    let directory = TempDir::new().unwrap();
    let path = directory.path().join("config.toml");
    for url in ["sqlite://:memory:", "sqlite://file:/volatile%3Fvfs=memdb"] {
        std::fs::write(&path, config(url)).unwrap();
        for command in ["status", "reset-overlay-stream"] {
            let result = Command::new(env!("CARGO_BIN_EXE_lightning-goatsctl"))
                .env_clear()
                .env("TOKIO_WORKER_THREADS", "2")
                .args(["--config", path.to_str().unwrap(), command])
                .output()
                .unwrap();
            assert!(!result.status.success(), "accepted {url} {command}");
            assert!(result.stdout.is_empty());
            assert!(String::from_utf8_lossy(&result.stderr).contains("SQLite"));
        }
    }
}

#[tokio::test]
async fn ordinary_file_preserves_credit_deduplication_events_and_identity_in_another_process() {
    let directory = TempDir::new().unwrap();
    let url = format!("sqlite://{}/ledger.db", directory.path().display());
    let store = LedgerStore::connect(&url).await.unwrap();
    let payment = SettledPayment {
        source: "strike".to_owned(),
        source_id: "synthetic-durability-receipt".to_owned(),
        payment_hash: Some("35".repeat(32)),
        address_user: "herd".to_owned(),
        credit_pool: "herd".to_owned(),
        amount_msat: 2_340_000,
        settled_at: Some(1_700_000_000),
        context_json: None,
    };
    store.record_payment(&payment).await.unwrap();
    let identity = store.overlay_stream_id().await.unwrap();
    drop(store);
    let path = directory.path().join("config.toml");
    std::fs::write(&path, config(&url)).unwrap();
    let status = Command::new(env!("CARGO_BIN_EXE_lightning-goatsctl"))
        .env_clear()
        .env("TOKIO_WORKER_THREADS", "2")
        .args(["--config", path.to_str().unwrap(), "status"])
        .output()
        .unwrap();
    assert!(status.status.success(), "{:?}", status.stderr);
    assert!(String::from_utf8_lossy(&status.stdout).contains("feed_credit_sats=2340"));
    let reopened = LedgerStore::connect(&url).await.unwrap();
    assert_eq!(reopened.overlay_stream_id().await.unwrap(), identity);
    assert_eq!(
        reopened.record_payment(&payment).await.unwrap(),
        SettlementOutcome::Duplicate
    );
    assert_eq!(reopened.feed_credit_sats().await.unwrap(), 2340);
    assert_eq!(reopened.events_after(0, 10).await.unwrap().len(), 1);
}
