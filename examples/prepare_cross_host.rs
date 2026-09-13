//! Prepare synthetic daemon state only. Never starts a daemon or contacts a peer.
#![forbid(unsafe_code)]

use anyhow::{Context, Result, bail, ensure};
use lightning_goats::{
    config::{AppConfig, RuntimeMode},
    domain::payment::SettledPayment,
    ledger::{LedgerStore, SettlementOutcome},
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs::{DirBuilder, File, OpenOptions},
    io::Write,
    os::unix::fs::{DirBuilderExt, OpenOptionsExt},
    path::Path,
};
use uuid::Uuid;

fn exclusive_file(path: &Path, data: &[u8]) -> Result<()> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)?;
    file.write_all(data)?;
    file.sync_all()?;
    Ok(())
}

async fn prepare(root: &Path, source: &str) -> Result<Value> {
    ensure!(
        source.len() == 40 && source.bytes().all(|b| b.is_ascii_hexdigit()),
        "a full declared source commit is required"
    );
    ensure!(root.is_absolute(), "use a new absolute directory");
    let parent = root.parent().context("directory needs a parent")?;
    ensure!(
        parent.canonicalize()? == parent,
        "directory parent must be canonical (no symlinks or traversal)"
    );
    let root_text = root.to_str().context("directory must be UTF-8")?;
    ensure!(
        !root_text
            .chars()
            .any(|c| c.is_control() || "?#%".contains(c)),
        "directory cannot contain SQLite URL delimiters"
    );
    let database = format!("sqlite://{root_text}/daemon.db");
    let mut config: toml::Value =
        toml::from_str(include_str!("../deploy/config.canary.toml.example"))?;
    config["database"]["url"] = database.clone().into();
    // No real provider, even if a caller later starts this prepared canary.
    config["strike"]["api_url"] = "https://127.0.0.1:9/".into();
    config["nostr"]["nak_config_path"] = format!("{root_text}/unused-nak").into();
    let config_text = toml::to_string_pretty(&config)?;
    let parsed: AppConfig = toml::from_str(&config_text)?;
    parsed.validate()?;
    ensure!(
        parsed.service.mode == RuntimeMode::Canary,
        "canary required"
    );
    ensure!(
        parsed.feeder.threshold_sats == 1000,
        "shipped threshold changed"
    );
    ensure!(
        parsed.feeder.inter_feed_delay_seconds == 5,
        "shipped delay changed"
    );

    // Atomic creation refuses existing stores and dangling destination symlinks.
    // On any later failure retain partial evidence; never auto-delete or retry it.
    DirBuilder::new().mode(0o700).create(root)?;
    let credentials = root.join("synthetic-credentials");
    DirBuilder::new().mode(0o700).create(&credentials)?;
    for name in ["strike-api-key", "strike-webhook-secret"] {
        exclusive_file(&credentials.join(name), b"synthetic-cross-host-only\n")?;
    }
    exclusive_file(&root.join("daemon.toml"), config_text.as_bytes())?;

    let run_id = Uuid::new_v4();
    let payment = SettledPayment {
        source: "synthetic-cross-host".into(),
        source_id: run_id.to_string(),
        payment_hash: None,
        address_user: "herd".into(),
        credit_pool: "herd".into(),
        amount_msat: 2_340_000,
        settled_at: None,
        context_json: Some(
            json!({"scope":"synthetic ledger seed; not Strike settlement"}).to_string(),
        ),
    };
    let ledger = LedgerStore::connect(&database).await?;
    ensure!(
        ledger.record_payment(&payment).await?
            == SettlementOutcome::Credited {
                sats: 2340,
                address_user: "herd".into(),
                credit_pool: "herd".into(),
            },
        "synthetic credit was not recorded"
    );
    drop(ledger);
    let reopened = LedgerStore::connect(&database).await?;
    ensure!(
        reopened.record_payment(&payment).await? == SettlementOutcome::Duplicate,
        "duplicate seed was credited"
    );
    let events = reopened.events_after(0, 10).await?;
    ensure!(events.len() == 1 && events[0].event_type == "payment_received");
    ensure!(reopened.feed_credit_sats().await? == 2340);
    ensure!(reopened.unresolved_feed_attempt().await?.is_none());
    ensure!(reopened.next_outbox_entry().await?.is_none());
    let evidence = json!({
        "schema": 1,
        "scope": "inactive synthetic daemon preparation; no network or provider settlement",
        "run_id": run_id,
        "declared_source_commit": source,
        "config_sha256": hex::encode(Sha256::digest(config_text.as_bytes())),
        "mode": "canary",
        "seed_sats": 2340,
        "threshold_sats": parsed.feeder.threshold_sats,
        "inter_feed_delay_seconds": parsed.feeder.inter_feed_delay_seconds,
        "duplicate_seed": "not credited",
        "payment_received_events": 1,
        "unresolved_feed_attempts": 0,
        "nostr_outbox_entries": 0,
        "started": false,
        "activation_gates": ["verified release provenance and runtime ownership",
            "acknowledged harmless HOME fixture and command-count evidence",
            "approved authenticated network and harmless-test manifest"],
        "not_proven": ["Strike/provider settlement", "home command delivery",
            "two confirmed feeds and 340 remainder", "production readiness"]
    });
    exclusive_file(
        &root.join("PREPARED.json"),
        &serde_json::to_vec_pretty(&evidence)?,
    )?;
    File::open(&credentials)?.sync_all()?;
    File::open(root)?.sync_all()?;
    File::open(parent)?.sync_all()?;
    Ok(evidence)
}

