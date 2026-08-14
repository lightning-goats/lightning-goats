use std::{env, path::PathBuf};

use anyhow::{Context, Result, bail};

pub async fn read_systemd_credential(name: &str) -> Result<String> {
    if name.is_empty()
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        bail!("invalid systemd credential name");
    }

    let directory = env::var_os("CREDENTIALS_DIRECTORY")
        .context("CREDENTIALS_DIRECTORY is not set; service credentials are unavailable")?;
    let path = PathBuf::from(directory).join(name);
    let value = tokio::fs::read_to_string(&path)
        .await
        .with_context(|| format!("failed reading systemd credential {name}"))?;
    let value = value.trim().to_owned();
    if value.is_empty() {
        bail!("systemd credential {name} is empty");
    }
    Ok(value)
}
