use std::time::Duration;

use anyhow::{Context, Result, bail};
use reqwest::{Client, Url};
use serde::Deserialize;
use uuid::Uuid;
use zeroize::Zeroizing;

use crate::secrets::read_systemd_credential;

#[derive(Debug, Clone, Deserialize)]
pub struct TrustedOpenHabConfig {
    pub url: String,
    pub request_item: String,
    pub ack_item: String,
    pub override_item: String,
    pub remote_enabled_item: String,
    #[serde(default)]
    pub temperature_item: Option<String>,
}

#[derive(Clone)]
pub struct OpenHabClient {
    client: Client,
    base_url: Url,
    auth_token: Zeroizing<String>,
    request_item: String,
    ack_item: String,
    override_item: String,
    remote_enabled_item: String,
    temperature_item: Option<String>,
}

impl OpenHabClient {
    pub async fn from_config(config: &TrustedOpenHabConfig) -> Result<Self> {
        let token = read_systemd_credential("openhab-token").await?;
        Self::new(config, token)
    }

    pub fn new(config: &TrustedOpenHabConfig, auth_token: String) -> Result<Self> {
        if auth_token.trim().is_empty() {
            bail!("OpenHAB authentication token is empty");
        }
        for (value, field) in [
            (&config.request_item, "OpenHAB feeder request item"),
            (&config.ack_item, "OpenHAB feeder acknowledgement item"),
            (&config.override_item, "OpenHAB override item"),
            (&config.remote_enabled_item, "OpenHAB remote-enabled item"),
        ] {
            validate_identifier(value, field)?;
        }
        if let Some(item) = &config.temperature_item {
            validate_identifier(item, "OpenHAB temperature item")?;
        }

        let mut base_url = Url::parse(&config.url).context("invalid OpenHAB URL")?;
        match base_url.scheme() {
            "http" | "https" => {}
            scheme => bail!("unsupported OpenHAB URL scheme: {scheme}"),
        }
        let host = base_url.host_str().context("OpenHAB URL is missing a host")?;
        if !matches!(host, "127.0.0.1" | "localhost" | "::1") {
            bail!("trusted gateway OpenHAB URL must use loopback");
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
            .no_proxy()
            .connect_timeout(Duration::from_secs(2))
            .timeout(Duration::from_secs(5))
            .build()
            .context("failed building OpenHAB HTTP client")?;

        Ok(Self {
            client,
            base_url,
            auth_token: Zeroizing::new(auth_token),
            request_item: config.request_item.clone(),
            ack_item: config.ack_item.clone(),
            override_item: config.override_item.clone(),
            remote_enabled_item: config.remote_enabled_item.clone(),
            temperature_item: config.temperature_item.clone(),
        })
    }

    pub async fn feeder_safety(&self) -> Result<(bool, bool)> {
        let override_enabled = parse_switch(
            &self.item_state(&self.override_item).await?,
            "FeederOverride",
        )?;
        let remote_enabled = parse_switch(
            &self.item_state(&self.remote_enabled_item).await?,
            "LightningGoatsRemoteEnabled",
        )?;
        Ok((override_enabled, remote_enabled))
    }

    pub async fn acknowledged_request(&self) -> Result<Option<Uuid>> {
        let state = self.item_state(&self.ack_item).await?;
        let state = state.trim();
        if state.is_empty() || matches!(state, "NULL" | "UNDEF" | "-") {
            return Ok(None);
        }
        Uuid::parse_str(state)
            .map(Some)
            .with_context(|| format!("OpenHAB feeder acknowledgement is not a UUID: {state:?}"))
    }

    pub async fn command_feeder_request(&self, request_id: Uuid) -> Result<()> {
        self.command_item(&self.request_item, &request_id.to_string())
            .await
    }

    pub async fn temperature_f(&self) -> Result<Option<f64>> {
        let Some(item) = &self.temperature_item else {
            return Ok(None);
        };
        let state = self.item_state(item).await?;
        let number = state
            .split_whitespace()
            .next()
            .context("OpenHAB temperature state is empty")?
            .parse::<f64>()
            .with_context(|| format!("OpenHAB temperature state is not numeric: {state:?}"))?;
        if !number.is_finite() || !(-100.0..=150.0).contains(&number) {
            bail!("OpenHAB temperature state is non-finite or out of range");
        }
        Ok(Some(number))
    }

    async fn item_state(&self, item: &str) -> Result<String> {
        let endpoint = self
            .base_url
            .join(&format!("rest/items/{item}/state"))
            .context("failed constructing OpenHAB item state URL")?;
        self.client
            .get(endpoint)
            .basic_auth(self.auth_token.as_str(), Some(""))
            .send()
            .await
            .context("failed requesting OpenHAB item state")?
            .error_for_status()
            .context("OpenHAB item state request returned an error status")?
            .text()
            .await
            .context("failed reading OpenHAB item state")
    }

    async fn command_item(&self, item: &str, command: &str) -> Result<()> {
        let endpoint = self
            .base_url
            .join(&format!("rest/items/{item}"))
            .context("failed constructing OpenHAB item command URL")?;
        self.client
            .post(endpoint)
            .basic_auth(self.auth_token.as_str(), Some(""))
            .header("Content-Type", "text/plain")
            .body(command.to_owned())
            .send()
            .await
            .context("OpenHAB item command failed")?
            .error_for_status()
            .context("OpenHAB item command returned an error status")?;
        Ok(())
    }
}

fn parse_switch(state: &str, name: &str) -> Result<bool> {
    match state.trim() {
        "ON" => Ok(true),
        "OFF" => Ok(false),
        other => bail!("OpenHAB {name} returned unexpected state {other:?}"),
    }
}

fn validate_identifier(value: &str, field: &str) -> Result<()> {
    if value.is_empty() || value.len() > 128 {
        bail!("{field} must contain 1 to 128 characters");
    }
    if !value
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
    {
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
        body::Bytes,
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
        remote_state: &'static str,
        ack_state: String,
        commands: Arc<AtomicUsize>,
    }

    async fn state_handler(
        State(state): State<MockState>,
        headers: HeaderMap,
        axum::extract::Path(item): axum::extract::Path<String>,
    ) -> impl IntoResponse {
        if headers.get("authorization").is_none() {
            return (StatusCode::UNAUTHORIZED, "missing auth").into_response();
        }
        let value = match item.as_str() {
            "FeederOverride" => state.override_state.to_owned(),
            "LightningGoatsRemoteEnabled" => state.remote_state.to_owned(),
            "LightningGoatsFeederAck" => state.ack_state,
            "AmbientTemperature" => "67.4 °F".to_owned(),
            _ => return StatusCode::NOT_FOUND.into_response(),
        };
        (StatusCode::OK, value).into_response()
    }

    async fn command_handler(
        State(state): State<MockState>,
        headers: HeaderMap,
        body: Bytes,
    ) -> impl IntoResponse {
        if headers.get("authorization").is_none() {
            return StatusCode::UNAUTHORIZED;
        }
        assert!(Uuid::parse_str(std::str::from_utf8(&body).unwrap()).is_ok());
        state.commands.fetch_add(1, Ordering::SeqCst);
        StatusCode::ACCEPTED
    }

    async fn spawn_mock(state: MockState) -> String {
        let app = Router::new()
            .route("/rest/items/{item}/state", get(state_handler))
            .route("/rest/items/LightningGoatsFeederRequest", post(command_handler))
            .with_state(state);
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        format!("http://{address}/")
    }

    fn config(url: String) -> TrustedOpenHabConfig {
        TrustedOpenHabConfig {
            url,
            request_item: "LightningGoatsFeederRequest".to_owned(),
            ack_item: "LightningGoatsFeederAck".to_owned(),
            override_item: "FeederOverride".to_owned(),
            remote_enabled_item: "LightningGoatsRemoteEnabled".to_owned(),
            temperature_item: Some("AmbientTemperature".to_owned()),
        }
    }

    #[tokio::test]
    async fn reads_safety_ack_temperature_and_commands_uuid() {
        let request_id = Uuid::new_v4();
        let commands = Arc::new(AtomicUsize::new(0));
        let base_url = spawn_mock(MockState {
            override_state: "OFF",
            remote_state: "ON",
            ack_state: request_id.to_string(),
            commands: Arc::clone(&commands),
        })
        .await;
        let client = OpenHabClient::new(&config(base_url), "token".to_owned()).unwrap();

        assert_eq!(client.feeder_safety().await.unwrap(), (false, true));
        assert_eq!(client.acknowledged_request().await.unwrap(), Some(request_id));
        assert_eq!(client.temperature_f().await.unwrap(), Some(67.4));
        client.command_feeder_request(request_id).await.unwrap();
        assert_eq!(commands.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn rejects_non_loopback_openhab_and_path_injection() {
        let mut cfg = config("http://10.8.0.6:8080/".to_owned());
        assert!(OpenHabClient::new(&cfg, "token".to_owned()).is_err());
        cfg.url = "http://127.0.0.1:8080/".to_owned();
        cfg.request_item = "../rule".to_owned();
        assert!(OpenHabClient::new(&cfg, "token".to_owned()).is_err());
    }
}
