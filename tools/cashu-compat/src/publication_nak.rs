//! Bounded nak/NIP-46 adapter. Only a distinct client credential enters this process.
use crate::{publication::Unsigned, publication_broker::Backend};
use anyhow::{Result, ensure};
use async_trait::async_trait;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{io::Read, path::PathBuf, process::Stdio, time::Duration};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWriteExt},
    process::Command,
};
use zeroize::Zeroizing;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub binary: PathBuf,
    pub binary_sha256: String,
    pub private_directory: PathBuf,
    pub owner: String,
    pub client_pubkey: String,
    pub relays: Vec<String>,
    pub deadline_seconds: u64,
}
impl Config {
    pub fn validate(&self) -> Result<()> {
        self.validate_identity()?;
        for relay in &self.relays {
            crate::relay::validate_relay(relay)?;
        }
        Ok(())
    }
    fn validate_identity(&self) -> Result<()> {
        lightning_goats_nostr_event::validate_public_key(&self.owner)?;
        lightning_goats_nostr_event::validate_public_key(&self.client_pubkey)?;
        lightning_goats_nostr_event::validate_hex(&self.binary_sha256, 32)?;
        ensure!(
            self.client_pubkey != self.owner
                && !self.relays.is_empty()
                && self.relays.len() <= 8
                && (1..=45).contains(&self.deadline_seconds),
            "invalid Cashu NIP-46 client policy"
        );
        for path in [&self.binary, &self.private_directory] {
            ensure!(
                path.is_absolute()
                    && path != std::path::Path::new("/")
                    && path.components().all(|c| matches!(
                        c,
                        std::path::Component::RootDir | std::path::Component::Normal(_)
                    )),
                "invalid Cashu signing helper path"
            );
        }
        let unique: std::collections::BTreeSet<_> = self.relays.iter().collect();
        ensure!(
            unique.len() == self.relays.len(),
            "duplicate Cashu signing relay"
        );
        Ok(())
    }
    fn runtime_files(&self) -> Result<()> {
        crate::private_ipc::protected_config(&self.binary)?;
        self.digest_and_state()
    }
    /// Qualify local files and credential binding without retaining a client.
    pub(crate) fn verify_local_client(&self, client_key: &str) -> Result<()> {
        self.validate()?;
        self.runtime_files()?;
        self.verify_client_identity(client_key)
    }

