use super::AlertPolicy;
use anyhow::{Result, bail};
use serde::Deserialize;
use std::path::Path;
use tokio::io::AsyncReadExt;
use zeroize::Zeroizing;

const MAX_POLICY_BYTES: usize = 16 * 1024;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PolicyInput {
    threshold_sats: u64,
    recipient: String,
    inbox_relays: Vec<String>,
    account_binding: String,
}

impl AlertPolicy {
    /// Read only the systemd-injected private-alert-policy credential. It must
    /// be a regular, read-only, owner-readable file (0400), at most 16 KiB.
    /// Trust in the directory and file ownership comes from the reviewed unit's
    /// LoadCredential assignment; this loader does not establish host provenance.
    pub async fn from_systemd_credential() -> Result<Self> {
        Self::read_policy_file(&credential_path("private-alert-policy")?).await
    }

    async fn read_policy_file(path: &Path) -> Result<Self> {
        let bytes = tokio::time::timeout(std::time::Duration::from_secs(3), read_policy(path))
            .await
            .map_err(|_| anyhow::anyhow!("private alert policy read timed out"))?
            .map_err(|_| anyhow::anyhow!("private alert policy credential rejected"))?;
        let input: PolicyInput = serde_json::from_slice(&bytes)
            .map_err(|_| anyhow::anyhow!("invalid private alert policy credential"))?;
        Self::new(
            input.threshold_sats,
            input.recipient,
            input.inbox_relays,
            &input.account_binding,
        )
    }
}

fn credential_path(name: &str) -> Result<std::path::PathBuf> {
    let directory = std::env::var_os("CREDENTIALS_DIRECTORY")
        .ok_or_else(|| anyhow::anyhow!("private alert credential unavailable"))?;
    let directory = Path::new(&directory);
    if !directory.is_absolute() {
        bail!("private alert credential directory must be absolute");
    }
    Ok(directory.join(name))
}

// Callers supply only fixed internal credential names, never user paths.
pub(super) async fn credential(name: &'static str) -> Result<Zeroizing<Vec<u8>>> {
    tokio::time::timeout(
        std::time::Duration::from_secs(3),
        read_policy(&credential_path(name)?),
    )
    .await
    .map_err(|_| anyhow::anyhow!("private alert credential read timed out"))?
    .map_err(|_| anyhow::anyhow!("private alert credential rejected"))
}

#[cfg(unix)]
async fn read_policy(path: &Path) -> Result<Zeroizing<Vec<u8>>> {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    let before = tokio::fs::symlink_metadata(path).await?;
    if !before.is_file() || before.permissions().mode() & 0o777 != 0o400 {
        bail!("invalid credential metadata");
    }
    let file = tokio::fs::File::open(path).await?;
    let opened = file.metadata().await?;
    if before.dev() != opened.dev()
        || before.ino() != opened.ino()
        || !opened.is_file()
        || opened.permissions().mode() & 0o777 != 0o400
        || opened.len() > MAX_POLICY_BYTES as u64
    {
        bail!("invalid credential metadata");
    }
    let mut bytes = Zeroizing::new(Vec::new());
    file.take((MAX_POLICY_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .await?;
    if bytes.is_empty() || bytes.len() > MAX_POLICY_BYTES {
        bail!("invalid credential length");
    }
    Ok(bytes)
}

#[cfg(not(unix))]
async fn read_policy(_path: &Path) -> Result<Zeroizing<Vec<u8>>> {
    bail!("private alert credentials require Unix permission checks")
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::{
        fs,
        os::unix::fs::{PermissionsExt, symlink},
    };

    fn write(path: &Path, bytes: &[u8], mode: u32) {
        // Remove the previous read-only fixture before creating a new one.
        let _ = fs::remove_file(path);
        fs::write(path, bytes).unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(mode)).unwrap();
    }
    fn valid() -> Vec<u8> {
        serde_json::to_vec(&serde_json::json!({
            "threshold_sats":100, "recipient":"ab".repeat(32),
            "inbox_relays":["wss://inbox.invalid"], "account_binding":"synthetic"
        }))
        .unwrap()
    }

    #[tokio::test]
    async fn credential_requires_bounded_readonly_regular_file() {
        let root = tempfile::TempDir::new().unwrap();
        let path = root.path().join("policy");
        write(&path, &valid(), 0o400);
        assert!(AlertPolicy::read_policy_file(&path).await.is_ok());
        for mode in [0o600, 0o440, 0o444, 0o000] {
            write(&path, &valid(), mode);
            assert!(AlertPolicy::read_policy_file(&path).await.is_err());
        }
        write(&path, &vec![b' '; MAX_POLICY_BYTES + 1], 0o400);
        assert!(AlertPolicy::read_policy_file(&path).await.is_err());
        write(&path, &valid(), 0o400);
        let link = root.path().join("link");
        symlink(&path, &link).unwrap();
        assert!(AlertPolicy::read_policy_file(&link).await.is_err());
        assert!(AlertPolicy::read_policy_file(root.path()).await.is_err());
    }

    #[tokio::test]
    async fn unknown_duplicate_missing_and_invalid_values_are_redacted() {
        let root = tempfile::TempDir::new().unwrap();
        let path = root.path().join("policy");
        let mut unknown: serde_json::Value = serde_json::from_slice(&valid()).unwrap();
        unknown["private_sentinel"] = "DO-NOT-LOG".into();
        let duplicate =
            String::from_utf8(valid())
                .unwrap()
                .replacen('{', "{\"threshold_sats\":123,", 1);
        let mut bad: serde_json::Value = serde_json::from_slice(&valid()).unwrap();
        bad["threshold_sats"] = "DO-NOT-LOG".into();
        for bytes in [
            serde_json::to_vec(&unknown).unwrap(),
            duplicate.into_bytes(),
            serde_json::to_vec(&bad).unwrap(),
            b"{}".to_vec(),
        ] {
            write(&path, &bytes, 0o400);
            let error = match AlertPolicy::read_policy_file(&path).await {
                Ok(_) => panic!("invalid policy accepted"),
                Err(error) => error,
            };
            assert_eq!(error.to_string(), "invalid private alert policy credential");
            assert!(!format!("{error:#}").contains("DO-NOT-LOG"));
        }
    }
}