#[tokio::main]
async fn main() -> Result<()> {
    let args: Vec<_> = std::env::args_os().collect();
    if args.len() != 3 {
        bail!("usage: prepare_cross_host NEW_ABSOLUTE_DIRECTORY DECLARED_SOURCE_COMMIT");
    }
    let source = args[2].to_str().context("source must be UTF-8")?;
    println!("{}", prepare(Path::new(&args[1]), source).await?);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::os::unix::fs::{PermissionsExt, symlink};

    #[tokio::test]
    async fn fresh_synthetic_state_preserves_atomic_credit_and_refuses_reuse() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().canonicalize().unwrap().join("session");
        let evidence = prepare(&root, &"a".repeat(40)).await.unwrap();
        assert_eq!(evidence["seed_sats"], 2340);
        assert_eq!(evidence["started"], false);
        assert_eq!(
            fs::metadata(&root).unwrap().permissions().mode() & 0o777,
            0o700
        );
        let before = fs::read(root.join("PREPARED.json")).unwrap();
        assert!(prepare(&root, &"a".repeat(40)).await.is_err());
        assert_eq!(before, fs::read(root.join("PREPARED.json")).unwrap());
        let cfg = AppConfig::load(&root.join("daemon.toml")).unwrap();
        assert_eq!(cfg.strike.api_url, "https://127.0.0.1:9/");
        assert_eq!(cfg.service.mode, RuntimeMode::Canary);
        assert_eq!(
            fs::metadata(root.join("synthetic-credentials/strike-api-key"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }

    #[tokio::test]
    async fn rejects_path_aliases_and_url_delimiters_before_writing() {
        let directory = tempfile::tempdir().unwrap();
        let parent = directory.path().canonicalize().unwrap();
        for name in ["bad?mode=ro", "bad#fragment", "bad%2fpath"] {
            let path = parent.join(name);
            assert!(prepare(&path, &"a".repeat(40)).await.is_err());
            assert!(!path.exists());
        }
        let target = parent.join("untouched");
        fs::create_dir(&target).unwrap();
        symlink(&target, parent.join("alias")).unwrap();
        assert!(
            prepare(&parent.join("alias/new"), &"a".repeat(40))
                .await
                .is_err()
        );
        assert!(fs::read_dir(&target).unwrap().next().is_none());
        symlink(parent.join("missing"), parent.join("dangling")).unwrap();
        assert!(
            prepare(&parent.join("dangling"), &"a".repeat(40))
                .await
                .is_err()
        );
        assert!(!parent.join("missing").exists());
    }
}