    fn verify_client_identity(&self, client_key: &str) -> Result<()> {
        ensure!(client_key.len() == 64, "invalid NIP-46 client credential");
        let bytes = Zeroizing::new(
            hex::decode(client_key)
                .map_err(|_| anyhow::anyhow!("invalid NIP-46 client credential"))?,
        );
        let mut key = cdk::secp256k1::SecretKey::from_slice(&bytes)
            .map_err(|_| anyhow::anyhow!("invalid NIP-46 client credential"))?;
        let public =
            cdk::secp256k1::PublicKey::from_secret_key(&cdk::secp256k1::Secp256k1::new(), &key)
                .x_only_public_key()
                .0
                .to_string();
        key.non_secure_erase();
        ensure!(
            public == self.client_pubkey && public != self.owner,
            "NIP-46 credential does not match separate client identity"
        );
        Ok(())
    }
    fn digest_and_state(&self) -> Result<()> {
        crate::private_ipc::private_state(&self.private_directory)?;
        let mut file = std::fs::File::open(&self.binary)?;
        let metadata = file.metadata()?;
        ensure!(
            metadata.len() > 0 && metadata.len() <= 256 * 1024 * 1024,
            "signing helper size exceeds bound"
        );
        let mut hash = Sha256::new();
        let mut buffer = [0u8; 8192];
        loop {
            let n = file.read(&mut buffer)?;
            if n == 0 {
                break;
            }
            hash.update(&buffer[..n]);
        }
        ensure!(
            hex::encode(hash.finalize()) == self.binary_sha256,
            "signing helper digest differs from approved artifact"
        );
        Ok(())
    }
}
pub struct Nak {
    config: Config,
    bunker: String,
    client_key: Zeroizing<String>,
    lock: tokio::sync::Mutex<()>,
}
impl Nak {
    pub fn new(config: Config, client_key: Zeroizing<String>) -> Result<Self> {
        config.validate()?;
        config.runtime_files()?;
        Self::assemble(config, client_key)
    }
    fn assemble(config: Config, client_key: Zeroizing<String>) -> Result<Self> {
        config.validate_identity()?;
        config.verify_client_identity(&client_key)?;
        let mut bunker = url::Url::parse(&format!("bunker://{}", config.owner))?;
        for relay in &config.relays {
            bunker.query_pairs_mut().append_pair("relay", relay);
        }
        Ok(Self {
            config,
            bunker: bunker.into(),
            client_key,
            lock: tokio::sync::Mutex::new(()),
        })
    }
    async fn run(&self, operation: Operation, input: &str, output_limit: usize) -> Result<String> {
        ensure!(input.len() <= 64_000, "Cashu helper input exceeds bound");
        let _guard = self.lock.lock().await;
        let mut command = Command::new(&self.config.binary);
        command
            .arg("--config-path")
            .arg(&self.config.private_directory);
        match operation {
            Operation::Encrypt => {
                command.args([
                    "encrypt",
                    "--stdin",
                    "--recipient-pubkey",
                    &self.config.owner,
                ]);
            }
            Operation::Decrypt => {
                command.args(["decrypt", "--stdin", "--sender-pubkey", &self.config.owner]);
            }
            Operation::Sign => {
                command.arg("event");
            }
        }
        command
            .env_clear()
            .env("NO_COLOR", "1")
            .env("NOSTR_SECRET_KEY", &self.bunker)
            .env("NOSTR_CLIENT_KEY", self.client_key.as_str())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        let mut child = command
            .spawn()
            .map_err(|_| anyhow::anyhow!("Cashu signing helper unavailable"))?;
        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| anyhow::anyhow!("Cashu helper pipe unavailable"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| anyhow::anyhow!("Cashu helper pipe unavailable"))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| anyhow::anyhow!("Cashu helper pipe unavailable"))?;
        let execution =
            tokio::time::timeout(Duration::from_secs(self.config.deadline_seconds), async {
                tokio::try_join!(
                    async {
                        stdin.write_all(input.as_bytes()).await?;
                        stdin.write_all(b"\n").await?;
                        drop(stdin);
                        Ok::<_, anyhow::Error>(())
                    },
                    bounded(stdout, output_limit),
                    bounded(stderr, 8192),
                    async { Ok::<_, anyhow::Error>(child.wait().await?) }
                )
            })
            .await;
        let (_, stdout, _stderr, status) = match execution {
            Ok(Ok(result)) => result,
            _ => {
                let _ = tokio::time::timeout(Duration::from_secs(5), child.kill()).await;
                anyhow::bail!("Cashu signing helper failed or exceeded deadline");
            }
        };
        ensure!(status.success(), "Cashu signing helper rejected operation");
        let text = std::str::from_utf8(&stdout)
            .map_err(|_| anyhow::anyhow!("invalid Cashu helper output"))?;
        Ok(text.to_owned())
    }
    fn target(&self, owner: &str) -> Result<()> {
        ensure!(
            owner == self.config.owner,
            "Cashu encryption target differs from owner"
        );
        Ok(())
    }
}
enum Operation {
    Encrypt,
    Decrypt,
    Sign,
}
async fn bounded(reader: impl AsyncRead + Unpin, limit: usize) -> Result<Zeroizing<Vec<u8>>> {
    let mut bytes = Zeroizing::new(Vec::new());
    reader
        .take(u64::try_from(limit)? + 1)
        .read_to_end(&mut bytes)
        .await?;
    ensure!(bytes.len() <= limit, "Cashu helper output exceeds bound");
    Ok(bytes)
}
#[async_trait]
impl Backend for Nak {
    async fn encrypt_self(&self, owner: &str, plaintext: &str) -> Result<String> {
        self.target(owner)?;
        ensure!(
            !plaintext.is_empty() && plaintext.len() <= 1024,
            "Cashu history plaintext exceeds bound"
        );
        let raw = self.run(Operation::Encrypt, plaintext, 1025).await?;
        let ciphertext = raw.strip_suffix('\n').unwrap_or(&raw).to_owned();
        crate::publication::validate_envelope(&ciphertext)?;
        Ok(ciphertext)
    }
    async fn decrypt_self(&self, owner: &str, ciphertext: &str) -> Result<String> {
        self.target(owner)?;
        crate::publication::validate_envelope(ciphertext)?;
        let raw = self.run(Operation::Decrypt, ciphertext, 1025).await?;
        let plaintext = raw.strip_suffix('\n').unwrap_or(&raw);
        ensure!(
            !plaintext.is_empty() && plaintext.len() <= 1024,
            "invalid decrypted history response"
        );
        Ok(plaintext.to_owned())
    }
    async fn sign(&self, unsigned: Unsigned) -> Result<String> {
        ensure!(
            unsigned.pubkey == self.config.owner && matches!(unsigned.kind, 10019 | 7376),
            "Cashu signing kind or owner not authorized"
        );
        let body = serde_json::to_string(&unsigned)?;
        let raw = self.run(Operation::Sign, &body, 65_536).await?;
        let verified = lightning_goats_nostr_event::VerifiedEvent::from_json(&raw)
            .map_err(|_| anyhow::anyhow!("Cashu signer returned an invalid event"))?;
        let event = verified.event();
        ensure!(
            event.pubkey == unsigned.pubkey
                && event.kind == unsigned.kind
                && event.created_at == unsigned.created_at
                && event.content == unsigned.content
                && event.tags == unsigned.tags,
            "Cashu signer changed intended event"
        );
        Ok(raw)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::publication::tests::{announcement, envelope, owner, sign};
    use std::{os::unix::fs::PermissionsExt, path::Path};
    fn client_key() -> Zeroizing<String> {
        Zeroizing::new(hex::encode([2; 32]))
    }
    fn client_pubkey() -> String {
        cdk::nuts::SecretKey::from_slice(&[2; 32])
            .unwrap()
            .public_key()
            .x_only_public_key()
            .to_string()
    }
    fn config(binary: PathBuf, directory: PathBuf) -> Config {
        Config {
            binary,
            binary_sha256: "ab".repeat(32),
            private_directory: directory,
            owner: owner(),
            client_pubkey: client_pubkey(),
            relays: vec!["wss://relay.example".into()],
            deadline_seconds: 1,
        }
    }
    fn stub(directory: &Path, mode: &str) -> Result<Nak> {
        let binary = directory.join("helper.py");
        std::fs::write(
            &binary,
            format!(
                r#"#!/usr/bin/python3
import json, os, pathlib, sys, time
root = pathlib.Path(sys.argv[2])
(root / 'pid').write_text(str(os.getpid()))
assert sys.argv[1] == '--config-path'
assert 'HOME' not in os.environ and 'CREDENTIALS_DIRECTORY' not in os.environ
assert 'HTTP_PROXY' not in os.environ and 'HTTPS_PROXY' not in os.environ
assert os.environ['NOSTR_SECRET_KEY'].startswith('bunker://')
assert os.environ['NOSTR_CLIENT_KEY'] not in ' '.join(sys.argv)
body = sys.stdin.read()
(root / 'observed.json').write_text(json.dumps({{'args':sys.argv[3:], 'body':body}}))
mode = '{mode}'
if mode == 'timeout': time.sleep(60)
if mode == 'stdout': sys.stdout.write('X' * 65537)
elif mode == 'stderr': sys.stderr.write('PRIVATE-DIAGNOSTIC' * 1024)
elif mode == 'failure':
    sys.stderr.write('PRIVATE-DIAGNOSTIC'); sys.exit(1)
elif mode == 'utf8': sys.stdout.buffer.write(bytes([255]))
else: sys.stdout.buffer.write((root / 'response').read_bytes())
"#
            ),
        )?;
        std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o700))?;
        // Test-only private constructor. Production rejects a user-owned helper.
        Nak::assemble(config(binary, directory.into()), client_key())
    }
    #[tokio::test]
    async fn fixed_commands_use_stdin_and_distinct_client_without_inherited_environment()
    -> Result<()> {
        let dir = crate::test_directory()?;
        let adapter = stub(dir.path(), "response")?;
        let plaintext = "[[\"direction\",\"in\"],[\"amount\",\"7\"],[\"unit\",\"sat\"]]";
        std::fs::write(dir.path().join("response"), format!("{}\n", envelope(9)))?;
        assert_eq!(
            adapter.encrypt_self(&owner(), plaintext).await?,
            envelope(9)
        );
        let observed: serde_json::Value =
            serde_json::from_slice(&std::fs::read(dir.path().join("observed.json"))?)?;
        assert_eq!(
            observed["args"],
            serde_json::json!(["encrypt", "--stdin", "--recipient-pubkey", owner()])
        );
        assert_eq!(observed["body"], format!("{plaintext}\n"));
        std::fs::write(dir.path().join("response"), format!("{plaintext}\n"))?;
        assert_eq!(
            adapter.decrypt_self(&owner(), &envelope(9)).await?,
            plaintext
        );
        let unsigned = announcement().unsigned(None)?;
        let signed = sign(unsigned)?;
        std::fs::write(dir.path().join("response"), &signed)?;
        assert_eq!(adapter.sign(announcement().unsigned(None)?).await?, signed);
        let mut invalid = announcement().unsigned(None)?;
        invalid.kind = 1;
        assert!(adapter.sign(invalid).await.is_err());
        assert!(
            adapter
                .encrypt_self(&client_pubkey(), plaintext)
                .await
                .is_err()
        );
        assert!(
            Nak::new(
                config(dir.path().join("helper.py"), dir.path().into()),
                client_key()
            )
            .is_err()
        );
        assert!(
            Nak::assemble(
                config(dir.path().join("helper.py"), dir.path().into()),
                Zeroizing::new(hex::encode([4; 32]))
            )
            .is_err()
        );
        Ok(())
    }
    #[tokio::test]
    async fn output_failures_and_timeout_are_bounded_redacted_and_reaped() -> Result<()> {
        for mode in ["stdout", "stderr", "failure", "utf8", "timeout"] {
            let dir = crate::test_directory()?;
            let adapter = stub(dir.path(), mode)?;
            let error = adapter
                .run(Operation::Sign, "{}", 65536)
                .await
                .err()
                .unwrap();
            assert!(!format!("{error:#}").contains("PRIVATE-DIAGNOSTIC"));
            let pid = std::fs::read_to_string(dir.path().join("pid"))?;
            assert!(
                !Path::new(&format!("/proc/{pid}")).exists(),
                "helper not reaped"
            );
        }
        Ok(())
    }
    #[tokio::test]
    async fn helper_cancellation_kills_child_and_invalid_output_cannot_change_signed_intent()
    -> Result<()> {
        let dir = crate::test_directory()?;
        let adapter = stub(dir.path(), "timeout")?;
        let task = tokio::spawn(async move { adapter.run(Operation::Sign, "{}", 65536).await });
        tokio::time::timeout(Duration::from_secs(2), async {
            while !dir.path().join("pid").exists() {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await?;
        let pid = std::fs::read_to_string(dir.path().join("pid"))?;
        task.abort();
        let _ = task.await;
        tokio::time::timeout(Duration::from_secs(3), async {
            while Path::new(&format!("/proc/{pid}")).exists() {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await?;
        let adapter = stub(dir.path(), "response")?;
        let mut changed = announcement().unsigned(None)?;
        changed.created_at += 1;
        std::fs::write(dir.path().join("response"), sign(changed)?)?;
        assert!(adapter.sign(announcement().unsigned(None)?).await.is_err());
        Ok(())
    }

    struct Process(std::process::Child);
    impl Drop for Process {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    fn start(binary: &Path, args: &[String], secret: Option<&str>) -> Result<Process> {
        let mut command = std::process::Command::new(binary);
        command
            .args(args)
            .env_clear()
            .env("NO_COLOR", "1")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        if let Some(secret) = secret {
            command.env("NOSTR_SECRET_KEY", secret);
        }
        Ok(Process(command.spawn()?))
    }
    #[tokio::test]
    #[ignore = "requires explicit non-root loopback-only namespace and pinned real nak"]
    async fn isolated_real_nak_bunker_roundtrip() -> Result<()> {
        ensure!(
            rustix::process::geteuid().as_raw() > 0
                && std::env::var_os("CREDENTIALS_DIRECTORY").is_none(),
            "fixture requires non-root credential-free identity"
        );
        let host_namespace = std::env::var("LG_CASHU_HOST_NETNS")?;
        ensure!(
            host_namespace
                .strip_prefix("net:[")
                .and_then(|s| s.strip_suffix(']'))
                .is_some_and(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit())),
            "invalid fixture host namespace"
        );
        ensure!(
            std::fs::read_link("/proc/self/ns/net")?.to_string_lossy() != host_namespace,
            "fixture requires isolated network namespace"
        );
        let devices = std::fs::read_to_string("/proc/net/dev")?;
        let names: Vec<_> = devices
            .lines()
            .skip(2)
            .filter_map(|line| line.split_once(':').map(|(name, _)| name.trim()))
            .collect();
        ensure!(names == ["lo"], "fixture requires loopback only");
        let root = crate::test_directory()?;
        let binary = PathBuf::from("/usr/local/bin/nak");
        let mut preflight = config(binary.clone(), root.path().to_path_buf());
        preflight.binary_sha256 =
            "b44b36c792fbc3fb73b7ba3bbc94beda2219826271aa8d5f130f569c3817c3b9".into();
        preflight.runtime_files()?;
        let socket = std::net::TcpListener::bind("127.0.0.1:0")?;
        let port = socket.local_addr()?.port();
        drop(socket);
        let relay_url = format!("ws://127.0.0.1:{port}");
        let _relay = start(
            &binary,
            &[
                "--config-path".into(),
                root.path().join("relay").display().to_string(),
                "serve".into(),
                "--hostname".into(),
                "127.0.0.1".into(),
                "--port".into(),
                port.to_string(),
            ],
            None,
        )?;
        tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                if tokio::net::TcpStream::connect((std::net::Ipv4Addr::LOCALHOST, port))
                    .await
                    .is_ok()
                {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await?;
        // Publicly known fixture bytes, never project credentials.
        let signer_key = Zeroizing::new(hex::encode([4; 32]));
        let bunker = start(
            &binary,
            &[
                "--config-path".into(),
                root.path().join("bunker").display().to_string(),
                "bunker".into(),
                "--authorized-keys".into(),
                client_pubkey(),
                relay_url.clone(),
            ],
            Some(&signer_key),
        )?;
        tokio::time::sleep(Duration::from_millis(300)).await;
        let client_directory = root.path().join("client");
        std::fs::create_dir(&client_directory)?;
        std::fs::set_permissions(&client_directory, std::fs::Permissions::from_mode(0o700))?;
        let helper = PathBuf::from(std::env::var("LG_CASHU_NAK_HELPER")?);
        use std::os::unix::fs::MetadataExt;
        let metadata = std::fs::symlink_metadata(&helper)?;
        ensure!(
            metadata.is_file()
                && metadata.uid() == 0
                && metadata.nlink() == 1
                && metadata.mode() & 0o022 == 0,
            "fixture helper must be a root-owned immutable copy"
        );
        let mut configuration = config(helper, client_directory);
        configuration.binary_sha256 = std::env::var("LG_CASHU_NAK_SHA256")?;
        configuration.relays = vec![relay_url];
        configuration.deadline_seconds = 45;
        configuration.digest_and_state()?;
        // Only this guarded test permits loopback and a root-owned scratch helper.
        // Production also requires protected ancestors and a configured WSS relay.
        let adapter = Nak::assemble(configuration, client_key())?;
        let broker_directory = root.path().join("broker");
        std::fs::create_dir(&broker_directory)?;
        std::fs::set_permissions(&broker_directory, std::fs::Permissions::from_mode(0o700))?;
        let policy = crate::publication_broker::Policy {
            broker_instance: uuid::Uuid::new_v4(),
            broker_epoch: uuid::Uuid::new_v4(),
            wallet_instance: uuid::Uuid::new_v4(),
            wallet_epoch: uuid::Uuid::new_v4(),
            wallet_uid: rustix::process::geteuid().as_raw() + 1,
            owner: owner(),
            receiving_key: cdk::nuts::SecretKey::from_slice(&[1; 32])?
                .public_key()
                .x_only_public_key()
                .to_string(),
            mints: vec!["https://mint.example".into()],
            relays: vec!["wss://relay.example".into()],
            announcements: true,
            redemptions: true,
            max_history_sats: 100,
            max_new_per_hour: 8,
            max_attempts: 3,
        };
        let now = i64::try_from(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_secs(),
        )?;
        let mut request = crate::publication_broker::Request {
            version: 1,
            wallet_instance: policy.wallet_instance,
            wallet_epoch: policy.wallet_epoch,
            operation_id: uuid::Uuid::new_v4(),
            intent: crate::publication::Intent::Redemption {
                owner: owner(),
                created_at: now as u64,
                acquisition_id: uuid::Uuid::new_v4(),
                source_event: "ab".repeat(32),
                sender: client_pubkey(),
                net_sats: 7,
            },
        };
        let mut broker = crate::publication_broker::Broker::initialize(&broker_directory, policy)?;
        let history = broker.sign(&request, &adapter, now).await?;
        history.validate_for(&request)?;
        let ciphertext = history.ciphertext.as_ref().unwrap();
        assert_eq!(
            adapter.decrypt_self(&owner(), ciphertext).await?,
            request.intent.encryption_plaintext()?.unwrap()
        );
        use base64::Engine;
        let mut altered = base64::engine::general_purpose::STANDARD.decode(ciphertext)?;
        *altered.last_mut().unwrap() ^= 1;
        assert!(
            adapter
                .decrypt_self(
                    &owner(),
                    &base64::engine::general_purpose::STANDARD.encode(altered)
                )
                .await
                .is_err()
        );
        request.operation_id = uuid::Uuid::new_v4();
        request.intent = announcement();
        let announcement = broker.sign(&request, &adapter, now).await?;
        announcement.validate_for(&request)?;
        drop(bunker);
        assert_eq!(
            broker.sign(&request, &adapter, now).await?.event_json,
            announcement.event_json
        );
        println!("CASHU_REAL_NIP46_NIP44_AND_KINDS_VERIFIED");
        Ok(())
    }
}
