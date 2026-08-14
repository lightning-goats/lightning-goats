use std::{ffi::OsStr, path::PathBuf, process::Stdio, sync::Arc, time::Duration};

use anyhow::{Context, Result, bail};
use reqwest::Url;
use serde::{Deserialize, Serialize};
use tokio::{io::AsyncWriteExt, process::Command, time::timeout};
use zeroize::Zeroizing;

use crate::{config::NostrConfig, secrets::read_systemd_credential};

const NAK_TIMEOUT: Duration = Duration::from_secs(45);
const PUBLISH_DUMMY_SECRET: &str = "01";

#[derive(Clone)]
pub struct NakClient {
    nak_path: PathBuf,
    nak_config_path: PathBuf,
    bunker_uri: String,
    project_pubkey: String,
    client_key: Arc<Zeroizing<String>>,
    relays: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SignedNostrEvent {
    pub id: String,
    pub pubkey: String,
    pub created_at: u64,
    pub kind: u64,
    pub tags: Vec<Vec<String>>,
    pub content: String,
    pub sig: String,
}

impl NakClient {
    pub async fn from_config(config: &NostrConfig) -> Result<Self> {
        let client_key = read_systemd_credential("nostr-client-key").await?;
        Self::new(config, client_key)
    }

    pub fn new(config: &NostrConfig, client_key: String) -> Result<Self> {
        if client_key.trim().is_empty() {
            bail!("NIP-46 client key is empty");
        }
        let bunker_uri = build_bunker_uri(&config.bunker_pubkey, &config.relays)?;
        Ok(Self {
            nak_path: config.nak_path.clone(),
            nak_config_path: config.nak_config_path.clone(),
            bunker_uri,
            project_pubkey: config.bunker_pubkey.to_ascii_lowercase(),
            client_key: Arc::new(Zeroizing::new(client_key)),
            relays: config.relays.clone(),
        })
    }

    pub async fn sign_kind1(
        &self,
        content: &str,
        tags: Vec<Vec<String>>,
    ) -> Result<SignedNostrEvent> {
        tokio::fs::create_dir_all(&self.nak_config_path)
            .await
            .with_context(|| {
                format!(
                    "failed creating nak runtime config directory {}",
                    self.nak_config_path.display()
                )
            })?;

        let partial = serde_json::to_string(&serde_json::json!({
            "kind": 1,
            "content": content,
            "tags": tags,
        }))
        .context("failed serializing partial Nostr event")?;

        let output = self
            .run_nak(
                [OsStr::new("event")],
                &partial,
                Some((&self.bunker_uri, self.client_key.as_str())),
            )
            .await
            .context("nak failed signing Nostr event through NIP-46")?;
        let event: SignedNostrEvent = serde_json::from_str(output.trim())
            .context("nak returned invalid signed Nostr event JSON")?;
        validate_signed_event(&event, &self.project_pubkey)?;
        self.verify_event(&event).await?;
        Ok(event)
    }

    pub async fn publish_signed(&self, event: &SignedNostrEvent) -> Result<()> {
        validate_signed_event(event, &self.project_pubkey)?;
        self.verify_event(event).await?;

        let event_json =
            serde_json::to_string(event).context("failed serializing signed Nostr event")?;
        let mut args = vec![
            "event".to_owned(),
            "--sec".to_owned(),
            PUBLISH_DUMMY_SECRET.to_owned(),
        ];
        args.extend(self.relays.iter().cloned());
        self.run_nak(args.iter().map(AsRef::as_ref), &event_json, None)
            .await
            .context("nak failed publishing persisted signed Nostr event")?;
        Ok(())
    }

    pub async fn verify_event(&self, event: &SignedNostrEvent) -> Result<()> {
        validate_signed_event(event, &self.project_pubkey)?;
        let event_json =
            serde_json::to_string(event).context("failed serializing signed Nostr event")?;
        self.run_nak([OsStr::new("verify")], &event_json, None)
            .await
            .context("nak rejected persisted Nostr event signature")?;
        Ok(())
    }

    async fn run_nak<I, S>(
        &self,
        args: I,
        stdin_text: &str,
        signer: Option<(&str, &str)>,
    ) -> Result<String>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let mut command = Command::new(&self.nak_path);
        command
            .arg("--config-path")
            .arg(&self.nak_config_path)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .env_clear()
            .env("NO_COLOR", "1");

