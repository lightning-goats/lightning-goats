use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use reqwest::{Client, Url};
use serde_json::Value;

use super::{client::WeatherSnapshot, store::GatewayStore};

const MAX_WEATHER_BODY: usize = 64 * 1024;

#[derive(Clone)]
pub struct WeatherAdapter {
    client: Client,
    url: Url,
    max_stale: Duration,
    store: GatewayStore,
}

impl WeatherAdapter {
    pub fn new(url: &str, max_stale: Duration, store: GatewayStore) -> Result<Self> {
        if max_stale.is_zero() {
            bail!("weather max stale duration must be greater than zero");
        }
        let url = Url::parse(url).context("invalid trusted weather URL")?;
        if url.scheme() != "http" || url.host_str() != Some("127.0.0.1") {
            bail!("trusted weather URL must use http://127.0.0.1");
        }
        if url.path() != "/get_received_data" || url.query().is_some() || url.fragment().is_some() {
            bail!("trusted weather URL must be exactly /get_received_data without query/fragment");
        }
        let client = Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(2))
            .timeout(Duration::from_secs(3))
            .build()
            .context("failed building trusted weather HTTP client")?;
        Ok(Self {
            client,
            url,
            max_stale,
            store,
        })
    }

    pub async fn fetch(&self) -> Result<WeatherSnapshot> {
        let response = self
            .client
            .get(self.url.clone())
            .header("Accept", "application/json")
            .send()
            .await
            .context("trusted weather read failed")?;
        if !response.status().is_success() {
            bail!("trusted weather returned HTTP {}", response.status());
        }
        let body = crate::http::bounded_body(response, MAX_WEATHER_BODY).await?;
        let value: Value =
            serde_json::from_slice(&body).context("trusted weather JSON is malformed")?;
        let latest = match value {
            Value::Array(items) => {
                let mut latest = None;
                for item in items {
                    let object = item
                        .as_object()
                        .context("weather array entry must be an object")?;
                    let epoch =
                        observation_epoch(&required_bounded_string(object, "dateutc", 64)?)?;
                    if latest
                        .as_ref()
                        .is_none_or(|(previous, _)| epoch > *previous)
                    {
                        latest = Some((epoch, item));
                    }
                }
                latest.context("trusted weather array is empty")?.1
            }
            Value::Object(_) => value,
            _ => bail!("trusted weather response must be an object or non-empty array"),
        };
        let object = latest
            .as_object()
            .context("trusted weather latest entry must be a JSON object")?;

        let observed_at = required_bounded_string(object, "dateutc", 64)?;
        let epoch = validate_observation_time(&observed_at, now_epoch()?, self.max_stale)?;
        let observed_at = chrono::DateTime::from_timestamp(epoch, 0)
            .context("invalid observation time")?
            .to_rfc3339_opts(chrono::SecondsFormat::Secs, true);

        let temperature = required_number(object, "tempf", -100.0, 150.0)?;
        let humidity = required_integer(object, "humidity", 0, 100)?;
        let wind_speed = required_number(object, "windspeedmph", 0.0, 250.0)?;
        let wind_direction = normalize_wind_direction(object.get("winddir"))?;
        let uv_index = required_integer(object, "uv", 0, 50)?;

        let snapshot = WeatherSnapshot {
            observed_at,
            temperature,
            humidity,
            wind_speed,
            wind_direction,
            uv_index,
            apparent_temperature: optional_number(object, &["feelslikef"], -100.0, 150.0)?,
            wind_gust: optional_number(object, &["windgustmph"], 0.0, 300.0)?,
            pressure_relative: optional_number(
                object,
                &["baromrelin", "baromrelinin"],
                20.0,
                40.0,
            )?,
            pressure_trend: optional_bounded_string(object, &["pressuretrend"], 32)?,
            rain_hourly: optional_number(object, &["hourlyrainin"], 0.0, 20.0)?,
            rain_daily: optional_number(object, &["dailyrainin"], 0.0, 100.0)?,
            solar_radiation: optional_number(object, &["solarradiation"], 0.0, 2_000.0)?,
        };
        // Invalid bodies must not advance the persisted high-water mark.
        self.store.accept_weather_time(epoch).await?;
        Ok(snapshot)
    }
}

pub(crate) fn now_epoch() -> Result<i64> {
    Ok(i64::try_from(
        SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs(),
    )?)
}

