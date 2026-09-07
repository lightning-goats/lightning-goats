use std::{
    collections::HashSet,
    fs,
    net::{IpAddr, SocketAddr},
    path::{Path, PathBuf},
    str::FromStr,
};

use anyhow::{Context, Result, bail};
use reqwest::Url;
use serde::Deserialize;

use crate::domain::invoice::validate_user;

pub const REQUIRED_PHASE1_LIGHTNING_USERS: [&str; 6] =
    ["herd", "dexter", "rowan", "cosmo", "newton", "nova"];

#[derive(Debug, Clone, Deserialize)]
pub struct AppConfig {
    pub service: ServiceConfig,
    pub database: DatabaseConfig,
    pub strike: StrikeConfig,
    pub lnurl: LnurlConfig,
    #[serde(default)]
    pub lightning_address: Vec<LightningAddressConfig>,
    pub feeder: FeederConfig,
    pub gateway: IntegrationGatewayConfig,
    pub informational: InformationalConfig,
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
        if !self.database.url.starts_with("sqlite://") || self.database.url == "sqlite::memory:" {
            bail!("database.url must be a file-backed sqlite:// URL");
        }

        let strike_url = Url::parse(&self.strike.api_url).context("invalid strike.api_url")?;
        if strike_url.scheme() != "https" || strike_url.host_str().is_none() {
            bail!("strike.api_url must use https:// with a host");
        }
        if !strike_url.username().is_empty()
            || strike_url.password().is_some()
            || strike_url.query().is_some()
            || strike_url.fragment().is_some()
        {
            bail!("strike.api_url must not contain credentials, query, or fragment");
        }

        self.validate_lnurl()?;
        if self.feeder.threshold_sats == 0 {
            bail!("feeder.threshold_sats must be greater than zero");
        }
        if self.feeder.inter_feed_delay_seconds == 0 {
            bail!("feeder.inter_feed_delay_seconds must be greater than zero");
        }
        validate_gateway_origin(&self.gateway.url)?;
        self.validate_informational()?;
        self.validate_nostr()?;
        Ok(())
    }

    fn validate_lnurl(&self) -> Result<()> {
        if self.lnurl.invoice_expiry_seconds == 0 || self.lnurl.invoice_expiry_seconds > 86_400 {
            bail!("lnurl.invoice_expiry_seconds must be between 1 and 86400 seconds");
        }
        let public_url =
            Url::parse(&self.lnurl.public_base_url).context("invalid lnurl.public_base_url")?;
        if public_url.scheme() != "https" || public_url.host_str().is_none() {
            bail!("lnurl.public_base_url must use https:// with a host");
        }
        if !public_url.username().is_empty()
            || public_url.password().is_some()
            || public_url.query().is_some()
            || public_url.fragment().is_some()
            || public_url.path() != "/"
        {
            bail!(
                "lnurl.public_base_url must be a root HTTPS origin without credentials, query, fragment, or path"
            );
        }

        if self.lightning_address.is_empty() {
            bail!("Phase 1 requires configured Lightning Addresses");
        }
        let mut users = HashSet::new();
        for address in &self.lightning_address {
            validate_user(&address.user)
                .with_context(|| format!("invalid Lightning Address user {}", address.user))?;
            if !users.insert(address.user.as_str()) {
                bail!("duplicate Lightning Address user {}", address.user);
            }
            if address.display_name.trim().is_empty() || address.display_name.len() > 80 {
                bail!("Lightning Address display_name must contain 1 to 80 characters");
            }
            if address.description.trim().is_empty() || address.description.len() > 250 {
                bail!("Lightning Address description must contain 1 to 250 characters");
            }
            validate_user(&address.credit_pool).with_context(|| {
                format!(
                    "invalid Lightning Address credit_pool {}",
                    address.credit_pool
                )
            })?;
            if address.credit_pool != "herd" {
                bail!(
                    "Phase 1 Lightning Address {} must credit the herd pool",
                    address.user
                );
            }
            if address.min_sendable_msat == 0
                || address.max_sendable_msat < address.min_sendable_msat
            {
                bail!(
                    "Lightning Address {} has an invalid min/max sendable range",
                    address.user
                );
            }
        }
        for required in REQUIRED_PHASE1_LIGHTNING_USERS {
            if !users.contains(required) {
                bail!("Phase 1 Lightning Address registry is missing required user {required}");
            }
        }
        Ok(())
    }

    fn validate_informational(&self) -> Result<()> {
        if !(10..=3_600).contains(&self.informational.interval_seconds) {
            bail!("informational.interval_seconds must be between 10 and 3600");
        }
        for (name, value) in [
            (
                "informational.interface_info_probability",
                self.informational.interface_info_probability,
            ),
            (
                "informational.weather_probability",
                self.informational.weather_probability,
            ),
        ] {
            if !value.is_finite() || !(0.0..=1.0).contains(&value) {
                bail!("{name} must be a finite probability between 0 and 1");
            }
        }
        if self.informational.interface_info_probability + self.informational.weather_probability
            > 1.0
        {
            bail!(
                "informational interface + weather unconditional probabilities must not exceed 1"
            );
        }
        Ok(())
    }

    fn validate_nostr(&self) -> Result<()> {
        if !self.nostr.nak_path.is_absolute() {
            bail!("nostr.nak_path must be an absolute path");
        }
        if !self.nostr.nak_config_path.is_absolute() {
            bail!("nostr.nak_config_path must be an absolute path");
        }
        if self.nostr.bunker_pubkey.len() != 64
            || !self
                .nostr
                .bunker_pubkey
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit())
        {
            bail!("nostr.bunker_pubkey must be a 32-byte hex public key");
        }
        if self.nostr.relays.is_empty() {
            bail!("nostr.relays must contain at least one relay");
        }
        for relay in &self.nostr.relays {
            let parsed =
                Url::parse(relay).with_context(|| format!("invalid Nostr relay URL {relay}"))?;
            if parsed.scheme() != "wss" || parsed.host_str().is_none() {
                bail!("Nostr relay URLs must use wss:// with a host: {relay}");
            }
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
    Canary,
    Active,
}

