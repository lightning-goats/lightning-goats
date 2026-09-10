use std::{
    fs,
    net::{IpAddr, SocketAddr},
    path::Path,
    time::Duration,
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
use tokio::time::{Instant, sleep, timeout, timeout_at};
use uuid::Uuid;

use crate::openhab::{OpenHabClient, TrustedOpenHabConfig};

use super::{
    client::{FeedOutcome, FeedRefusal, FeedRequestStatus, FeederSafety, RefusalReason},
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
            store.clone(),
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
    match timeout(
        Duration::from_secs(140),
        submit_feed_request(request_id, state),
    )
    .await
    {
        Ok(response) => response,
        Err(_) => outcome_response(
            StatusCode::GATEWAY_TIMEOUT,
            request_id,
            FeedOutcome::Ambiguous,
        ),
    }
}

async fn submit_feed_request(request_id: Uuid, state: GatewayState) -> Response {
    match state.store.status(request_id).await {
        Ok(Some(status)) => return existing_response(&state, request_id, status).await,
        Ok(None) => {}
        Err(error) => return internal_failure("Unable to read feeder request status", error),
    }
    let safety_refusal = !matches!(state.openhab.feeder_safety().await, Ok((false, true)));
    match state
        .store
        .begin_request(
            request_id,
            state.min_feed_interval,
            state.max_feeds_per_hour,
            safety_refusal,
        )
        .await
    {
        Ok(BeginRequestOutcome::New) => {}
        Ok(BeginRequestOutcome::NotDispatched(refusal)) => {
            return refusal_response(request_id, refusal);
        }
        Ok(BeginRequestOutcome::Acknowledged) => {
            return outcome_response(StatusCode::OK, request_id, FeedOutcome::Confirmed);
        }
        Ok(BeginRequestOutcome::Pending) => {
            return handle_existing_pending(&state, request_id).await;
        }
        Err(error) => return internal_failure("Unable to persist feeder admission", error),
    }
    if let Err(error) = state.openhab.command_feeder_request(request_id).await {
        tracing::warn!(%error, "command outcome ambiguous after durable admission");
        return outcome_response(StatusCode::CONFLICT, request_id, FeedOutcome::Ambiguous);
    }
    wait_for_ack(&state, request_id).await
}

async fn existing_response(
    state: &GatewayState,
    id: Uuid,
    status: StoredRequestStatus,
) -> Response {
    match status {
        StoredRequestStatus::Acknowledged => {
            outcome_response(StatusCode::OK, id, FeedOutcome::Confirmed)
        }
        StoredRequestStatus::Pending => handle_existing_pending(state, id).await,
        StoredRequestStatus::NotDispatched(refusal) => refusal_response(id, refusal),
    }
}

fn outcome_response(code: StatusCode, request_id: Uuid, status: FeedOutcome) -> Response {
    (
        code,
        Json(FeedRequestStatus {
            request_id,
            status,
            refusal: None,
        }),
    )
        .into_response()
}

fn refusal_response(request_id: Uuid, refusal: FeedRefusal) -> Response {
    let code = if refusal.reason == RefusalReason::Capacity {
        StatusCode::TOO_MANY_REQUESTS
    } else {
        StatusCode::LOCKED
    };
    (
        code,
        Json(FeedRequestStatus {
            request_id,
            status: FeedOutcome::NotDispatched,
            refusal: Some(refusal),
        }),
    )
        .into_response()
}

async fn handle_existing_pending(state: &GatewayState, request_id: Uuid) -> Response {
    match ack_matches(state, request_id).await {
        Ok(true) => {
            if let Err(error) = state.store.mark_acknowledged(request_id).await {
                return internal_failure("Unable to persist feeder acknowledgement", error);
            }
            outcome_response(StatusCode::OK, request_id, FeedOutcome::Confirmed)
        }
        Ok(false) => outcome_response(StatusCode::ACCEPTED, request_id, FeedOutcome::Pending),
        Err(error) => internal_failure("OpenHAB feeder acknowledgement unavailable", error),
    }
}

async fn wait_for_ack(state: &GatewayState, request_id: Uuid) -> Response {
    let deadline = Instant::now() + state.ack_timeout;
    loop {
        match timeout_at(deadline, ack_matches(state, request_id)).await {
            Ok(Ok(true)) => {
                if let Err(error) = state.store.mark_acknowledged(request_id).await {
                    return internal_failure("Unable to persist feeder acknowledgement", error);
                }
                return outcome_response(StatusCode::OK, request_id, FeedOutcome::Confirmed);
            }
            Ok(Ok(false)) => {}
            Ok(Err(_)) | Err(_) => {
                return outcome_response(StatusCode::CONFLICT, request_id, FeedOutcome::Ambiguous);
            }
        }
        if Instant::now() >= deadline {
            return outcome_response(
                StatusCode::GATEWAY_TIMEOUT,
                request_id,
                FeedOutcome::Ambiguous,
            );
        }
        sleep(
            state
                .ack_poll
                .min(deadline.saturating_duration_since(Instant::now())),
        )
        .await;
    }
}

async fn feed_request_status(
    AxumPath(request_id): AxumPath<Uuid>,
    State(state): State<GatewayState>,
) -> Response {
    match timeout(
        Duration::from_secs(10),
        lookup_feed_request(request_id, state),
    )
    .await
    {
        Ok(response) => response,
        Err(_) => outcome_response(
            StatusCode::GATEWAY_TIMEOUT,
            request_id,
            FeedOutcome::Ambiguous,
        ),
    }
}

async fn lookup_feed_request(request_id: Uuid, state: GatewayState) -> Response {
    match state.store.status(request_id).await {
        Ok(Some(status)) => existing_response(&state, request_id, status).await,
        Ok(None) => public_error(StatusCode::NOT_FOUND, "Unknown feeder request UUID"),
        Err(error) => internal_failure("Unable to read feeder request status", error),
    }
}

async fn weather(State(state): State<GatewayState>) -> Response {
    match state.weather.fetch().await {
        Ok(weather) => (StatusCode::OK, Json(weather)).into_response(),
        Err(error) => internal_failure("Weather data unavailable", error),
    }
}

async fn ack_matches(state: &GatewayState, request_id: Uuid) -> Result<bool> {
    Ok(state.openhab.acknowledged_request().await? == Some(request_id))
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
