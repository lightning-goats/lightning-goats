use std::{net::IpAddr, str::FromStr, time::Duration};

use anyhow::{Context, Result, bail};
use reqwest::{Client, StatusCode, Url};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

const MAX_ERROR_BODY: usize = 2_048;

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
pub struct FeedRequestStatus {
    pub request_id: Uuid,
    pub status: String,
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
        if response.temperature_f.is_some_and(|value| !value.is_finite()) {
            bail!("integration gateway returned non-finite temperature");
        }
        Ok(response.temperature_f)
    }

    pub async fn request_feed(&self, request_id: Uuid) -> Result<()> {
        let endpoint = self
            .base_url
            .join(&format!("v1/feeder/request/{request_id}"))?;
        let response = self
            .client
            .post(endpoint)
            .header("Content-Length", "0")
            .send()
            .await
            .context("integration gateway feeder request failed")?;
        if response.status() == StatusCode::NO_CONTENT {
            return Ok(());
        }
        provider_error(response, "feeder request").await
    }

    pub async fn feed_request_status(&self, request_id: Uuid) -> Result<FeedRequestStatus> {
        let endpoint = self
            .base_url
            .join(&format!("v1/feeder/request/{request_id}"))?;
        self.get_json(endpoint, "feeder request status").await
    }

    pub async fn weather(&self) -> Result<WeatherSnapshot> {
        let endpoint = self.base_url.join("v1/weather")?;
        self.get_json(endpoint, "weather").await
    }

    async fn get_json<T: for<'de> Deserialize<'de>>(&self, endpoint: Url, operation: &str) -> Result<T> {
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
        response
            .json::<T>()
            .await
            .with_context(|| format!("integration gateway {operation} returned malformed JSON"))
    }
}

fn validate_gateway_url(url: &Url) -> Result<()> {
    if !url.username().is_empty() || url.password().is_some() {
        bail!("integration gateway URL must not contain credentials");
    }
    let host = url.host_str().context("integration gateway URL is missing a host")?;
    match url.scheme() {
        "https" => Ok(()),
        "http" => {
            if host.eq_ignore_ascii_case("localhost") {
                return Ok(());
            }
            let ip = IpAddr::from_str(host)
                .context("plain HTTP integration gateway URL must use a literal private/loopback IP")?;
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

async fn provider_error(response: reqwest::Response, operation: &str) -> Result<()> {
    Err(provider_error_value(response, operation).await)
}

async fn provider_error_value(response: reqwest::Response, operation: &str) -> anyhow::Error {
    let status = response.status();
    let body = response.bytes().await.unwrap_or_default();
    let safe = String::from_utf8_lossy(&body[..body.len().min(MAX_ERROR_BODY)]);
    anyhow::anyhow!("integration gateway {operation} failed with HTTP {status}: {safe}")
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
}