impl RuntimeMode {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Shadow => "shadow",
            Self::Canary => "canary",
            Self::Active => "active",
        }
    }

    #[must_use]
    pub const fn feeder_enabled(self) -> bool {
        matches!(self, Self::Canary | Self::Active)
    }

    #[must_use]
    pub const fn nostr_enabled(self) -> bool {
        matches!(self, Self::Active)
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct StrikeConfig {
    pub api_url: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct LnurlConfig {
    pub public_base_url: String,
    pub invoice_expiry_seconds: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct LightningAddressConfig {
    pub user: String,
    pub display_name: String,
    pub description: String,
    pub credit_pool: String,
    pub min_sendable_msat: u64,
    pub max_sendable_msat: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct FeederConfig {
    pub threshold_sats: u64,
    pub inter_feed_delay_seconds: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct IntegrationGatewayConfig {
    pub url: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct InformationalConfig {
    pub interval_seconds: u64,
    pub interface_info_enabled: bool,
    pub weather_enabled: bool,
    pub interface_info_probability: f64,
    pub weather_probability: f64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct NostrConfig {
    pub nak_path: PathBuf,
    pub nak_config_path: PathBuf,
    pub bunker_pubkey: String,
    #[serde(default)]
    pub relays: Vec<String>,
}

fn validate_gateway_origin(raw: &str) -> Result<()> {
    let url = Url::parse(raw).context("invalid gateway.url")?;
    if !url.username().is_empty() || url.password().is_some() {
        bail!("gateway.url must not contain credentials");
    }
    if url.query().is_some() || url.fragment().is_some() || url.path() != "/" {
        bail!("gateway.url must be a root origin without query, fragment, or path");
    }
    let host = url.host_str().context("gateway.url is missing a host")?;
    match url.scheme() {
        "https" => Ok(()),
        "http" => {
            if host.eq_ignore_ascii_case("localhost") {
                return Ok(());
            }
            let ip = IpAddr::from_str(host)
                .context("plain HTTP gateway.url must use a literal private/loopback IP")?;
            if ip.is_loopback() || is_private_ip(ip) {
                Ok(())
            } else {
                bail!("plain HTTP gateway.url must use a private/loopback address")
            }
        }
        scheme => bail!("unsupported gateway.url scheme {scheme}"),
    }
}

fn is_private_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => ip.is_private(),
        IpAddr::V6(ip) => ip.is_unique_local(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn phase1_addresses() -> Vec<LightningAddressConfig> {
        REQUIRED_PHASE1_LIGHTNING_USERS
            .into_iter()
            .map(|user| LightningAddressConfig {
                user: user.to_owned(),
                display_name: if user == "herd" {
                    "Lightning Goats".to_owned()
                } else {
                    let mut chars = user.chars();
                    let first = chars.next().unwrap().to_ascii_uppercase();
                    format!("{first}{}", chars.as_str())
                },
                description: format!("Feed the Lightning Goats via {user}"),
                credit_pool: "herd".to_owned(),
                min_sendable_msat: 1_000,
                max_sendable_msat: 1_000_000_000,
            })
            .collect()
    }

    fn valid_config() -> AppConfig {
        AppConfig {
            service: ServiceConfig {
                listen: "127.0.0.1:8787".parse().unwrap(),
                mode: RuntimeMode::Shadow,
            },
            database: DatabaseConfig {
                url: "sqlite:///var/lib/lightning-goats/lightning-goats.db".to_owned(),
            },
            strike: StrikeConfig {
                api_url: "https://api.strike.me/".to_owned(),
            },
            lnurl: LnurlConfig {
                public_base_url: "https://lightning-goats.com/".to_owned(),
                invoice_expiry_seconds: 300,
            },
            lightning_address: phase1_addresses(),
            feeder: FeederConfig {
                threshold_sats: 1_000,
                inter_feed_delay_seconds: 30,
            },
            gateway: IntegrationGatewayConfig {
                url: "http://10.8.0.6:8789/".to_owned(),
            },
            informational: InformationalConfig {
                interval_seconds: 60,
                interface_info_enabled: true,
                weather_enabled: true,
                interface_info_probability: 0.4,
                weather_probability: 0.4,
            },
            nostr: NostrConfig {
                nak_path: PathBuf::from("/usr/local/bin/nak"),
                nak_config_path: PathBuf::from("/run/lightning-goats/nak"),
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
    fn canary_actuates_without_enabling_nostr() {
        assert!(RuntimeMode::Canary.feeder_enabled());
        assert!(!RuntimeMode::Canary.nostr_enabled());
    }

    #[test]
    fn rejects_public_listener() {
        let mut config = valid_config();
        config.service.listen = "0.0.0.0:8787".parse().unwrap();
        assert!(config.validate().is_err());
    }

    #[test]
    fn rejects_public_plain_http_gateway() {
        let mut config = valid_config();
        config.gateway.url = "http://8.8.8.8:8789/".to_owned();
        assert!(config.validate().is_err());
    }

    #[test]
    fn rejects_insecure_strike_api_url() {
        let mut config = valid_config();
        config.strike.api_url = "http://api.strike.me/".to_owned();
        assert!(config.validate().is_err());
    }

    #[test]
    fn rejects_missing_required_goat_address() {
        let mut config = valid_config();
        config
            .lightning_address
            .retain(|address| address.user != "nova");
        assert!(config.validate().is_err());
    }

    #[test]
    fn rejects_duplicate_goat_address() {
        let mut config = valid_config();
        config
            .lightning_address
            .push(config.lightning_address[0].clone());
        assert!(config.validate().is_err());
    }

    #[test]
    fn rejects_non_herd_credit_pool_in_phase1() {
        let mut config = valid_config();
        config.lightning_address[1].credit_pool = "dexter".to_owned();
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

    #[test]
    fn rejects_insecure_nostr_relay() {
        let mut config = valid_config();
        config.nostr.relays = vec!["ws://relay.example".to_owned()];
        assert!(config.validate().is_err());
    }
}
