use std::{collections::HashMap, sync::Arc};

use anyhow::{Context, Result, anyhow, bail};
use reqwest::Url;
use serde::Serialize;
use serde_json::json;
use sha2::{Digest, Sha256};

use crate::{
    config::{LightningAddressConfig, LnurlConfig},
    domain::invoice::validate_user,
    ledger::LedgerStore,
    strike::StrikeRuntime,
};

const PAY_REQUEST_TAG: &str = "payRequest";

#[derive(Clone)]
pub struct LnurlService {
    public_base_url: Url,
    invoice_expiry_seconds: u64,
    addresses: Arc<HashMap<String, RegisteredAddress>>,
    strike: StrikeRuntime,
    ledger: LedgerStore,
}

#[derive(Debug, Clone)]
struct RegisteredAddress {
    user: String,
    display_name: String,
    credit_pool: String,
    min_sendable_msat: u64,
    max_sendable_msat: u64,
    metadata: String,
    description_hash: String,
}

#[derive(Debug)]
pub enum LnurlServiceError {
    UnknownUser,
    InvalidAmount(&'static str),
    Busy,
    Provider(anyhow::Error),
}

impl LnurlServiceError {
    #[must_use]
    pub const fn is_unknown_user(&self) -> bool {
        matches!(self, Self::UnknownUser)
    }

    #[must_use]
    pub fn public_reason(&self) -> &'static str {
        match self {
            Self::UnknownUser => "Unknown Lightning Address",
            Self::InvalidAmount(reason) => reason,
            Self::Provider(_) => "Unable to create Lightning invoice",
            Self::Busy => "Invoice service is busy; retry later",
        }
    }

