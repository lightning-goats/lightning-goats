#![cfg(unix)]
use lightning_goats::{
    config::NostrConfig,
    nostr::{NakClient, SignedNostrEvent},
};
use serde_json::Value;
use std::{fs, os::unix::fs::PermissionsExt};
use tempfile::TempDir;
const RECIPIENT: &str = "abababababababababababababababababababababababababababababababab";
struct Fixture {
    root: TempDir,
    client: NakClient,
}
impl Fixture {
    fn new(mode: &str) -> Self {
        let root = TempDir::new().unwrap();
        let executable = root.path().join("nak");
        fs::write(&executable, include_str!("fixtures/private_nak_stub.py")).unwrap();
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
        fs::write(root.path().join("mode"), mode).unwrap();
        let client = NakClient::new(
            &NostrConfig {
                nak_path: executable,
                nak_config_path: root.path().into(),
                bunker_pubkey: "ab".repeat(32),
                relays: vec!["wss://announcement.invalid".into()],
            },
            "synthetic-key".into(),
        )
        .unwrap();
        Self { root, client }
    }
    fn calls(&self) -> Vec<Value> {
        fs::read_to_string(self.root.path().join("calls.jsonl"))
            .unwrap_or_default()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect()
    }
}
#[tokio::test]
async fn ciphertext_retry_uses_exact_bytes_and_no_signer_or_announcement_relays() {
    let f = Fixture::new("success");
    let wrap = f
        .client
        .wrap_private_message(RECIPIENT, "synthetic private alert")
        .await
        .unwrap();
    let raw = wrap.ciphertext_json().to_owned();
    assert!(!raw.contains("synthetic private alert"));
    let public: SignedNostrEvent = serde_json::from_str(&raw).unwrap();
    assert!(f.client.publish_signed(&public).await.is_err());
    fs::write(f.root.path().join("mode"), "publish_failure").unwrap();
    assert!(
        f.client
            .publish_private_wrap(RECIPIENT, &["wss://inbox.invalid".into()], &wrap)
            .await
            .is_err()
    );
    fs::write(f.root.path().join("mode"), "success").unwrap();
    let restored = f
        .client
        .restore_private_wrap(RECIPIENT, &raw)
        .await
        .unwrap();
    assert_eq!(wrap.event_id(), restored.event_id());
    f.client
        .publish_private_wrap(RECIPIENT, &["wss://inbox.invalid".into()], &restored)
        .await
        .unwrap();
    let calls = f.calls();
    assert_eq!(calls.iter().filter(|c| c["phase"] == "wrap").count(), 1);
    for call in &calls {
        let signing = call["phase"] == "wrap";
        assert_eq!(call["client_key"], signing);
        assert_eq!(call["signer"], signing);
        if call["phase"] == "publish" {
            assert_eq!(call["raw"], raw);
            assert_eq!(
                call["args"].as_array().unwrap().last().unwrap(),
                "wss://inbox.invalid"
            );
        }
    }
}
#[tokio::test]
async fn missing_encryption_and_invalid_wrapper_never_reach_publication() {
    for mode in [
        "no_encryption",
        "wrong_recipient",
        "public_kind",
        "plaintext",
        "extra_field",
        "malformed",
        "bad_signature",
    ] {
        let f = Fixture::new(mode);
        let error = f
            .client
            .wrap_private_message(RECIPIENT, "synthetic private alert")
            .await
            .err()
            .expect("invalid wrap accepted");
        assert!(!format!("{error:#}").contains("SENSITIVE-PRIVATE-ERROR"));
        assert!(f.calls().iter().all(|c| c["phase"] != "publish"));
    }
}
#[tokio::test]
async fn missing_inbox_or_invalid_inputs_fail_before_subprocess() {
    let f = Fixture::new("success");
    for recipient in ["", "not-a-public-key"] {
        assert!(
            f.client
                .wrap_private_message(recipient, "synthetic")
                .await
                .is_err()
        );
    }
    assert!(
        f.client
            .wrap_private_message(RECIPIENT, &"x".repeat(4097))
            .await
            .is_err()
    );
    assert!(f.calls().is_empty());
    let wrap = f
        .client
        .wrap_private_message(RECIPIENT, "synthetic")
        .await
        .unwrap();
    let before = f.calls().len();
    for relays in [
        vec![],
        vec!["http://inbox.invalid".into()],
        vec!["ws://inbox.invalid".into()],
        vec!["wss://user:secret@inbox.invalid".into()],
    ] {
        assert!(
            f.client
                .publish_private_wrap(RECIPIENT, &relays, &wrap)
                .await
                .is_err()
        );
    }
    assert_eq!(f.calls().len(), before);
}
#[tokio::test]
async fn changed_publication_echo_is_rejected() {
    let f = Fixture::new("mutated_publish");
    let wrap = f
        .client
        .wrap_private_message(RECIPIENT, "synthetic")
        .await
        .unwrap();
    assert!(
        f.client
            .publish_private_wrap(RECIPIENT, &["wss://inbox.invalid".into()], &wrap)
            .await
            .is_err()
    );
}