pub(crate) fn observation_epoch(raw: &str) -> Result<i64> {
    let epoch = if let Ok(time) = chrono::DateTime::parse_from_rfc3339(raw) {
        time.timestamp()
    } else {
        chrono::NaiveDateTime::parse_from_str(raw, "%Y-%m-%d %H:%M:%S")
            .context("weather dateutc must be RFC3339 or YYYY-MM-DD HH:MM:SS UTC")?
            .and_utc()
            .timestamp()
    };
    if epoch < 0 {
        bail!("weather observation precedes Unix epoch");
    }
    Ok(epoch)
}

pub(crate) fn validate_observation_time(raw: &str, now: i64, max_stale: Duration) -> Result<i64> {
    let epoch = observation_epoch(raw)?;
    if epoch.saturating_sub(now) > 30
        || now.saturating_sub(epoch) > i64::try_from(max_stale.as_secs())?
    {
        bail!("weather observation is stale or too far in the future");
    }
    Ok(epoch)
}

pub(crate) fn validate_snapshot(snapshot: &WeatherSnapshot) -> Result<()> {
    // The display never labels observations older than five minutes as current,
    // even if the gateway operator configures a longer local cache tolerance.
    validate_observation_time(
        &snapshot.observed_at,
        now_epoch()?,
        Duration::from_secs(300),
    )?;
    for (value, min, max) in [
        (snapshot.temperature, -100.0, 150.0),
        (snapshot.wind_speed, 0.0, 250.0),
    ] {
        if !value.is_finite() || !(min..=max).contains(&value) {
            bail!("invalid normalized weather value");
        }
    }
    if snapshot.humidity > 100 || snapshot.uv_index > 50 {
        bail!("invalid normalized weather range");
    }
    normalize_wind_direction(Some(&Value::String(snapshot.wind_direction.clone())))?;
    if snapshot.pressure_trend.as_ref().is_some_and(|text| {
        text.trim().is_empty() || text.len() > 32 || text.chars().any(char::is_control)
    }) {
        bail!("invalid normalized weather trend");
    }
    for (value, min, max) in [
        (snapshot.apparent_temperature, -100.0, 150.0),
        (snapshot.wind_gust, 0.0, 300.0),
        (snapshot.pressure_relative, 20.0, 40.0),
        (snapshot.rain_hourly, 0.0, 20.0),
        (snapshot.rain_daily, 0.0, 100.0),
        (snapshot.solar_radiation, 0.0, 2000.0),
    ] {
        if value.is_some_and(|v| !v.is_finite() || !(min..=max).contains(&v)) {
            bail!("invalid optional normalized weather value");
        }
    }
    Ok(())
}

pub fn format_weather_message(weather: &WeatherSnapshot) -> String {
    let mut primary = Vec::new();
    if let Some(apparent) = weather.apparent_temperature {
        primary.push(format!(
            "{:.0}°F (feels like {apparent:.1}°F)",
            weather.temperature
        ));
    } else {
        primary.push(format!("{:.0}°F", weather.temperature));
    }
    primary.push(format!("{}% humidity", weather.humidity));

    let mut wind = format!(
        "{:.0} mph wind from {}",
        weather.wind_speed, weather.wind_direction
    );
    if let Some(gust) = weather.wind_gust
        && gust > weather.wind_speed
    {
        wind.push_str(&format!(", gusts up to {gust:.1} mph"));
    }
    primary.push(wind);
    primary.push(format!("UV index {}", weather.uv_index));

    let mut extras = Vec::new();
    match (weather.pressure_relative, weather.pressure_trend.as_deref()) {
        (Some(pressure), Some(trend)) => {
            extras.push(format!("{} pressure {pressure:.2} inHg", capitalize(trend)))
        }
        (Some(pressure), None) => extras.push(format!("Pressure {pressure:.2} inHg")),
        (None, Some(trend)) => extras.push(format!("{} pressure", capitalize(trend))),
        (None, None) => {}
    }
    if let Some(hourly) = weather.rain_hourly
        && hourly > 0.0
    {
        extras.push(format!("Rain {hourly:.2}\" per hour"));
    } else if let Some(daily) = weather.rain_daily
        && daily > 0.0
    {
        extras.push(format!("Rain today {daily:.2}\""));
    }
    if let Some(solar) = weather.solar_radiation
        && solar > 0.0
    {
        extras.push(format!("Solar {solar:.0} W/m²"));
    }

    let mut message = format!("🌤️ Weather Update: {}", primary.join(", "));
    if !extras.is_empty() {
        message.push_str(". ");
        message.push_str(&extras.join("; "));
    }
    message
}

fn required_bounded_string(
    object: &serde_json::Map<String, Value>,
    key: &str,
    max_len: usize,
) -> Result<String> {
    let value = object
        .get(key)
        .and_then(Value::as_str)
        .with_context(|| format!("weather field {key} must be a string"))?
        .trim();
    if value.is_empty() || value.len() > max_len {
        bail!("weather field {key} has invalid length");
    }
    Ok(value.to_owned())
}

