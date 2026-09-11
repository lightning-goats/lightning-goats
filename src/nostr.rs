use std::{ffi::OsStr, path::PathBuf, process::Stdio, sync::Arc, time::Duration};

use anyhow::{Context, Result, bail};
use reqwest::Url;
use serde::{Deserialize, Serialize};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWriteExt},
    process::Command,
    time::timeout,
};
use zeroize::{Zeroize, Zeroizing};

use crate::{config::NostrConfig, secrets::read_systemd_credential};

const NAK_TIMEOUT: Duration = Duration::from_secs(45);
const NAK_CLEANUP_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_NAK_INPUT: usize = 64 * 1024;
const MAX_NAK_STDOUT: usize = 64 * 1024;
const MAX_NAK_STDERR: usize = 8 * 1024;
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
            "tags": &tags,
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
        // Do not attach a deserializer error: malformed field values can contain
        // subprocess credentials. Keep all untrusted output out of error logs.
        let event: SignedNostrEvent = serde_json::from_str(output.trim())
            .map_err(|_| anyhow::anyhow!("nak returned invalid signed Nostr event JSON"))?;
        validate_signed_event(&event, &self.project_pubkey)?;
        if event.content != content || event.tags != tags {
            bail!("signed Nostr event does not match requested content or tags");
        }
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
        let output = self
            .run_nak(args.iter().map(String::as_str), &event_json, None)
            .await
            .context("nak failed publishing persisted signed Nostr event")?;
        let echoed: SignedNostrEvent = serde_json::from_str(output.trim())
            .map_err(|_| anyhow::anyhow!("nak returned invalid publication event JSON"))?;
        if &echoed != event {
            bail!("nak publication returned a different persisted event");
        }
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
        self.run_nak_with_timeout(args, stdin_text, signer, NAK_TIMEOUT)
            .await
    }

    async fn run_nak_with_timeout<I, S>(
        &self,
        args: I,
        stdin_text: &str,
        signer: Option<(&str, &str)>,
        deadline: Duration,
    ) -> Result<String>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        if stdin_text.len() > MAX_NAK_INPUT {
            bail!("nak input exceeds {MAX_NAK_INPUT} bytes");
        }
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
        let stdout = child.stdout.take().context("failed opening nak stdout")?;
        let stderr = child.stderr.take().context("failed opening nak stderr")?;

        // Drain both pipes concurrently with stdin, under the same deadline.
        // Writing stdin first can deadlock if the child fills a pipe before
        // reading. Waiting with output would allocate unbounded child output.
        let execution = timeout(deadline, async {
            tokio::try_join!(
                async {
                    stdin
                        .write_all(stdin_text.as_bytes())
                        .await
                        .context("failed writing event to nak stdin")?;
                    stdin
                        .write_all(b"\n")
                        .await
                        .context("failed terminating nak stdin event")?;
                    drop(stdin);
                    Ok::<(), anyhow::Error>(())
                },
                read_bounded(stdout, MAX_NAK_STDOUT, "stdout"),
                async {
                    // stderr may contain credentials; bound and erase it, but
                    // never incorporate it into logs or durable outbox errors.
                    let _stderr =
                        Zeroizing::new(read_bounded(stderr, MAX_NAK_STDERR, "stderr").await?);
                    Ok::<(), anyhow::Error>(())
                },
                async { child.wait().await.context("failed waiting for nak") },
            )
        })
        .await;

        let ((), stdout, (), status) = match execution {
            Ok(Ok(output)) => output,
            failure => {
                // Retain the Child handle until kill/reap, including failures
                // while writing stdin. kill_on_drop remains a cancellation
                // backstop; systemd owns cleanup of the whole service cgroup.
                if !matches!(timeout(NAK_CLEANUP_TIMEOUT, child.kill()).await, Ok(Ok(()))) {
                    tracing::warn!("nak child cleanup did not complete within its bound");
                }
                return match failure {
                    Ok(Err(error)) => Err(error),
                    Err(_) => Err(anyhow::anyhow!("nak operation timed out")),
                    Ok(Ok(_)) => unreachable!(),
                };
            }
        };
        if !status.success() {
            bail!("nak exited with {status}; subprocess stderr withheld");
        }
        String::from_utf8(stdout).context("nak stdout was not UTF-8")
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

async fn read_bounded<R: AsyncRead + Unpin>(
    reader: R,
    limit: usize,
    stream: &str,
) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    reader
        .take((limit + 1) as u64)
        .read_to_end(&mut bytes)
        .await
        .with_context(|| format!("failed reading nak {stream}"))?;
    if bytes.len() > limit {
        bytes.zeroize();
        bail!("nak {stream} output exceeds {limit} bytes");
    }
    Ok(bytes)
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
    #[cfg(unix)]
    #[tokio::test]
    async fn timeout_includes_stdin_and_reaps_stalled_child() {
        use std::{fs, os::unix::fs::PermissionsExt};

        let directory = tempfile::TempDir::new().unwrap();
        let script = directory.path().join("nak-stub");
        fs::write(&script, include_str!("../tests/fixtures/nak_stub.py")).unwrap();
        fs::set_permissions(&script, fs::Permissions::from_mode(0o700)).unwrap();
        fs::write(directory.path().join("mode"), "stall_before_stdin").unwrap();
        let mut config = config();
        config.nak_path = script;
        config.nak_config_path = directory.path().to_owned();
        let client = NakClient::new(&config, "synthetic-key".to_owned()).unwrap();
        let input = "x".repeat(MAX_NAK_INPUT);
        let error = timeout(
            Duration::from_secs(3),
            client.run_nak_with_timeout(["event"], &input, None, Duration::from_millis(300)),
        )
        .await
        .expect("stdin, output and exit must share one deadline")
        .unwrap_err();
        assert!(format!("{error:#}").contains("timed out"));
        let pid = fs::read_to_string(directory.path().join("pid")).unwrap();
        #[cfg(target_os = "linux")]
        assert!(!std::path::Path::new(&format!("/proc/{pid}")).exists());
    }

    #[tokio::test]
    async fn input_limit_rejects_before_spawn() {
        let mut config = config();
        config.nak_path = PathBuf::from("/definitely-missing-nak-binary");
        let client = NakClient::new(&config, "synthetic-key".to_owned()).unwrap();
        let error = client
            .run_nak(["event"], &"x".repeat(MAX_NAK_INPUT + 1), None)
            .await
            .unwrap_err();
        assert!(format!("{error:#}").contains("input exceeds"));
    }

    #[tokio::test]
    async fn output_limit_accepts_exact_boundary_and_rejects_one_more_byte() {
        let accepted = vec![b'x'; 16];
        assert_eq!(
            read_bounded(accepted.as_slice(), 16, "stdout")
                .await
                .unwrap(),
            accepted
        );
        assert!(
            read_bounded([b'x'; 17].as_slice(), 16, "stderr")
                .await
                .is_err()
        );
    }
}
