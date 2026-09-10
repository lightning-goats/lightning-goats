use std::{net::IpAddr, str::FromStr, time::Duration};

use anyhow::{Context, Result, bail};
use reqwest::{Client, Url};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Clone)]
pub struct GatewayClient {
    client: Client,
    base_url: Url,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct FeederSafety {
    pub override_enabled: bool,
    pub remote_enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct FeedRequestStatus {
    pub request_id: Uuid,
    pub status: FeedOutcome,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refusal: Option<FeedRefusal>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FeedOutcome {
    NotDispatched,
    Pending,
    Confirmed,
    Ambiguous,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RefusalReason {
    Safety,
    Unresolved,
    Capacity,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct FeedRefusal {
    pub reason: RefusalReason,
    pub retry_after_seconds: u64,
}

impl FeedRequestStatus {
    pub fn validate(&self, expected: Uuid) -> Result<()> {
        if self.request_id != expected {
            bail!("gateway response UUID mismatch");
        }
        match (self.status, self.refusal) {
            (FeedOutcome::NotDispatched, Some(refusal))
                if (1..=86_400).contains(&refusal.retry_after_seconds) =>
            {
                Ok(())
            }
            (FeedOutcome::Pending | FeedOutcome::Confirmed | FeedOutcome::Ambiguous, None) => {
                Ok(())
            }
            _ => bail!("contradictory gateway outcome"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WeatherSnapshot {
    pub observed_at: String,
    pub temperature: f64,
    pub humidity: u64,
    pub wind_speed: f64,
    pub wind_direction: String,
    pub uv_index: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub apparent_temperature: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wind_gust: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pressure_relative: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pressure_trend: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rain_hourly: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rain_daily: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub solar_radiation: Option<f64>,
}

impl GatewayClient {
    pub fn new(base_url: &str) -> Result<Self> {
        let mut base_url = Url::parse(base_url).context("invalid integration gateway URL")?;
        validate_gateway_url(&base_url)?;
        if base_url.query().is_some() || base_url.fragment().is_some() {
            bail!("integration gateway URL must not contain query or fragment");
        }
        if base_url.path() != "/" && !base_url.path().is_empty() {
            bail!("integration gateway URL must be a root origin");
        }
        base_url.set_path("/");

        let client = Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(3))
            .timeout(Duration::from_secs(15))
            .build()
            .context("failed building integration gateway HTTP client")?;
        Ok(Self { client, base_url })
    }

    pub async fn health(&self) -> Result<()> {
        let endpoint = self.base_url.join("healthz")?;
        self.client
            .get(endpoint)
            .send()
            .await
            .context("integration gateway health request failed")?
            .error_for_status()
            .context("integration gateway health returned an error status")?;
        Ok(())
    }

    pub async fn feeder_safety(&self) -> Result<FeederSafety> {
        let endpoint = self.base_url.join("v1/feeder/override")?;
        self.get_json(endpoint, "feeder safety").await
    }

    pub async fn temperature_f(&self) -> Result<Option<f64>> {
        #[derive(Deserialize)]
        struct TemperatureResponse {
            temperature_f: Option<f64>,
        }
        let endpoint = self.base_url.join("v1/temperature")?;
        let response: TemperatureResponse = self.get_json(endpoint, "temperature").await?;
        if response
            .temperature_f
            .is_some_and(|value| !value.is_finite())
        {
            bail!("integration gateway returned non-finite temperature");
        }
        Ok(response.temperature_f)
    }

    pub async fn request_feed(&self, request_id: Uuid) -> Result<FeedRequestStatus> {
        let endpoint = self
            .base_url
            .join(&format!("v1/feeder/request/{request_id}"))?;
        let response = self
            .client
            .post(endpoint)
            .timeout(Duration::from_secs(150))
            .header("Content-Length", "0")
            .send()
            .await?;
        Self::read_feed_status(response, request_id).await
    }

    pub async fn feed_request_status(&self, request_id: Uuid) -> Result<FeedRequestStatus> {
        let endpoint = self
            .base_url
            .join(&format!("v1/feeder/request/{request_id}"))?;
        let response = self
            .client
            .get(endpoint)
            .timeout(Duration::from_secs(15))
            .send()
            .await?;
        Self::read_feed_status(response, request_id).await
    }

    async fn read_feed_status(
        response: reqwest::Response,
        request_id: Uuid,
    ) -> Result<FeedRequestStatus> {
        let status = response.status();
        if !matches!(status.as_u16(), 200 | 202 | 409 | 423 | 429 | 504) {
            bail!("gateway feed operation returned HTTP {status}");
        }
        let result: FeedRequestStatus =
            serde_json::from_slice(&crate::http::bounded_body(response, 4096).await?)?;
        result.validate(request_id)?;
        if result.status == FeedOutcome::Confirmed && status.as_u16() != 200 {
            bail!("gateway confirmation HTTP status mismatch");
        }
        if result.status == FeedOutcome::NotDispatched
            && !matches!(status.as_u16(), 200 | 423 | 429)
        {
            bail!("gateway refusal HTTP status mismatch");
        }
        Ok(result)
    }

    pub async fn weather(&self) -> Result<WeatherSnapshot> {
        let endpoint = self.base_url.join("v1/weather")?;
        self.get_json(endpoint, "weather").await
    }

    async fn get_json<T: for<'de> Deserialize<'de>>(
        &self,
        endpoint: Url,
        operation: &str,
    ) -> Result<T> {
        let response = self
            .client
            .get(endpoint)
            .header("Accept", "application/json")
            .send()
            .await
            .with_context(|| format!("integration gateway {operation} request failed"))?;
        if !response.status().is_success() {
            return Err(provider_error_value(response, operation).await);
        }
        serde_json::from_slice(&crate::http::bounded_body(response, 65_536).await?)
            .with_context(|| format!("integration gateway {operation} returned malformed JSON"))
    }
}

fn validate_gateway_url(url: &Url) -> Result<()> {
    if !url.username().is_empty() || url.password().is_some() {
        bail!("integration gateway URL must not contain credentials");
    }
    let host = url
        .host_str()
        .context("integration gateway URL is missing a host")?;
    match url.scheme() {
        "https" => Ok(()),
        "http" => {
            if host.eq_ignore_ascii_case("localhost") {
                return Ok(());
            }
            let ip = IpAddr::from_str(host).context(
                "plain HTTP integration gateway URL must use a literal private/loopback IP",
            )?;
            if ip.is_loopback() || is_private_ip(ip) {
                Ok(())
            } else {
                bail!("plain HTTP integration gateway URL must use a private/loopback address")
            }
        }
        scheme => bail!("unsupported integration gateway URL scheme {scheme}"),
    }
}

fn is_private_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => ip.is_private(),
        IpAddr::V6(ip) => ip.is_unique_local(),
    }
}

async fn provider_error_value(response: reqwest::Response, operation: &str) -> anyhow::Error {
    anyhow::anyhow!(
        "integration gateway {operation} failed with HTTP {}",
        response.status()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allows_existing_wireguard_and_loopback_http() {
        GatewayClient::new("http://10.8.0.6:8789/").unwrap();
        GatewayClient::new("http://127.0.0.1:8789/").unwrap();
    }

    #[test]
    fn rejects_public_plain_http_and_path_urls() {
        assert!(GatewayClient::new("http://8.8.8.8:8789/").is_err());
        assert!(GatewayClient::new("http://10.8.0.6:8789/proxy/").is_err());
    }
    #[tokio::test]
    async fn refuses_uncorrelated_legacy_and_contradictory_feed_responses() {
        use axum::{Router, response::IntoResponse, routing::post};
        let id = Uuid::new_v4();
        for (code,body) in [
            (204, String::new()),
            (200, serde_json::json!({"request_id":Uuid::new_v4(),"status":"confirmed"}).to_string()),
            (200, serde_json::json!({"request_id":id,"status":"acknowledged"}).to_string()),
            (423, serde_json::json!({"request_id":id,"status":"not_dispatched"}).to_string()),
            (200, serde_json::json!({"request_id":id,"status":"confirmed","refusal":{"reason":"capacity","retry_after_seconds":5}}).to_string()),
        ] {
            let app=Router::new().route("/v1/feeder/request/{id}",post(move || async move {
                (axum::http::StatusCode::from_u16(code).unwrap(),body).into_response()
            }));
            let listener=tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let address=listener.local_addr().unwrap();
            let task=tokio::spawn(async move { axum::serve(listener,app).await.unwrap() });
            assert!(GatewayClient::new(&format!("http://{address}")).unwrap().request_feed(id).await.is_err());
            task.abort();
        }
    }
}