fn optional_bounded_string(
    object: &serde_json::Map<String, Value>,
    keys: &[&str],
    max_len: usize,
) -> Result<Option<String>> {
    for key in keys {
        let Some(value) = object.get(*key) else {
            continue;
        };
        if value.is_null() {
            continue;
        }
        let value = value
            .as_str()
            .with_context(|| format!("weather field {key} must be a string"))?
            .trim();
        if value.is_empty() || value.len() > max_len {
            bail!("weather field {key} has invalid length");
        }
        return Ok(Some(value.to_owned()));
    }
    Ok(None)
}

fn required_number(
    object: &serde_json::Map<String, Value>,
    key: &str,
    min: f64,
    max: f64,
) -> Result<f64> {
    parse_number(
        object
            .get(key)
            .with_context(|| format!("weather field {key} is missing"))?,
        key,
        min,
        max,
    )
}

fn optional_number(
    object: &serde_json::Map<String, Value>,
    keys: &[&str],
    min: f64,
    max: f64,
) -> Result<Option<f64>> {
    for key in keys {
        let Some(value) = object.get(*key) else {
            continue;
        };
        if value.is_null() {
            continue;
        }
        return parse_number(value, key, min, max).map(Some);
    }
    Ok(None)
}

fn parse_number(value: &Value, key: &str, min: f64, max: f64) -> Result<f64> {
    let number = match value {
        Value::Number(number) => number.as_f64(),
        Value::String(string) => string.parse::<f64>().ok(),
        _ => None,
    }
    .with_context(|| format!("weather field {key} must be numeric"))?;
    if !number.is_finite() || !(min..=max).contains(&number) {
        bail!("weather field {key} is out of range");
    }
    Ok(number)
}

fn required_integer(
    object: &serde_json::Map<String, Value>,
    key: &str,
    min: u64,
    max: u64,
) -> Result<u64> {
    let number = required_number(object, key, min as f64, max as f64)?;
    if number.fract() != 0.0 {
        bail!("weather field {key} must be an integer");
    }
    Ok(number as u64)
}

fn normalize_wind_direction(value: Option<&Value>) -> Result<String> {
    let value = value.context("weather field winddir is missing")?;
    if let Some(text) = value.as_str() {
        let text = text.trim();
        if !text.is_empty()
            && text.len() <= 16
            && text
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        {
            if let Ok(degrees) = text.parse::<f64>() {
                return degrees_to_cardinal(degrees);
            }
            return Ok(text.to_owned());
        }
        bail!("weather field winddir has invalid string value");
    }
    let degrees = parse_number(value, "winddir", 0.0, 360.0)?;
    degrees_to_cardinal(degrees)
}

fn degrees_to_cardinal(degrees: f64) -> Result<String> {
    if !degrees.is_finite() || !(0.0..=360.0).contains(&degrees) {
        bail!("weather wind direction is out of range");
    }
    const DIRECTIONS: [&str; 16] = [
        "N", "NNE", "NE", "ENE", "E", "ESE", "SE", "SSE", "S", "SSW", "SW", "WSW", "W", "WNW",
        "NW", "NNW",
    ];
    let normalized = if degrees == 360.0 { 0.0 } else { degrees };
    let index = ((normalized / 22.5) + 0.5).floor() as usize % DIRECTIONS.len();
    Ok(DIRECTIONS[index].to_owned())
}

