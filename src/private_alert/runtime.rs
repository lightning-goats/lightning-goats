use super::{AlertPolicy, AlertStore, policy::credential, run_private_alert_worker};
use crate::{config::NostrConfig, nostr::NakClient, strike::StrikeClient};
use anyhow::{Result, bail};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::path::{Component, PathBuf};
use zeroize::Zeroizing;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RuntimeConfig {
    database_url: String,
    strike_api_url: String,
    nak_path: PathBuf,
    nak_config_path: PathBuf,
    bunker_pubkey: String,
    bunker_relays: Vec<String>,
}

impl RuntimeConfig {
    async fn load() -> Result<Self> {
        let raw = credential("private-alert-runtime").await?;
        let config: Self = serde_json::from_slice(&raw)
            .map_err(|_| anyhow::anyhow!("invalid private alert runtime configuration"))?;
        config
            .validate()
            .map_err(|_| anyhow::anyhow!("invalid private alert runtime configuration"))?;
        Ok(config)
    }
    fn validate(&self) -> Result<()> {
        let database = crate::sqlite::durable_options(&self.database_url)?;
        for path in [
            database.get_filename(),
            self.nak_path.as_path(),
            self.nak_config_path.as_path(),
        ] {
            if !path.is_absolute()
                || path
                    .components()
                    .any(|part| !matches!(part, Component::RootDir | Component::Normal(_)))
            {
                bail!("private alert paths must be absolute and canonical");
            }
        }
        let url = reqwest::Url::parse(&self.strike_api_url)?;
        let local = url
            .host_str()
            .and_then(|h| h.parse::<std::net::IpAddr>().ok())
            .is_some_and(|ip| ip.is_loopback());
        if self.strike_api_url.len() > 2048
            || url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
            || !(url.scheme() == "https" || (url.scheme() == "http" && local))
        {
            bail!("invalid private alert provider origin");
        }
        if self.bunker_pubkey.len() != 64
            || !self
                .bunker_pubkey
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            bail!("invalid private alert signer identity");
        }
        crate::nostr::validate_private_relays(&self.bunker_relays)
    }
    fn nostr(&self) -> NostrConfig {
        NostrConfig {
            nak_path: self.nak_path.clone(),
            nak_config_path: self.nak_config_path.clone(),
            bunker_pubkey: self.bunker_pubkey.clone(),
            relays: self.bunker_relays.clone(),
        }
    }
    async fn policy(&self) -> Result<AlertPolicy> {
        let mut policy = AlertPolicy::from_systemd_credential().await?;
        // A provider/signer reassignment also requires explicit reconciliation.
        let binding = serde_json::to_vec(&(
            &policy.binding,
            &self.strike_api_url,
            &self.bunker_pubkey,
            &self.bunker_relays,
        ))?;
        policy.binding = hex::encode(Sha256::digest(binding));
        Ok(policy)
    }
}

/// Preparation only: no provider/signer credential is read and no network
/// client or subprocess is constructed. Existing alert state is never reset.
pub async fn initialize_private_alert() -> Result<()> {
    let config = RuntimeConfig::load().await?;
    let policy = config.policy().await?;
    let store = AlertStore::initialize(&config.database_url, &policy).await?;
    store.pool.close().await;
    Ok(())
}

/// Offline preflight of existing state and protected configuration; does not
/// attest provider permissions or signer capability and does not publish.
pub async fn check_private_alert() -> Result<()> {
    let config = RuntimeConfig::load().await?;
    let policy = config.policy().await?;
    let store = AlertStore::connect(&config.database_url, &policy).await?;
    store.pool.close().await;
    Ok(())
}

async fn key(name: &'static str) -> Result<Zeroizing<String>> {
    let raw = credential(name).await?;
    let text = std::str::from_utf8(&raw)
        .map_err(|_| anyhow::anyhow!("invalid private alert credential"))?
        .trim();
    if text.is_empty() || text.len() > 4096 {
        bail!("invalid private alert credential");
    }
    Ok(Zeroizing::new(text.to_owned()))
}

/// Explicit operational invocation only. Uses balance-read and NIP-46 client
/// credentials dedicated to this worker; no webhook/OpenHAB credentials loaded.
#[cfg(unix)]
pub async fn run_private_alert() -> Result<()> {
    use tokio::signal::unix::{SignalKind, signal};
    // Register shutdown before opening state or constructing clients.
    let mut terminate = signal(SignalKind::terminate())?;
    let mut interrupt = signal(SignalKind::interrupt())?;
    let config = RuntimeConfig::load().await?;
    let policy = config.policy().await?;
    let store = AlertStore::connect(&config.database_url, &policy).await?;
    let mut strike_key = key("private-alert-strike-key").await?;
    let mut nostr_key = key("private-alert-nostr-key").await?;
    let strike = StrikeClient::new(&config.strike_api_url, std::mem::take(&mut *strike_key))?;
    let nak = NakClient::new(&config.nostr(), std::mem::take(&mut *nostr_key))?;
    let (stop, receiver) = tokio::sync::watch::channel(false);
    let shutdown = tokio::spawn(async move {
        tokio::select! { _ = terminate.recv() => {}, _ = interrupt.recv() => {} }
        let _ = stop.send(true);
    });
    run_private_alert_worker(&store, &policy, &strike, &nak, receiver).await;
    shutdown.abort();
    let _ = shutdown.await;
    store.pool.close().await;
    Ok(())
}

#[cfg(not(unix))]
pub async fn run_private_alert() -> Result<()> {
    bail!("private alert runtime requires Unix");
}
