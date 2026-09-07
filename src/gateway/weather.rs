use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use anyhow::{Context, Result, bail};
use reqwest::{Client, Url};
use serde_json::Value;

use super::client::WeatherSnapshot;

const MAX_WEATHER_BODY: u64 = 64 * 1024;

#[derive(Clone)]
pub struct WeatherAdapter {
    client: Client,
    url: Url,
    max_stale: Duration,
    freshness: Arc<Mutex<Option<Freshness>>>,
}

struct Freshness {
    observed_at: String,
    first_seen: Instant,
}

impl WeatherAdapter {
    pub fn new(url: &str, max_stale: Duration) -> Result<Self> {
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
            .connect_timeout(Duration::from_secs(2))
            .timeout(Duration::from_secs(3))
            .build()
            .context("failed building trusted weather HTTP client")?;
        Ok(Self {
            client,
            url,
            max_stale,
            freshness: Arc::new(Mutex::new(None)),
        })
    }

    pub async fn fetch(&self) -> Result<WeatherSnapshot> {
        let response = self
            .client
            .get(self.url.clone())
            .header("Accept", "application/json")
            .send()
            .await
            .context("trusted weather read failed")?
            .error_for_status()
            .context("trusted weather read returned an error status")?;
        let length = response
            .content_length()
            .context("trusted weather response must include Content-Length")?;
        if length > MAX_WEATHER_BODY {
            bail!("trusted weather response is too large");
        }
        let body = response
            .bytes()
            .await
            .context("failed reading trusted weather response")?;
        if body.len() as u64 > MAX_WEATHER_BODY {
            bail!("trusted weather response exceeded size limit");
        }
        let value: Value = serde_json::from_slice(&body).context("trusted weather JSON is malformed")?;
        let latest = match value {
            Value::Array(items) => items.into_iter().last().context("trusted weather array is empty")?,
            Value::Object(_) => value,
            _ => bail!("trusted weather response must be an object or non-empty array"),
        };
        let object = latest
            .as_object()
            .context("trusted weather latest entry must be a JSON object")?;

        let observed_at = required_bounded_string(object, "dateutc", 64)?;
        self.check_freshness(&observed_at)?;

        let temperature = required_number(object, "tempf", -100.0, 150.0)?;
        let humidity = required_integer(object, "humidity", 0, 100)?;
        let wind_speed = required_number(object, "windspeedmph", 0.0, 250.0)?;
        let wind_direction = normalize_wind_direction(object.get("winddir"))?;
        let uv_index = required_integer(object, "uv", 0, 50)?;

        Ok(WeatherSnapshot {
            observed_at,
            temperature,
            humidity,
            wind_speed,
            wind_direction,
            uv_index,
            apparent_temperature: optional_number(object, &["feelslikef", "feelslike"], -100.0, 150.0)?,
            wind_gust: optional_number(object, &["windgustmph"], 0.0, 300.0)?,
            pressure_relative: optional_number(object, &["baromrelin", "baromrelinin"], 20.0, 40.0)?,
            pressure_trend: optional_bounded_string(object, &["pressuretrend"], 32)?,
            rain_hourly: optional_number(object, &["hourlyrainin"], 0.0, 20.0)?,
            rain_daily: optional_number(object, &["dailyrainin"], 0.0, 100.0)?,
            solar_radiation: optional_number(object, &["solarradiation"], 0.0, 2_000.0)?,
        })
    }

    fn check_freshness(&self, observed_at: &str) -> Result<()> {
        let now = Instant::now();
        let mut guard = self
            .freshness
            .lock()
            .map_err(|_| anyhow::anyhow!("weather freshness lock poisoned"))?;
        match guard.as_mut() {
            Some(state) if state.observed_at == observed_at => {
                if now.duration_since(state.first_seen) > self.max_stale {
                    bail!("trusted weather observation is stale");
                }
            }
            Some(state) => {
                state.observed_at = observed_at.to_owned();
                state.first_seen = now;
            }
            None => {
                *guard = Some(Freshness {
                    observed_at: observed_at.to_owned(),
                    first_seen: now,
                });
            }
        }
        Ok(())
    }
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
        "N", "NNE", "NE", "ENE", "E", "ESE", "SE", "SSE", "S", "SSW", "SW", "WSW",
        "W", "WNW", "NW", "NNW",
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