fn capitalize(value: &str) -> String {
    let mut chars = value.chars();
    match chars.next() {
        Some(first) => format!("{}{}", first.to_uppercase(), chars.as_str()),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn timestamp(epoch: i64) -> String {
        chrono::DateTime::from_timestamp(epoch, 0)
            .unwrap()
            .to_rfc3339()
    }

    fn observation(epoch: i64) -> Value {
        serde_json::json!({"dateutc":timestamp(epoch),"tempf":72,"humidity":25,"windspeedmph":8,"winddir":315,"uv":4,"feelslike":20})
    }

    #[test]
    fn timestamps_use_actual_age_with_bounded_future_skew() {
        let now = 1_789_000_000;
        let max = Duration::from_secs(300);
        for delta in [-300, 0, 30] {
            assert_eq!(
                validate_observation_time(&timestamp(now + delta), now, max).unwrap(),
                now + delta
            );
        }
        for delta in [-301, 31] {
            assert!(validate_observation_time(&timestamp(now + delta), now, max).is_err());
        }
        for bad in ["yesterday", "2026-99-99 99:99:99", "1960-01-01T00:00:00Z"] {
            assert!(validate_observation_time(bad, now, max).is_err());
        }
        assert_eq!(
            observation_epoch("2026-09-10 12:00:00").unwrap(),
            observation_epoch("2026-09-10T14:00:00+02:00").unwrap()
        );
    }

    #[tokio::test]
    async fn weather_rejects_stale_first_read_and_regression_across_reopen() {
        use axum::{Json, Router, routing::get};
        use std::sync::{Arc, Mutex};
        let dir = tempfile::TempDir::new().unwrap();
        let db = format!("sqlite://{}", dir.path().join("weather.db").display());
        let now = now_epoch().unwrap();
        let body = Arc::new(Mutex::new(observation(now - 3600)));
        let app = Router::new().route(
            "/get_received_data",
            get({
                let body = body.clone();
                move || {
                    let body = body.clone();
                    async move { Json(body.lock().unwrap().clone()) }
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!(
            "http://{}/get_received_data",
            listener.local_addr().unwrap()
        );
        let task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let adapter = WeatherAdapter::new(
            &url,
            Duration::from_secs(300),
            GatewayStore::connect(&db).await.unwrap(),
        )
        .unwrap();
        assert!(
            adapter.fetch().await.is_err(),
            "ancient first response cannot get a freshness grace period"
        );
        *body.lock().unwrap() = observation(now + 120);
        assert!(adapter.fetch().await.is_err());
        *body.lock().unwrap() = serde_json::json!([
            observation(now - 20),
            observation(now - 10),
            observation(now - 30)
        ]);
        let weather = adapter.fetch().await.unwrap();
        assert_eq!(
            weather.observed_at,
            timestamp(now - 10).replace("+00:00", "Z")
        );
        assert_eq!(
            weather.apparent_temperature, None,
            "ambiguous feelslike has no declared units"
        );
        let encoded = serde_json::to_value(&weather).unwrap();
        assert_eq!(encoded["temperature_f"], 72.0);
        assert_eq!(encoded["wind_speed_mph"], 8.0);
        assert!(encoded.get("temperature").is_none());
        validate_snapshot(&weather).unwrap();
        let mut stale = weather.clone();
        stale.observed_at = timestamp(now - 3600);
        assert!(validate_snapshot(&stale).is_err());
        drop(adapter);
        let reopened = WeatherAdapter::new(
            &url,
            Duration::from_secs(300),
            GatewayStore::connect(&db).await.unwrap(),
        )
        .unwrap();
        for epoch in [now - 11, now - 3600, now - 11] {
            *body.lock().unwrap() = observation(epoch);
            assert!(reopened.fetch().await.is_err());
        }
        *body.lock().unwrap() = observation(now - 10);
        reopened.fetch().await.unwrap();
        let mut invalid = observation(now + 5);
        invalid["tempf"] = 999.into();
        *body.lock().unwrap() = invalid;
        assert!(reopened.fetch().await.is_err());
        *body.lock().unwrap() = observation(now);
        reopened.fetch().await.unwrap(); // invalid data did not advance high-water mark
        task.abort();
    }

    #[test]
    fn wind_degrees_are_normalized() {
        assert_eq!(degrees_to_cardinal(0.0).unwrap(), "N");
        assert_eq!(degrees_to_cardinal(90.0).unwrap(), "E");
        assert_eq!(degrees_to_cardinal(225.0).unwrap(), "SW");
        assert_eq!(degrees_to_cardinal(360.0).unwrap(), "N");
    }

    #[test]
    fn message_matches_legacy_weather_style() {
        let snapshot = WeatherSnapshot {
            observed_at: "2026-09-07 18:00:00".to_owned(),
            temperature: 72.0,
            humidity: 25,
            wind_speed: 8.0,
            wind_direction: "NW".to_owned(),
            uv_index: 4,
            apparent_temperature: Some(70.5),
            wind_gust: Some(14.2),
            pressure_relative: Some(30.12),
            pressure_trend: Some("rising".to_owned()),
            rain_hourly: Some(0.0),
            rain_daily: Some(0.0),
            solar_radiation: Some(520.0),
        };
        let message = format_weather_message(&snapshot);
        assert!(message.starts_with("🌤️ Weather Update: 72°F (feels like 70.5°F)"));
        assert!(message.contains("25% humidity"));
        assert!(message.contains("gusts up to 14.2 mph"));
        assert!(message.contains("Rising pressure 30.12 inHg"));
        assert!(message.contains("Solar 520 W/m²"));
    }
}
