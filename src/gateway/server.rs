use std::{
    fs,
    net::{IpAddr, SocketAddr},
    path::Path,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result, bail};
use axum::{
    Json, Router,
    extract::{Path as AxumPath, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use tokio::time::{Instant, sleep};
use uuid::Uuid;

use crate::openhab::{OpenHabClient, TrustedOpenHabConfig};

use super::{
    client::{FeedRequestStatus, FeederSafety},
    store::{BeginRequestOutcome, GatewayStore, StoredRequestStatus},
    weather::WeatherAdapter,
};

#[derive(Debug, Clone, Deserialize)]
pub struct GatewayServerConfig {
    pub service: GatewayServiceConfig,
    pub database: GatewayDatabaseConfig,
    pub openhab: TrustedOpenHabConfig,
    pub weather: GatewayWeatherConfig,
    pub feeder: GatewayFeederConfig,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GatewayServiceConfig {
    pub listen: SocketAddr,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GatewayDatabaseConfig {
    pub url: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GatewayWeatherConfig {
    pub url: String,
    pub max_stale_seconds: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GatewayFeederConfig {
    pub ack_timeout_seconds: u64,
    pub ack_poll_milliseconds: u64,
    pub min_feed_interval_seconds: u64,
    pub max_feeds_per_hour: u64,
}

#[derive(Clone)]
pub struct TrustedGateway {
    state: GatewayState,
    listen: SocketAddr,
}

#[derive(Clone)]
struct GatewayState {
    openhab: OpenHabClient,
    store: GatewayStore,
    weather: WeatherAdapter,
    ack_timeout: Duration,
    ack_poll: Duration,
    min_feed_interval: Duration,
    max_feeds_per_hour: u64,
}

#[derive(Serialize)]
struct HealthResponse {
    status: &'static str,
}

#[derive(Serialize)]
struct TemperatureResponse {
    temperature_f: Option<f64>,
}

#[derive(Serialize)]
struct ErrorResponse {
    status: &'static str,
    reason: String,
}

impl GatewayServerConfig {
    pub fn load(path: &Path) -> Result<Self> {
        let raw = fs::read_to_string(path)
            .with_context(|| format!("failed reading gateway config {}", path.display()))?;
        let config: Self = toml::from_str(&raw)
            .with_context(|| format!("failed parsing gateway config {}", path.display()))?;
        config.validate()?;
        Ok(config)
    }

    pub fn validate(&self) -> Result<()> {
        let ip = self.service.listen.ip();
        if ip.is_unspecified() || (!ip.is_loopback() && !is_private_ip(ip)) {
            bail!(
                "gateway service.listen must use a private/loopback address, never a public/unspecified address"
            );
        }
        if !self.database.url.starts_with("sqlite://") || self.database.url == "sqlite::memory:" {
            bail!("gateway database.url must be a file-backed sqlite:// URL");
        }
        if self.weather.max_stale_seconds == 0 || self.weather.max_stale_seconds > 86_400 {
            bail!("gateway weather.max_stale_seconds must be between 1 and 86400");
        }
        if self.feeder.ack_timeout_seconds == 0 || self.feeder.ack_timeout_seconds > 120 {
            bail!("gateway feeder.ack_timeout_seconds must be between 1 and 120");
        }
        if self.feeder.ack_poll_milliseconds < 50 || self.feeder.ack_poll_milliseconds > 5_000 {
            bail!("gateway feeder.ack_poll_milliseconds must be between 50 and 5000");
        }
        if self.feeder.min_feed_interval_seconds < 5
            || self.feeder.min_feed_interval_seconds > 86_400
        {
            bail!("gateway feeder.min_feed_interval_seconds must be between 5 and 86400");
        }
        if self.feeder.max_feeds_per_hour == 0 || self.feeder.max_feeds_per_hour > 60 {
            bail!("gateway feeder.max_feeds_per_hour must be between 1 and 60");
        }
        Ok(())
    }
}

impl TrustedGateway {
    pub async fn from_config(config: &GatewayServerConfig) -> Result<Self> {
        config.validate()?;
        let openhab = OpenHabClient::from_config(&config.openhab).await?;
        let store = GatewayStore::connect(&config.database.url).await?;
        let weather = WeatherAdapter::new(
            &config.weather.url,
            Duration::from_secs(config.weather.max_stale_seconds),
        )?;
        Ok(Self {
            state: GatewayState {
                openhab,
                store,
                weather,
                ack_timeout: Duration::from_secs(config.feeder.ack_timeout_seconds),
                ack_poll: Duration::from_millis(config.feeder.ack_poll_milliseconds),
                min_feed_interval: Duration::from_secs(config.feeder.min_feed_interval_seconds),
                max_feeds_per_hour: config.feeder.max_feeds_per_hour,
            },
            listen: config.service.listen,
        })
    }

    #[must_use]
    pub const fn listen(&self) -> SocketAddr {
        self.listen
    }

    pub fn router(&self) -> Router {
        Router::new()
            .route("/healthz", get(healthz))
            .route("/v1/feeder/override", get(feeder_safety))
            .route("/v1/temperature", get(temperature))
            .route(
                "/v1/feeder/request/{request_id}",
                post(feed_request).get(feed_request_status),
            )
            .route("/v1/weather", get(weather))
            .with_state(self.state.clone())
    }
}

async fn healthz() -> Json<HealthResponse> {
    Json(HealthResponse { status: "ok" })
}

async fn feeder_safety(State(state): State<GatewayState>) -> Response {
    match state.openhab.feeder_safety().await {
        Ok((override_enabled, remote_enabled)) => (
            StatusCode::OK,
            Json(FeederSafety {
                override_enabled,
                remote_enabled,
            }),
        )
            .into_response(),
        Err(error) => internal_failure("OpenHAB feeder safety unavailable", error),
    }
}

async fn temperature(State(state): State<GatewayState>) -> Response {
    match state.openhab.temperature_f().await {
        Ok(temperature_f) => {
            (StatusCode::OK, Json(TemperatureResponse { temperature_f })).into_response()
        }
        Err(error) => internal_failure("OpenHAB temperature unavailable", error),
    }
}

async fn feed_request(
    AxumPath(request_id): AxumPath<Uuid>,
    State(state): State<GatewayState>,
) -> Response {
    match state.store.status(request_id).await {
        Ok(Some(StoredRequestStatus::Acknowledged)) => {
            return StatusCode::NO_CONTENT.into_response();
        }
        Ok(Some(StoredRequestStatus::Pending)) => {
            return handle_existing_pending(&state, request_id).await;
        }
        Ok(None) => {}
        Err(error) => return internal_failure("Unable to read feeder request status", error),
    }

    let safety = match state.openhab.feeder_safety().await {
        Ok((override_enabled, remote_enabled)) => FeederSafety {
            override_enabled,
            remote_enabled,
        },
        Err(error) => return internal_failure("OpenHAB feeder safety unavailable", error),
    };
    if safety.override_enabled {
        return public_error(StatusCode::LOCKED, "FeederOverride is ON");
    }
    if !safety.remote_enabled {
        return public_error(
            StatusCode::LOCKED,
            "Lightning Goats remote feeding is disabled",
        );
    }
    if let Err(reason) = enforce_local_rate_safety(&state).await {
        return public_error(StatusCode::TOO_MANY_REQUESTS, &reason);
    }

    match state.store.begin_request(request_id).await {
        Ok(BeginRequestOutcome::New) => {}
        Ok(BeginRequestOutcome::Acknowledged) => return StatusCode::NO_CONTENT.into_response(),
        Ok(BeginRequestOutcome::Pending) => {
            return handle_existing_pending(&state, request_id).await;
        }
        Err(error) => return internal_failure("Unable to persist feeder request", error),
    }

    if let Err(error) = state.openhab.command_feeder_request(request_id).await {
        return internal_failure(
            "OpenHAB feeder command failed after durable intent; outcome is ambiguous",
            error,
        );
    }

    wait_for_ack(&state, request_id).await
}

async fn handle_existing_pending(state: &GatewayState, request_id: Uuid) -> Response {
    match ack_matches(state, request_id).await {
        Ok(true) => {
            if let Err(error) = state.store.mark_acknowledged(request_id).await {
                return internal_failure("Unable to persist feeder acknowledgement", error);
            }
            StatusCode::NO_CONTENT.into_response()
        }
        Ok(false) => public_error(
            StatusCode::CONFLICT,
            "Feeder request already exists in pending/ambiguous state; command was not resent",
        ),
        Err(error) => internal_failure("OpenHAB feeder acknowledgement unavailable", error),
    }
}

async fn wait_for_ack(state: &GatewayState, request_id: Uuid) -> Response {
    let deadline = Instant::now() + state.ack_timeout;
    loop {
        match ack_matches(state, request_id).await {
            Ok(true) => {
                if let Err(error) = state.store.mark_acknowledged(request_id).await {
                    return internal_failure("Unable to persist feeder acknowledgement", error);
                }
                return StatusCode::NO_CONTENT.into_response();
            }
            Ok(false) => {}
            Err(error) => {
                return internal_failure(
                    "OpenHAB acknowledgement read failed after feeder command; outcome is ambiguous",
                    error,
                );
            }
        }
        if Instant::now() >= deadline {
            return public_error(
                StatusCode::GATEWAY_TIMEOUT,
                "Feeder acknowledgement timed out; request remains pending and will not be resent automatically",
            );
        }
        sleep(state.ack_poll).await;
    }
}

async fn feed_request_status(
    AxumPath(request_id): AxumPath<Uuid>,
    State(state): State<GatewayState>,
) -> Response {
    let mut status = match state.store.status(request_id).await {
        Ok(Some(status)) => status,
        Ok(None) => return public_error(StatusCode::NOT_FOUND, "Unknown feeder request UUID"),
        Err(error) => return internal_failure("Unable to read feeder request status", error),
    };
    if status == StoredRequestStatus::Pending {
        match ack_matches(&state, request_id).await {
            Ok(true) => {
                if let Err(error) = state.store.mark_acknowledged(request_id).await {
                    return internal_failure("Unable to persist feeder acknowledgement", error);
                }
                status = StoredRequestStatus::Acknowledged;
            }
            Ok(false) => {}
            Err(error) => {
                return internal_failure("OpenHAB feeder acknowledgement unavailable", error);
            }
        }
    }
    (
        StatusCode::OK,
        Json(FeedRequestStatus {
            request_id,
            status: status.as_str().to_owned(),
        }),
    )
        .into_response()
}

async fn weather(State(state): State<GatewayState>) -> Response {
    match state.weather.fetch().await {
        Ok(weather) => (StatusCode::OK, Json(weather)).into_response(),
        Err(error) => internal_failure("Weather data unavailable", error),
    }
}

async fn enforce_local_rate_safety(state: &GatewayState) -> std::result::Result<(), String> {
    let now = unix_now().map_err(|error| format!("local clock unavailable: {error:#}"))?;
    if let Some(last) = state
        .store
        .last_acknowledged_at()
        .await
        .map_err(|error| format!("feed history unavailable: {error:#}"))?
    {
        let min_interval = i64::try_from(state.min_feed_interval.as_secs())
            .map_err(|_| "configured feed interval is out of range".to_owned())?;
        if now.saturating_sub(last) < min_interval {
            return Err("local minimum feed interval has not elapsed".to_owned());
        }
    }
    let count = state
        .store
        .acknowledged_since(now.saturating_sub(3_600))
        .await
        .map_err(|error| format!("recent feed history unavailable: {error:#}"))?;
    if count >= state.max_feeds_per_hour {
        return Err("local maximum feeds-per-hour safety cap reached".to_owned());
    }
    Ok(())
}

async fn ack_matches(state: &GatewayState, request_id: Uuid) -> Result<bool> {
    Ok(state.openhab.acknowledged_request().await? == Some(request_id))
}

fn unix_now() -> Result<i64> {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system clock is before Unix epoch")?
        .as_secs();
    i64::try_from(seconds).context("Unix time exceeds i64 range")
}

fn public_error(status: StatusCode, reason: &str) -> Response {
    (
        status,
        Json(ErrorResponse {
            status: "ERROR",
            reason: reason.to_owned(),
        }),
    )
        .into_response()
}

fn internal_failure(public_reason: &str, error: anyhow::Error) -> Response {
    tracing::error!(%error, "trusted integration gateway operation failed");
    public_error(StatusCode::BAD_GATEWAY, public_reason)
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

    fn config() -> GatewayServerConfig {
        GatewayServerConfig {
            service: GatewayServiceConfig {
                listen: "10.8.0.6:8789".parse().unwrap(),
            },
            database: GatewayDatabaseConfig {
                url: "sqlite:///var/lib/lightning-goats-gateway/gateway.db".to_owned(),
            },
            openhab: TrustedOpenHabConfig {
                url: "http://127.0.0.1:8080/".to_owned(),
                request_item: "GoatFeeder_ManualRequest".to_owned(),
                ack_item: "GoatFeeder_Result".to_owned(),
                request_payload_template: "{request_id}".to_owned(),
                override_item: "FeederOverride".to_owned(),
                remote_enabled_item: "LightningGoatsRemoteEnabled".to_owned(),
                temperature_item: None,
            },
            weather: GatewayWeatherConfig {
                url: "http://127.0.0.1:5000/get_received_data".to_owned(),
                max_stale_seconds: 300,
            },
            feeder: GatewayFeederConfig {
                ack_timeout_seconds: 20,
                ack_poll_milliseconds: 250,
                min_feed_interval_seconds: 30,
                max_feeds_per_hour: 10,
            },
        }
    }

    #[test]
    fn refuses_public_or_unspecified_listener() {
        let mut config = config();
        config.validate().unwrap();
        config.service.listen = "0.0.0.0:8789".parse().unwrap();
        assert!(config.validate().is_err());
        config.service.listen = "8.8.8.8:8789".parse().unwrap();
        assert!(config.validate().is_err());
    }

    #[test]
    fn rejects_unsafe_local_rate_configuration() {
        let mut interval_config = config();
        interval_config.feeder.min_feed_interval_seconds = 0;
        assert!(interval_config.validate().is_err());
        let mut rate_config = config();
        rate_config.feeder.max_feeds_per_hour = 0;
        assert!(rate_config.validate().is_err());
    }
}