    #[must_use]
    pub fn internal_error(&self) -> Option<&anyhow::Error> {
        match self {
            Self::Provider(error) => Some(error),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LnurlPayRequest {
    pub callback: String,
    pub max_sendable: u64,
    pub min_sendable: u64,
    pub metadata: String,
    pub tag: &'static str,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct LnurlPayCallbackResponse {
    pub pr: String,
    pub routes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct LnurlErrorResponse {
    pub status: &'static str,
    pub reason: String,
}

impl LnurlErrorResponse {
    #[must_use]
    pub fn new(reason: impl Into<String>) -> Self {
        Self {
            status: "ERROR",
            reason: reason.into(),
        }
    }
}

impl LnurlService {
    pub fn new(
        config: &LnurlConfig,
        configured_addresses: &[LightningAddressConfig],
        strike: StrikeRuntime,
        ledger: LedgerStore,
    ) -> Result<Self> {
        let public_base_url =
            Url::parse(&config.public_base_url).context("invalid LNURL public base URL")?;
        let host = public_base_url
            .host_str()
            .context("LNURL public base URL has no host")?;
        let mut addresses = HashMap::with_capacity(configured_addresses.len());
        for configured in configured_addresses {
            validate_user(&configured.user).context("invalid configured Lightning Address user")?;
            if configured.credit_pool != "herd" {
                bail!("Phase 1 Lightning Address must use herd credit pool");
            }
            let identifier = format!("{}@{host}", configured.user);
            let metadata = serde_json::to_string(&json!([
                ["text/plain", configured.description],
                ["text/identifier", identifier]
            ]))
            .context("failed serializing LNURL metadata")?;
            let description_hash = hex::encode(Sha256::digest(metadata.as_bytes()));
            let registered = RegisteredAddress {
                user: configured.user.clone(),
                display_name: configured.display_name.clone(),
                credit_pool: configured.credit_pool.clone(),
                min_sendable_msat: configured.min_sendable_msat,
                max_sendable_msat: configured.max_sendable_msat,
                metadata,
                description_hash,
            };
            if addresses
                .insert(configured.user.clone(), registered)
                .is_some()
            {
                bail!("duplicate Lightning Address user {}", configured.user);
            }
        }

        Ok(Self {
            public_base_url,
            invoice_expiry_seconds: config.invoice_expiry_seconds,
            addresses: Arc::new(addresses),
            strike,
            ledger,
        })
    }

    pub fn discovery(&self, user: &str) -> Result<LnurlPayRequest, LnurlServiceError> {
        let address = self.address(user)?;
        let callback = self
            .public_base_url
            .join(&format!("lnurlp/{}/callback", address.user))
            .map_err(|error| LnurlServiceError::Provider(anyhow!(error)))?;
        Ok(LnurlPayRequest {
            callback: callback.to_string(),
            max_sendable: address.max_sendable_msat,
            min_sendable: address.min_sendable_msat,
            metadata: address.metadata.clone(),
            tag: PAY_REQUEST_TAG,
        })
    }

    pub async fn callback(
        &self,
        user: &str,
        amount_msat: u64,
    ) -> Result<LnurlPayCallbackResponse, LnurlServiceError> {
        let address = self.address(user)?;
        if amount_msat < address.min_sendable_msat {
            return Err(LnurlServiceError::InvalidAmount(
                "Amount is below the minimum",
            ));
        }
        if amount_msat > address.max_sendable_msat {
            return Err(LnurlServiceError::InvalidAmount(
                "Amount exceeds the maximum",
            ));
        }
        if amount_msat % 1_000 != 0 {
            return Err(LnurlServiceError::InvalidAmount(
                "Amount must be a whole number of satoshis",
            ));
        }

        let created = self
            .strike
            .create_and_record_receive_request(
                &self.ledger,
                &address.user,
                &address.credit_pool,
                amount_msat,
                &address.description_hash,
                self.invoice_expiry_seconds,
            )
            .await
            .map_err(|error| {
                if error.is::<crate::strike::InvoiceCapacityError>() {
                    LnurlServiceError::Busy
                } else {
                    LnurlServiceError::Provider(error)
                }
            })?;
        Ok(LnurlPayCallbackResponse {
            pr: created.invoice,
            routes: Vec::new(),
        })
    }

    #[must_use]
    pub fn configured_users(&self) -> Vec<String> {
        let mut users = self.addresses.keys().cloned().collect::<Vec<_>>();
        users.sort();
        users
    }

    #[must_use]
    pub fn display_name(&self, user: &str) -> Option<&str> {
        self.addresses
            .get(user)
            .map(|address| address.display_name.as_str())
    }

    fn address(&self, user: &str) -> Result<&RegisteredAddress, LnurlServiceError> {
        if validate_user(user).is_err() {
            return Err(LnurlServiceError::UnknownUser);
        }
        self.addresses
            .get(user)
            .ok_or(LnurlServiceError::UnknownUser)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        config::{LightningAddressConfig, LnurlConfig},
        strike::{StrikeClient, StrikeWebhookVerifier},
    };
    use tempfile::TempDir;

    fn configured(user: &str) -> LightningAddressConfig {
        LightningAddressConfig {
            user: user.to_owned(),
            display_name: user.to_owned(),
            description: format!("Feed the Lightning Goats via {user}"),
            credit_pool: "herd".to_owned(),
            min_sendable_msat: 1_000,
            max_sendable_msat: 1_000_000_000,
        }
    }

    async fn service() -> (TempDir, LnurlService) {
        let directory = TempDir::new().unwrap();
        let ledger = LedgerStore::connect(&format!(
            "sqlite://{}",
            directory.path().join("lnurl.db").display()
        ))
        .await
        .unwrap();
        let strike = StrikeRuntime::new(
            StrikeClient::new("http://127.0.0.1:9/", "test-key".to_owned()).unwrap(),
            StrikeWebhookVerifier::new("test-secret".to_owned()).unwrap(),
        );
        let addresses = ["herd", "dexter", "rowan", "cosmo", "newton", "nova"]
            .into_iter()
            .map(configured)
            .collect::<Vec<_>>();
        let service = LnurlService::new(
            &LnurlConfig {
                public_base_url: "https://lightning-goats.com/".to_owned(),
                invoice_expiry_seconds: 300,
            },
            &addresses,
            strike,
            ledger,
        )
        .unwrap();
        (directory, service)
    }

    #[tokio::test]
    async fn discovery_supports_all_phase1_addresses() {
        let (_directory, service) = service().await;
        for user in ["herd", "dexter", "rowan", "cosmo", "newton", "nova"] {
            let discovery = service.discovery(user).unwrap();
            assert_eq!(discovery.tag, "payRequest");
            assert_eq!(
                discovery.callback,
                format!("https://lightning-goats.com/lnurlp/{user}/callback")
            );
            assert!(
                discovery
                    .metadata
                    .contains(&format!("{user}@lightning-goats.com"))
            );
        }
    }

    #[tokio::test]
    async fn metadata_hash_is_exact_sha256_of_returned_raw_string() {
        let (_directory, service) = service().await;
        let discovery = service.discovery("dexter").unwrap();
        let expected = hex::encode(Sha256::digest(discovery.metadata.as_bytes()));
        assert_eq!(service.addresses["dexter"].description_hash, expected);
    }

    #[tokio::test]
    async fn unknown_user_is_rejected_before_provider_access() {
        let (_directory, service) = service().await;
        assert!(matches!(
            service.callback("attacker", 1_000).await,
            Err(LnurlServiceError::UnknownUser)
        ));
    }

    #[tokio::test]
    async fn non_sat_aligned_amount_is_rejected_before_provider_access() {
        let (_directory, service) = service().await;
        assert!(matches!(
            service.callback("herd", 1_001).await,
            Err(LnurlServiceError::InvalidAmount(_))
        ));
    }
}
