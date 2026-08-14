use std::time::Duration;

use anyhow::{Context, Result, bail};
use reqwest::{Client, Url};
use zeroize::Zeroizing;

use crate::{config::OpenHabConfig, secrets::read_systemd_credential};

#[derive(Clone)]
pub struct OpenHabClient {
    client: Client,
    base_url: Url,
    auth_token: Zeroizing<String>,
    feeder_rule_id: String,
    override_item: String,
}

impl OpenHabClient {
    pub async fn from_config(config: &OpenHabConfig) -> Result<Self> {
        let token = read_systemd_credential("openhab-token").await?;
        Self::new(
            &config.url,
            token,
            &config.feeder_rule_id,
            &config.override_item,
        )
    }

    pub fn new(
        base_url: &str,
        auth_token: String,
        feeder_rule_id: &str,
        override_item: &str,
    ) -> Result<Self> {
        if auth_token.trim().is_empty() {
            bail!("OpenHAB authentication token is empty");
        }
        validate_identifier(feeder_rule_id, "OpenHAB feeder rule ID")?;
        validate_identifier(override_item, "OpenHAB override item")?;

        let mut base_url = Url::parse(base_url).context("invalid OpenHAB URL")?;
        match base_url.scheme() {
            "http" | "https" => {}
            scheme => bail!("unsupported OpenHAB URL scheme: {scheme}"),
        }
        if base_url.host_str().is_none() {
            bail!("OpenHAB URL is missing a host");
        }
        if !base_url.username().is_empty() || base_url.password().is_some() {
            bail!("OpenHAB credentials must not be embedded in the URL");
        }
        if base_url.query().is_some() || base_url.fragment().is_some() {
            bail!("OpenHAB base URL must not contain a query or fragment");
        }
        if !base_url.path().ends_with('/') {
            base_url.set_path(&format!("{}/", base_url.path()));
        }

        let client = Client::builder()
            .connect_timeout(Duration::from_secs(3))
            .timeout(Duration::from_secs(5))
            .build()
            .context("failed building OpenHAB HTTP client")?;

        Ok(Self {
            client,
            base_url,
            auth_token: Zeroizing::new(auth_token),
            feeder_rule_id: feeder_rule_id.to_owned(),
            override_item: override_item.to_owned(),
        })
    }

    pub async fn feeder_override_enabled(&self) -> Result<bool> {
        let endpoint = self
            .base_url
            .join(&format!("rest/items/{}/state", self.override_item))
            .context("failed constructing OpenHAB override URL")?;
        let response = self
            .client
            .get(endpoint)
            .basic_auth(self.auth_token.as_str(), Some(""))
            .send()
            .await
            .context("failed requesting OpenHAB feeder override state")?
            .error_for_status()
            .context("OpenHAB feeder override request returned an error status")?;
        let state = response
            .text()
            .await
            .context("failed reading OpenHAB feeder override state")?;

        match state.trim() {
            "ON" => Ok(true),
            "OFF" => Ok(false),
            other => bail!("OpenHAB feeder override returned unexpected state {other:?}"),
        }
    }

    pub async fn trigger_feeder(&self) -> Result<()> {
        let endpoint = self
            .base_url
            .join(&format!("rest/rules/{}/runnow", self.feeder_rule_id))
            .context("failed constructing OpenHAB feeder rule URL")?;
        self.client
            .post(endpoint)
            .basic_auth(self.auth_token.as_str(), Some(""))
            .send()
            .await
            .context("OpenHAB feeder request failed")?
            .error_for_status()
            .context("OpenHAB feeder request returned an error status")?;
        Ok(())
    }
}

fn validate_identifier(value: &str, field: &str) -> Result<()> {
    if value.is_empty() || value.len() > 128 {
        bail!("{field} must contain 1 to 128 characters");
    }
    if !value.bytes().all(|byte| {
        byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.')
    }) {
        bail!("{field} contains unsupported characters");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    use axum::{
        Router,
        extract::State,
        http::{HeaderMap, StatusCode},
        response::IntoResponse,
        routing::{get, post},
    };
    use tokio::net::TcpListener;

    use super::*;

    #[derive(Clone)]
    struct MockState {
        override_state: &'static str,
        triggers: Arc<AtomicUsize>,
    }

    async fn override_handler(
        State(state): State<MockState>,
        headers: HeaderMap,
    ) -> impl IntoResponse {
        if headers.get("authorization").is_none() {
            return (StatusCode::UNAUTHORIZED, "missing auth").into_response();
        }
        (StatusCode::OK, state.override_state).into_response()
    }

    async fn feeder_handler(
        State(state): State<MockState>,
        headers: HeaderMap,
    ) -> impl IntoResponse {
        if headers.get("authorization").is_none() {
            return (StatusCode::UNAUTHORIZED, "missing auth").into_response();
        }
        state.triggers.fetch_add(1, Ordering::SeqCst);
        StatusCode::OK.into_response()
    }

    async fn spawn_mock(state: MockState) -> String {
        let app = Router::new()
            .route("/rest/items/FeederOverride/state", get(override_handler))
            .route("/rest/rules/rule123/runnow", post(feeder_handler))
            .with_state(state);
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        format!("http://{address}/")
    }

    #[tokio::test]
    async fn reads_override_and_triggers_rule() {
        let triggers = Arc::new(AtomicUsize::new(0));
        let base_url = spawn_mock(MockState {
            override_state: "OFF",
            triggers: Arc::clone(&triggers),
        })
        .await;
        let client = OpenHabClient::new(&base_url, "token".to_owned(), "rule123", "FeederOverride")
            .unwrap();

        assert!(!client.feeder_override_enabled().await.unwrap());
        client.trigger_feeder().await.unwrap();
        assert_eq!(triggers.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn unexpected_override_state_fails_closed_to_caller() {
        let base_url = spawn_mock(MockState {
            override_state: "UNDEF",
            triggers: Arc::new(AtomicUsize::new(0)),
        })
        .await;
        let client = OpenHabClient::new(&base_url, "token".to_owned(), "rule123", "FeederOverride")
            .unwrap();

        assert!(client.feeder_override_enabled().await.is_err());
    }

    #[test]
    fn rejects_path_injection_identifiers() {
        assert!(OpenHabClient::new(
            "http://127.0.0.1:8080/",
            "token".to_owned(),
            "../rule",
            "FeederOverride"
        )
        .is_err());
    }
}
