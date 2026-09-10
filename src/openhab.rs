use std::time::Duration;

use anyhow::{Context, Result, bail};
use reqwest::{Client, Url};
use serde::Deserialize;
use serde_json::Value;
use uuid::Uuid;
use zeroize::Zeroizing;

use crate::secrets::read_systemd_credential;

const REQUEST_ID_PLACEHOLDER: &str = "{request_id}";

#[derive(Debug, Clone, Deserialize)]
pub struct TrustedOpenHabConfig {
    pub url: String,
    pub request_item: String,
    pub ack_item: String,
    /// Exact command body template expected by the existing correlated feeder
    /// owner. It must contain exactly one `{request_id}` placeholder and no
    /// other brace expansion. This lets deployment bind to the live owner
    /// contract without changing code.
    pub request_payload_template: String,
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
    request_payload_template: String,
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
            (
                &config.ack_item,
                "OpenHAB feeder acknowledgement/result item",
            ),
            (&config.override_item, "OpenHAB override item"),
            (&config.remote_enabled_item, "OpenHAB remote-enabled item"),
        ] {
            validate_identifier(value, field)?;
        }
        validate_request_payload_template(&config.request_payload_template)?;
        if let Some(item) = &config.temperature_item {
            validate_identifier(item, "OpenHAB temperature item")?;
        }

        let mut base_url = Url::parse(&config.url).context("invalid OpenHAB URL")?;
        match base_url.scheme() {
            "http" | "https" => {}
            scheme => bail!("unsupported OpenHAB URL scheme: {scheme}"),
        }
        let host = base_url
            .host_str()
            .context("OpenHAB URL is missing a host")?;
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
            .redirect(reqwest::redirect::Policy::none())
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
            request_payload_template: config.request_payload_template.clone(),
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

    /// Return the UUID of the most recently successful correlated feeder result.
    ///
    /// The trusted gateway supports both a dedicated ack Item containing only a
    /// UUID and the existing OpenHAB correlated-owner style where the result Item
    /// is JSON. JSON is accepted only when it contains a UUID identifier and an
    /// explicit success/completed outcome; unknown result shapes fail closed.
    pub async fn acknowledged_request(&self) -> Result<Option<Uuid>> {
        let state = self.item_state(&self.ack_item).await?;
        parse_acknowledgement(&state)
    }

    pub async fn command_feeder_request(&self, request_id: Uuid) -> Result<()> {
        let command = self
            .request_payload_template
            .replace(REQUEST_ID_PLACEHOLDER, &request_id.to_string());
        if command.len() > 2_048 {
            bail!("rendered OpenHAB feeder request exceeds 2048 bytes");
        }
        self.command_item(&self.request_item, &command).await
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
        let response = self
            .client
            .get(endpoint)
            .basic_auth(self.auth_token.as_str(), Some(""))
            .send()
            .await
            .context("failed requesting OpenHAB item state")?;
        if !response.status().is_success() {
            bail!("OpenHAB item state returned HTTP {}", response.status());
        }
        let bytes = crate::http::bounded_body(response, 4096).await?;
        String::from_utf8(bytes).context("OpenHAB state must be UTF-8")
    }

    async fn command_item(&self, item: &str, command: &str) -> Result<()> {
        let endpoint = self
            .base_url
            .join(&format!("rest/items/{item}"))
            .context("failed constructing OpenHAB item command URL")?;
        let response = self
            .client
            .post(endpoint)
            .basic_auth(self.auth_token.as_str(), Some(""))
            .header("Content-Type", "text/plain")
            .body(command.to_owned())
            .send()
            .await
            .context("OpenHAB item command failed")?;
        if !response.status().is_success() {
            bail!("OpenHAB command returned HTTP {}", response.status());
        }
        Ok(())
    }
}

fn validate_request_payload_template(template: &str) -> Result<()> {
    if template.is_empty() || template.len() > 1_024 {
        bail!("OpenHAB request_payload_template must contain 1 to 1024 characters");
    }
    if template.matches(REQUEST_ID_PLACEHOLDER).count() != 1 {
        bail!("OpenHAB request_payload_template must contain exactly one {REQUEST_ID_PLACEHOLDER}");
    }
    let without_id = template.replace(REQUEST_ID_PLACEHOLDER, "");
    if without_id.contains('{') || without_id.contains('}') {
        bail!("OpenHAB request_payload_template contains unsupported brace expansion");
    }
    if template.contains('\n') || template.contains('\r') || template.contains('\0') {
        bail!("OpenHAB request_payload_template must be a single bounded command value");
    }
    Ok(())
}

