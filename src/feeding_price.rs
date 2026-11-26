//! Read the pinned OpenHAB observation without using the display float for
//! accounting. Only the ledger can use an observation to open a cycle.
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::path::Path;
use tokio::io::AsyncReadExt as _;

const MAX_CACHE_BYTES: u64 = 16_384;
const SATOSHIS_PER_BITCOIN: u128 = 100_000_000;
const MAX_USD_PER_BITCOIN: u128 = 10_000_000;
pub const SOURCE: &str = "OpenHAB BTC_USD_Price";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Observation {
    rate_usd: String,
    observed_at_ms: i64,
    target_sats: u64,
}

impl Observation {
    pub fn from_cache(body: &[u8], now: i64) -> Result<Self> {
        let cache: Cache = serde_json::from_slice(body).context("invalid BTC/USD cache")?;
        cache
            .market
            .context("missing BTC/USD observation")?
            .validate(now)
    }
}

impl Market {
    pub(crate) fn from_openhab(rate: String, observed_at_ms: i64, now: i64) -> Result<Self> {
        let timestamp = chrono::DateTime::from_timestamp_millis(observed_at_ms)
            .context("invalid OpenHAB BTC/USD observation time")?;
        let market = Self {
            source: SOURCE.into(),
            observed_at: timestamp.to_rfc3339(),
            price_usd_exact: rate,
        };
        market.validate(now)?;
        Ok(market)
    }

    fn validate(&self, now: i64) -> Result<Observation> {
        let market = self;
        ensure!(market.source == SOURCE, "unexpected BTC/USD source");
        let observed_at_ms = chrono::DateTime::parse_from_rfc3339(&market.observed_at)
            .context("invalid BTC/USD observation time")?
            .timestamp_millis();
        let target_sats = target_sats(&market.price_usd_exact)?;
        let observation = Observation {
            rate_usd: market.price_usd_exact.clone(),
            observed_at_ms,
            target_sats,
        };
        ensure!(
            observation.is_fresh(now),
            "stale or future BTC/USD observation"
        );
        Ok(observation)
    }
}

impl Observation {
    pub async fn read_cache(path: &Path, now: i64) -> Result<Self> {
        let mut body = Vec::new();
        tokio::fs::File::open(path)
            .await?
            .take(MAX_CACHE_BYTES + 1)
            .read_to_end(&mut body)
            .await?;
        ensure!(
            body.len() as u64 <= MAX_CACHE_BYTES,
            "BTC/USD cache is too large"
        );
        Self::from_cache(&body, now)
    }

    pub fn is_fresh(&self, now: i64) -> bool {
        self.observed_at_ms >= 0
            && (now.saturating_sub(300_000)..=now.saturating_add(30_000))
                .contains(&self.observed_at_ms)
    }

    pub fn rate_usd(&self) -> &str {
        &self.rate_usd
    }

    pub fn observed_at_ms(&self) -> i64 {
        self.observed_at_ms
    }

    pub fn target_sats(&self) -> u64 {
        self.target_sats
    }
}

#[derive(Deserialize)]
struct Cache {
    market: Option<Market>,
}

#[derive(Deserialize, Serialize)]
pub(crate) struct Market {
    source: String,
    observed_at: String,
    price_usd_exact: String,
}

fn target_sats(rate: &str) -> Result<u64> {
    let (whole, fraction) = match rate.split_once('.') {
        Some((whole, fraction)) => {
            ensure!(!fraction.is_empty(), "empty BTC/USD fraction");
            (whole, fraction)
        }
        None => (rate, ""),
    };
    ensure!(
        !whole.is_empty()
            && whole.len() <= 8
            && fraction.len() <= 18
            && whole
                .bytes()
                .chain(fraction.bytes())
                .all(|b| b.is_ascii_digit()),
        "invalid BTC/USD decimal"
    );
    // The bounds above keep both products and their sum below u128::MAX.
    let scale = 10_u128.pow(u32::try_from(fraction.len())?);
    let numerator = whole.parse::<u128>()? * scale
        + if fraction.is_empty() {
            0
        } else {
            fraction.parse::<u128>()?
        };
    ensure!(
        (scale..=MAX_USD_PER_BITCOIN * scale).contains(&numerator),
        "BTC/USD price outside the supported range"
    );
    Ok(u64::try_from(
        (SATOSHIS_PER_BITCOIN * scale).div_ceil(numerator),
    )?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn computes_whole_satoshi_ceiling_without_float_rounding() -> Result<()> {
        assert_eq!(target_sats("80000")?, 1250);
        assert_eq!(target_sats("90000")?, 1112);
        assert_eq!(target_sats("100000")?, 1000);
        assert_eq!(target_sats("79999.999999999999999999")?, 1251);
        assert_eq!(target_sats("80000.000000000000000001")?, 1250);
        for invalid in ["", "0", "-1", "0.99", "10000000.01", "1.", "NaN", "8e4"] {
            assert!(target_sats(invalid).is_err(), "{invalid}");
        }
        Ok(())
    }

    #[test]
    fn requires_a_fresh_observation_from_the_pinned_source() -> Result<()> {
        let cache = br#"{"market":{"source":"OpenHAB BTC_USD_Price","observed_at":"2026-11-26T12:00:00Z","price_usd_exact":"80000"}}"#;
        let now = chrono::DateTime::parse_from_rfc3339("2026-11-26T12:00:00Z")?.timestamp_millis();
        assert_eq!(
            Observation::from_cache(cache, now + 300_000)?.target_sats(),
            1250
        );
        assert_eq!(
            Observation::from_cache(cache, now - 30_000)?.rate_usd(),
            "80000"
        );
        assert!(Observation::from_cache(cache, now + 300_001).is_err());
        assert!(Observation::from_cache(cache, now - 30_001).is_err());
        assert!(Observation::from_cache(br#"{"market":null}"#, now).is_err());
        assert!(Observation::from_cache(br#"{"market":{"price_usd":80000}}"#, now).is_err());
        Ok(())
    }
}