        if let Some((bunker_uri, client_key)) = signer {
            command
                .env("NOSTR_SECRET_KEY", bunker_uri)
                .env("NOSTR_CLIENT_KEY", client_key);
        }

        let mut child = command
            .spawn()
            .with_context(|| format!("failed spawning {}", self.nak_path.display()))?;
        let mut stdin = child.stdin.take().context("failed opening nak stdin")?;
        stdin
            .write_all(stdin_text.as_bytes())
            .await
            .context("failed writing event to nak stdin")?;
        stdin
            .write_all(b"\n")
            .await
            .context("failed terminating nak stdin event")?;
        drop(stdin);

        let output = timeout(NAK_TIMEOUT, child.wait_with_output())
            .await
            .context("nak operation timed out")?
            .context("failed waiting for nak")?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!(
                "nak exited with {}: {}",
                output.status,
                sanitize_stderr(&stderr)
            );
        }
        String::from_utf8(output.stdout).context("nak stdout was not UTF-8")
    }
}

fn build_bunker_uri(pubkey: &str, relays: &[String]) -> Result<String> {
    let mut url = Url::parse(&format!("bunker://{pubkey}"))
        .context("failed constructing NIP-46 bunker URL")?;
    {
        let mut query = url.query_pairs_mut();
        for relay in relays {
            query.append_pair("relay", relay);
        }
    }
    Ok(url.to_string())
}

fn validate_signed_event(event: &SignedNostrEvent, expected_pubkey: &str) -> Result<()> {
    if event.pubkey != expected_pubkey {
        bail!("signed Nostr event pubkey does not match configured Lightning Goats identity");
    }
    validate_hex(&event.id, 64, "Nostr event id")?;
    validate_hex(&event.pubkey, 64, "Nostr pubkey")?;
    validate_hex(&event.sig, 128, "Nostr signature")?;
    if event.created_at == 0 {
        bail!("signed Nostr event has zero created_at");
    }
    if event.kind != 1 {
        bail!("unexpected signed Nostr event kind {}", event.kind);
    }
    Ok(())
}

fn validate_hex(value: &str, expected_len: usize, field: &str) -> Result<()> {
    if value.len() != expected_len || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        bail!("{field} is not valid hexadecimal of length {expected_len}");
    }
    Ok(())
}

fn sanitize_stderr(stderr: &str) -> String {
    stderr
        .lines()
        .take(8)
        .map(|line| line.chars().take(240).collect::<String>())
        .collect::<Vec<_>>()
        .join(" | ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> NostrConfig {
        NostrConfig {
            nak_path: PathBuf::from("/usr/local/bin/nak"),
            nak_config_path: PathBuf::from("/run/lightning-goats/nak"),
            bunker_pubkey: "ab".repeat(32),
            relays: vec![
                "wss://relay-one.example".to_owned(),
                "wss://relay-two.example".to_owned(),
            ],
        }
    }

    #[test]
    fn bunker_uri_contains_each_relay_without_signer_secret() {
        let uri = build_bunker_uri(&config().bunker_pubkey, &config().relays).unwrap();
        let parsed = Url::parse(&uri).unwrap();
        assert_eq!(parsed.scheme(), "bunker");
        assert_eq!(parsed.host_str(), Some(config().bunker_pubkey.as_str()));
        let relays: Vec<_> = parsed
            .query_pairs()
            .filter(|(key, _)| key == "relay")
            .map(|(_, value)| value.into_owned())
            .collect();
        assert_eq!(relays, config().relays);
        assert!(!uri.contains("secret="));
    }

    #[test]
    fn validation_requires_expected_project_identity() {
        let event = SignedNostrEvent {
            id: "01".repeat(32),
            pubkey: "ab".repeat(32),
            created_at: 1,
            kind: 1,
            tags: vec![],
            content: "hello".to_owned(),
            sig: "02".repeat(64),
        };
        validate_signed_event(&event, &"ab".repeat(32)).unwrap();
        assert!(validate_signed_event(&event, &"cd".repeat(32)).is_err());
    }
}