fn parse_acknowledgement(raw: &str) -> Result<Option<Uuid>> {
    let state = raw.trim();
    if state.is_empty() || matches!(state, "NULL" | "UNDEF" | "-") {
        return Ok(None);
    }
    if let Ok(id) = Uuid::parse_str(state) {
        return Ok(Some(id));
    }

    let value: Value = serde_json::from_str(state)
        .context("OpenHAB feeder result is neither a UUID nor valid JSON")?;
    let object = value
        .as_object()
        .context("OpenHAB feeder result JSON must be an object")?;
    let id_text = ["request_id", "requestId", "id"]
        .into_iter()
        .find_map(|key| object.get(key).and_then(Value::as_str))
        .context("OpenHAB feeder result JSON is missing request UUID")?;
    let id = Uuid::parse_str(id_text)
        .with_context(|| format!("OpenHAB feeder result request ID is not a UUID: {id_text:?}"))?;

    if object.get("success").and_then(Value::as_bool) == Some(true) {
        return Ok(Some(id));
    }
    if object.get("success").and_then(Value::as_bool) == Some(false) {
        bail!("OpenHAB feeder result explicitly reports failure for request {id}");
    }

    let outcome = ["status", "outcome", "result"]
        .into_iter()
        .find_map(|key| object.get(key).and_then(Value::as_str))
        .context("OpenHAB feeder result JSON lacks an explicit success outcome")?;
    match outcome.trim().to_ascii_lowercase().as_str() {
        "acknowledged" | "completed" | "confirmed" | "success" | "succeeded" | "fed" | "done" => {
            Ok(Some(id))
        }
        "failed" | "failure" | "rejected" | "error" | "blocked" => {
            bail!("OpenHAB feeder result reports {outcome:?} for request {id}")
        }
        other => bail!("OpenHAB feeder result has unknown outcome {other:?}"),
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
    use super::*;

    fn mock_config(url: String) -> TrustedOpenHabConfig {
        TrustedOpenHabConfig {
            url,
            request_item: "request".into(),
            ack_item: "ack".into(),
            request_payload_template: "{request_id}".into(),
            override_item: "override".into(),
            remote_enabled_item: "remote".into(),
            temperature_item: None,
        }
    }

    #[tokio::test]
    async fn state_reads_reject_oversized_chunked_and_non_utf8_data() {
        use axum::{Router, body::Body, response::Response};
        for bytes in [vec![b'x'; 4097], vec![0xff]] {
            let app = Router::new().fallback(move || {
                let bytes = bytes.clone();
                async move {
                    Response::new(Body::from_stream(futures_util::stream::iter([Ok::<
                        _,
                        std::convert::Infallible,
                    >(
                        bytes
                    )])))
                }
            });
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let url = format!("http://{}/", listener.local_addr().unwrap());
            let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
            let client = OpenHabClient::new(&mock_config(url), "synthetic-token".into()).unwrap();
            assert!(client.item_state("ack").await.is_err());
            assert!(client.feeder_safety().await.is_err());
            server.abort();
        }
    }

    #[tokio::test]
    async fn redirects_never_move_state_reads_or_repeat_commands() {
        use axum::{Router, http::StatusCode};
        use std::sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        };
        let calls = Arc::new(AtomicUsize::new(0));
        let target = Router::new().fallback({
            let calls = calls.clone();
            move || {
                let calls = calls.clone();
                async move {
                    calls.fetch_add(1, Ordering::SeqCst);
                    "OFF"
                }
            }
        });
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let target_url = format!("http://{}/", listener.local_addr().unwrap());
        let target_task = tokio::spawn(async move { axum::serve(listener, target).await.unwrap() });
        for status in [
            StatusCode::TEMPORARY_REDIRECT,
            StatusCode::PERMANENT_REDIRECT,
        ] {
            let url = target_url.clone();
            let source = Router::new().fallback(move || {
                let url = url.clone();
                async move { (status, [("location", url)]) }
            });
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let source_url = format!("http://{}/", listener.local_addr().unwrap());
            let source_task =
                tokio::spawn(async move { axum::serve(listener, source).await.unwrap() });
            let client =
                OpenHabClient::new(&mock_config(source_url), "synthetic-token".into()).unwrap();
            assert!(client.item_state("override").await.is_err());
            assert!(client.command_feeder_request(Uuid::new_v4()).await.is_err());
            assert_eq!(calls.load(Ordering::SeqCst), 0);
            source_task.abort();
        }
        target_task.abort();
    }

    #[test]
    fn parses_exact_uuid_and_correlated_json_success() {
        let id = Uuid::new_v4();
        assert_eq!(parse_acknowledgement(&id.to_string()).unwrap(), Some(id));
        assert_eq!(
            parse_acknowledgement(&format!(r#"{{"requestId":"{id}","status":"completed"}}"#))
                .unwrap(),
            Some(id)
        );
        assert_eq!(
            parse_acknowledgement(&format!(r#"{{"request_id":"{id}","success":true}}"#)).unwrap(),
            Some(id)
        );
    }

    #[test]
    fn correlated_failure_or_unknown_shape_fails_closed() {
        let id = Uuid::new_v4();
        assert!(
            parse_acknowledgement(&format!(r#"{{"requestId":"{id}","status":"failed"}}"#)).is_err()
        );
        assert!(parse_acknowledgement(&format!(r#"{{"requestId":"{id}"}}"#)).is_err());
    }

    #[test]
    fn request_template_is_narrow_and_deterministic() {
        validate_request_payload_template("{request_id}").unwrap();
        validate_request_payload_template(r#"request={request_id};source=lightning-goats"#)
            .unwrap();
        assert!(validate_request_payload_template("missing-id").is_err());
        assert!(validate_request_payload_template("{request_id}{request_id}").is_err());
        assert!(validate_request_payload_template("{request_id}{other}").is_err());
    }

    #[test]
    fn nullish_ack_is_none() {
        for state in ["", "NULL", "UNDEF", "-"] {
            assert_eq!(parse_acknowledgement(state).unwrap(), None);
        }
    }

    #[test]
    fn rejects_remote_openhab_url() {
        let config = TrustedOpenHabConfig {
            url: "http://10.8.0.6:8080/".to_owned(),
            request_item: "GoatFeeder_ManualRequest".to_owned(),
            ack_item: "GoatFeeder_Result".to_owned(),
            request_payload_template: "{request_id}".to_owned(),
            override_item: "FeederOverride".to_owned(),
            remote_enabled_item: "LightningGoatsRemoteEnabled".to_owned(),
            temperature_item: None,
        };
        assert!(OpenHabClient::new(&config, "token".to_owned()).is_err());
    }
}
