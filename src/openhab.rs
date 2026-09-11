use std::time::{Duration, SystemTime};

use anyhow::{Context, Result, bail};
use chrono::{DateTime, Utc};
use reqwest::{Client, Url};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use zeroize::Zeroizing;

use crate::secrets::read_systemd_credential;

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OwnerProtocol {
    FeederRequestV1,
    UuidCanary,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OwnerOutcome {
    Absent,
    Pending,
    Complete,
    Rejected,
    Ambiguous,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct OwnerRequest {
    request_id: Uuid,
    requested_at: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct OwnerResult {
    request_id: String,
    status: OwnerStatus,
    reason: String,
    at: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "lowercase")]
enum OwnerStatus {
    Accepted,
    Running,
    Complete,
    Denied,
    Failed,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TrustedOpenHabConfig {
    pub url: String,
    pub request_item: String,
    pub ack_item: String,
    pub protocol: OwnerProtocol,
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
    protocol: OwnerProtocol,
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
        if matches!(config.protocol, OwnerProtocol::UuidCanary)
            && (config.request_item != "LightningGoatsCanaryRequest"
                || config.ack_item != "LightningGoatsCanaryAck")
        {
            bail!("UUID echo protocol is restricted to harmless canary Items");
        }
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
            protocol: config.protocol,
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

    /// An owner rejection describes this invocation only. It must never release
    /// a gateway reservation or authorize a fresh physical request.
    pub async fn feeder_result(&self, request_id: Uuid) -> Result<OwnerOutcome> {
        let state = self.item_state(&self.ack_item).await?;
        match self.protocol {
            OwnerProtocol::FeederRequestV1 => parse_owner_result(&state, request_id),
            OwnerProtocol::UuidCanary => {
                let state = state.trim();
                if state.is_empty() || matches!(state, "NULL" | "UNDEF" | "-") {
                    return Ok(OwnerOutcome::Absent);
                }
                Ok(
                    if Uuid::parse_str(state).context("invalid canary UUID receipt")? == request_id
                    {
                        OwnerOutcome::Complete
                    } else {
                        OwnerOutcome::Absent
                    },
                )
            }
        }
    }

    pub async fn command_feeder_request(&self, request_id: Uuid) -> Result<()> {
        let command = match self.protocol {
            OwnerProtocol::FeederRequestV1 => serde_json::to_string(&OwnerRequest {
                request_id,
                requested_at: DateTime::<Utc>::from(SystemTime::now())
                    .to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
            })?,
            OwnerProtocol::UuidCanary => request_id.to_string(),
        };
        self.command_item(&self.request_item, &command).await
    }

    pub async fn temperature_f(&self) -> Result<Option<f64>> {
        let Some(item) = &self.temperature_item else {
            return Ok(None);
        };
        let state = self.item_state(item).await?;
        Ok(Some(parse_temperature_f(&state)?))
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

fn parse_temperature_f(raw: &str) -> Result<f64> {
    let raw = raw.trim();
    let (number, celsius) =
        if let Some(value) = raw.strip_suffix("°F").or_else(|| raw.strip_suffix('F')) {
            (value, false)
        } else if let Some(value) = raw.strip_suffix("°C").or_else(|| raw.strip_suffix('C')) {
            (value, true)
        } else {
            bail!("OpenHAB temperature must include explicit Celsius or Fahrenheit units");
        };
    let value: f64 = number
        .trim()
        .parse()
        .context("invalid OpenHAB temperature number")?;
    let fahrenheit = if celsius {
        value * 9.0 / 5.0 + 32.0
    } else {
        value
    };
    if !fahrenheit.is_finite() || !(-100.0..=150.0).contains(&fahrenheit) {
        bail!("OpenHAB temperature is non-finite or out of range");
    }
    Ok(fahrenheit)
}

fn parse_owner_result(raw: &str, expected: Uuid) -> Result<OwnerOutcome> {
    let state = raw.trim();
    if state.is_empty() || matches!(state, "NULL" | "UNDEF" | "-") {
        return Ok(OwnerOutcome::Absent);
    }
    let result: OwnerResult =
        serde_json::from_str(state).context("invalid feeder-request-v1 owner result")?;
    if result.request_id != expected.to_string() {
        return Ok(OwnerOutcome::Absent);
    }
    let at = DateTime::parse_from_rfc3339(&result.at).context("invalid owner result timestamp")?;
    if at > DateTime::<Utc>::from(SystemTime::now()) + chrono::Duration::seconds(30) {
        bail!("owner result timestamp is in the future");
    }
    match (result.status, result.reason.as_str()) {
        (OwnerStatus::Complete, "complete") => Ok(OwnerOutcome::Complete),
        (OwnerStatus::Accepted, "accepted") | (OwnerStatus::Running, "pulse_started") => {
            Ok(OwnerOutcome::Pending)
        }
        (
            OwnerStatus::Denied,
            "request_invalid"
            | "request_stale"
            | "ledger_restore_missing"
            | "ledger_invalid"
            | "ledger_recovery_failed"
            | "duplicate"
            | "busy"
            | "cooldown",
        ) => Ok(OwnerOutcome::Rejected),
        (
            OwnerStatus::Failed,
            "clock_invalid"
            | "restart_uncertain"
            | "ledger_readback_failed"
            | "ledger_persist_failed"
            | "execution_error",
        ) => Ok(OwnerOutcome::Ambiguous),
        _ => bail!("contradictory or unknown owner status/reason"),
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
            request_item: "GoatFeeder_ManualRequest".into(),
            ack_item: "ack".into(),
            protocol: OwnerProtocol::FeederRequestV1,
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
    fn temperature_requires_units_and_converts_celsius() {
        for raw in ["20 °C", "20 C", "68 °F", "68F"] {
            assert_eq!(parse_temperature_f(raw).unwrap(), 68.0);
        }
        for raw in [
            "20",
            "20 K",
            "NaN °F",
            "inf C",
            "200 °F",
            "20 °F garbage",
            "20 C F",
        ] {
            assert!(parse_temperature_f(raw).is_err(), "{raw}");
        }
    }

    #[test]
    fn exact_owner_contract_distinguishes_progress_completion_and_uncertainty() {
        let fixtures: serde_json::Value =
            serde_json::from_str(include_str!("../tests/fixtures/openhab/owner-results.json"))
                .unwrap();
        for case in fixtures["cases"].as_array().unwrap() {
            let id = Uuid::parse_str(case["result"]["requestId"].as_str().unwrap()).unwrap();
            let expected = match case["expected"].as_str().unwrap() {
                "pending" => OwnerOutcome::Pending,
                "complete" => OwnerOutcome::Complete,
                "rejected" => OwnerOutcome::Rejected,
                "ambiguous" => OwnerOutcome::Ambiguous,
                _ => panic!("unknown fixture expectation"),
            };
            let raw = case["result"].to_string();
            assert_eq!(parse_owner_result(&raw, id).unwrap(), expected);
            assert_eq!(
                parse_owner_result(&raw, Uuid::new_v4()).unwrap(),
                OwnerOutcome::Absent
            );
        }
    }

    #[test]
    fn aliases_and_contradictory_results_cannot_confirm() {
        let id = Uuid::new_v4();
        for raw in [
            id.to_string(),
            serde_json::json!({"requestId":id,"status":"completed"}).to_string(),
            serde_json::json!({"request_id":id,"success":true}).to_string(),
            serde_json::json!({"requestId":id,"status":"complete","reason":"execution_error","at":"2026-09-11T00:00:00Z"}).to_string(),
            serde_json::json!({"requestId":id,"status":"failed","reason":"execution_error","success":true,"at":"2026-09-11T00:00:00Z"}).to_string(),
        ] {
            assert!(parse_owner_result(&raw, id).is_err(), "{raw}");
        }
    }

    #[test]
    fn nullish_result_is_absent_and_canary_cannot_bind_physical_items() {
        for state in ["", "NULL", "UNDEF", "-"] {
            assert_eq!(
                parse_owner_result(state, Uuid::new_v4()).unwrap(),
                OwnerOutcome::Absent
            );
        }
        let mut config = mock_config("http://127.0.0.1/".into());
        config.protocol = OwnerProtocol::UuidCanary;
        assert!(OpenHabClient::new(&config, "synthetic".into()).is_err());
    }

    #[test]
    fn rejects_remote_openhab_url() {
        let config = TrustedOpenHabConfig {
            url: "http://10.8.0.6:8080/".to_owned(),
            request_item: "GoatFeeder_ManualRequest".to_owned(),
            ack_item: "GoatFeeder_Result".to_owned(),
            protocol: OwnerProtocol::FeederRequestV1,
            override_item: "FeederOverride".to_owned(),
            remote_enabled_item: "LightningGoatsRemoteEnabled".to_owned(),
            temperature_item: None,
        };
        assert!(OpenHabClient::new(&config, "token".to_owned()).is_err());
    }
}
