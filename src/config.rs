use std::{fs, net::SocketAddr, path::Path};

use anyhow::{Context, Result, bail};
use serde::Deserialize;

use crate::domain::invoice::validate_user;

#[derive(Debug, Clone, Deserialize)]
pub struct AppConfig {
    pub service: ServiceConfig,
    pub database: DatabaseConfig,
    pub lightning: LightningConfig,
    pub feeder: FeederConfig,
    pub openhab: OpenHabConfig,
    pub nostr: NostrConfig,
}

impl AppConfig {
    pub fn load(path: &Path) -> Result<Self> {
        let raw = fs::read_to_string(path)
            .with_context(|| format!("failed reading config {}", path.display()))?;
        let config: Self = toml::from_str(&raw)
            .with_context(|| format!("failed parsing config {}", path.display()))?;
        config.validate()?;
        Ok(config)
    }

    pub fn validate(&self) -> Result<()> {
        if !self.service.listen.ip().is_loopback() {
            bail!("service.listen must use a loopback address; nginx is the public boundary");
        }
        if !self.database.url.starts_with("sqlite://") {
            bail!("database.url must be a file-backed sqlite:// URL");
        }
        if self.lightning.clnrest_url.trim().is_empty() {
            bail!("lightning.clnrest_url must not be empty");
        }
        if self.feeder.threshold_sats == 0 {
            bail!("feeder.threshold_sats must be greater than zero");
        }
        validate_user(&self.lightning.herd_user)
            .context("lightning.herd_user must be a canonical clnaddress user")?;
        if self.openhab.url.trim().is_empty() {
            bail!("openhab.url must not be empty");
        }
        if self.openhab.feeder_rule_id.trim().is_empty() {
            bail!("openhab.feeder_rule_id must not be empty");
        }
        if self.openhab.override_item.trim().is_empty() {
            bail!("openhab.override_item must not be empty");
        }
        if self.nostr.nak_path.trim().is_empty() {
            bail!("nostr.nak_path must not be empty");
        }
        if self.nostr.bunker_pubkey.len() != 64
            || !self.nostr.bunker_pubkey.bytes().all(|byte| byte.is_ascii_hexdigit())
        {
            bail!("nostr.bunker_pubkey must be a 32-byte hex public key");
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct ServiceConfig {
    pub listen: SocketAddr,
    pub mode: RuntimeMode,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DatabaseConfig {
    pub url: String,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum RuntimeMode {
    Shadow,
    Active,
}

impl RuntimeMode {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Shadow => "shadow",
            Self::Active => "active",
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct LightningConfig {
    pub clnrest_url: String,
    pub herd_user: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct FeederConfig {
    pub threshold_sats: u64,
    pub inter_feed_delay_seconds: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct OpenHabConfig {
    pub url: String,
    pub feeder_rule_id: String,
    pub override_item: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct NostrConfig {
    pub nak_path: String,
    pub bunker_pubkey: String,
    #[serde(default)]
    pub relays: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_config() -> AppConfig {
        AppConfig {
            service: ServiceConfig {
                listen: "127.0.0.1:8787".parse().unwrap(),
                mode: RuntimeMode::Shadow,
            },
            database: DatabaseConfig {
                url: "sqlite:///var/lib/lightning-goats/lightning-goats.db".to_owned(),
            },
            lightning: LightningConfig {
                clnrest_url: "https://127.0.0.1:3010".to_owned(),
                herd_user: "herd".to_owned(),
            },
            feeder: FeederConfig {
                threshold_sats: 1_000,
                inter_feed_delay_seconds: 30,
            },
            openhab: OpenHabConfig {
                url: "http://127.0.0.1:8080".to_owned(),
                feeder_rule_id: "88bd9ec4de".to_owned(),
                override_item: "FeederOverride".to_owned(),
            },
            nostr: NostrConfig {
                nak_path: "/usr/local/bin/nak".to_owned(),
                bunker_pubkey: "00".repeat(32),
                relays: vec!["wss://relay.example".to_owned()],
            },
        }
    }

    #[test]
    fn accepts_valid_config() {
        valid_config().validate().unwrap();
    }

    #[test]
    fn rejects_public_listener() {
        let mut config = valid_config();
        config.service.listen = "0.0.0.0:8787".parse().unwrap();
        assert!(config.validate().is_err());
    }

    #[test]
    fn rejects_noncanonical_herd_user() {
        let mut config = valid_config();
        config.lightning.herd_user = "Herd".to_owned();
        assert!(config.validate().is_err());
    }

    #[test]
    fn rejects_zero_threshold() {
        let mut config = valid_config();
        config.feeder.threshold_sats = 0;
        assert!(config.validate().is_err());
    }

    #[test]
    fn rejects_non_file_sqlite_database() {
        let mut config = valid_config();
        config.database.url = "sqlite::memory:".to_owned();
        assert!(config.validate().is_err());
    }
}
