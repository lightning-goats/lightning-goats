//! Candidate read-only Kraken XMR/XBT oracle. An API adapter is not permission
//! to trade or a claim of deployment availability/data licensing. No API key.
//! Use the newest nonempty CLOSED one-minute candle, never the unfinished last
//! row or a fresh local timestamp substituted for old market observations.

use super::{MAX_RATE_EVIDENCE_BYTES, QuoteClock, RateEvidence, RateProvider, SystemQuoteClock};
use crate::domain::credit::{SATS_PER_BTC, XmrBtcRate};
use anyhow::{Context, Result, bail};
use futures_util::future::BoxFuture;
use serde::Deserialize;
use std::time::Duration;

pub const KRAKEN_SOURCE: &str = "kraken-xmrbtc-closed-vwap-v1";
const ENDPOINT: &str = "https://api.kraken.com/0/public/OHLC?pair=XMRXBT&interval=1";

#[derive(Clone)]
pub struct KrakenOhlcOracle {
    client: reqwest::Client,
    endpoint: reqwest::Url,
}
impl KrakenOhlcOracle {
    pub fn new() -> Result<Self> {
        Ok(Self {
            client: reqwest::Client::builder()
                .https_only(true)
                .no_proxy()
                .redirect(reqwest::redirect::Policy::none())
                .connect_timeout(Duration::from_secs(3))
                .timeout(Duration::from_secs(10))
                .user_agent("lightning-goats-quote/1")
                .build()?,
            endpoint: ENDPOINT.parse()?,
        })
    }
    async fn request(&self) -> Result<RateEvidence> {
        let response = self
            .client
            .get(self.endpoint.clone())
            .header(reqwest::header::ACCEPT, "application/json")
            .send()
            .await
            .map_err(|_| anyhow::anyhow!("rate provider transport unavailable"))?;
        // Reject redirects and errors WITHOUT reading/logging an upstream error body.
        if response.status() != reqwest::StatusCode::OK {
            bail!("rate provider did not return success");
        }
        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|h| h.to_str().ok())
            .unwrap_or("");
        if !content_type
            .split(';')
            .next()
            .unwrap_or("")
            .trim()
            .eq_ignore_ascii_case("application/json")
        {
            bail!("rate provider returned non-JSON content");
        }
        let bytes = crate::http::bounded_body(response, MAX_RATE_EVIDENCE_BYTES)
            .await
            .map_err(|_| anyhow::anyhow!("rate response exceeded transport limits"))?;
        let text = String::from_utf8(bytes).context("rate response is not UTF-8")?;
        parse_kraken_ohlc(text, SystemQuoteClock.now()?)
    }
}
impl RateProvider for KrakenOhlcOracle {
    fn source_id(&self) -> &str {
        KRAKEN_SOURCE
    }
    fn fetch(&self) -> BoxFuture<'_, Result<RateEvidence>> {
        Box::pin(self.request())
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope {
    error: Vec<String>,
    result: OhlcResult,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OhlcResult {
    #[serde(rename = "XXMRXXBT")]
    candles: Vec<Candle>,
    last: i64,
}
// Tuple deserialization rejects missing/extra fields and numeric price values.
#[derive(Deserialize)]
struct Candle(i64, String, String, String, String, String, String, u64);

/// Exact bounded decimal -> rational. Used for MONEY, never f64/scientific notation.
fn decimal(value: &str) -> Result<(u64, u64)> {
    if value.is_empty() || value.len() > 48 {
        bail!("decimal length outside bounds");
    }
    let (whole, fraction) = value.split_once('.').unwrap_or((value, ""));
    if whole.is_empty()
        || whole.len() > 20
        || (value.contains('.') && fraction.is_empty())
        || fraction.len() > 18
        || !whole
            .bytes()
            .chain(fraction.bytes())
            .all(|b| b.is_ascii_digit())
    {
        bail!("invalid decimal representation");
    }
    let n = whole
        .bytes()
        .chain(fraction.bytes())
        .try_fold(0_u128, |n, b| {
            n.checked_mul(10)
                .and_then(|n| n.checked_add(u128::from(b - b'0')))
        })
        .context("decimal overflow")?;
    let d = 10_u128.pow(fraction.len() as u32);
    let gcd = gcd(n, d);
    Ok((
        u64::try_from(n / gcd).context("decimal numerator overflow")?,
        u64::try_from(d / gcd)?,
    ))
}
fn gcd(mut a: u128, mut b: u128) -> u128 {
    while b != 0 {
        (a, b) = (b, a % b);
    }
    a
}
fn cmp(a: (u64, u64), b: (u64, u64)) -> std::cmp::Ordering {
    (u128::from(a.0) * u128::from(b.1)).cmp(&(u128::from(b.0) * u128::from(a.1)))
}

pub fn btc_per_xmr_decimal_rate(price: &str, source: &str, observed_at: i64) -> Result<XmrBtcRate> {
    let (n, d) = decimal(price)?;
    if n == 0 {
        bail!("zero market price");
    }
    let n = u128::from(n) * u128::from(SATS_PER_BTC);
    let g = gcd(n, u128::from(d));
    XmrBtcRate::new(
        u64::try_from(n / g).context("sats-rate numerator overflow")?,
        u64::try_from(u128::from(d) / g)?,
        source,
        observed_at,
    )
}

/// A parser, not authentication. Production callers use the fixed HTTPS oracle.
pub fn parse_kraken_ohlc(raw_json: String, fetched_at: i64) -> Result<RateEvidence> {
    if raw_json.len() > MAX_RATE_EVIDENCE_BYTES || fetched_at < 0 {
        bail!("rate response bounds exceeded");
    }
    let response: Envelope = serde_json::from_str(&raw_json)
        .map_err(|_| anyhow::anyhow!("unexpected Kraken OHLC schema"))?;
    if !response.error.is_empty()
        || response.result.last < 0
        || response.result.candles.len() < 2
        || response.result.candles.len() > 720
    {
        bail!("Kraken did not return usable closed candles");
    }
    let candles = &response.result.candles;
    let mut previous = None;
    for candle in candles {
        if candle.0 < 0
            || candle.0 % 60 != 0
            || candle.0 > fetched_at
            || previous.is_some_and(|p| p >= candle.0)
        {
            bail!("invalid candle chronology");
        }
        previous = Some(candle.0);
    }
    for candle in candles[..candles.len() - 1].iter().rev() {
        let volume = decimal(&candle.6)?;
        if candle.7 == 0 && volume.0 == 0 {
            continue;
        }
        if candle.7 == 0 || volume.0 == 0 {
            bail!("inconsistent candle volume/count");
        }
        if candle.0.checked_add(60).is_none_or(|end| end > fetched_at) {
            bail!("candle is not closed");
        }
        let open = decimal(&candle.1)?;
        let high = decimal(&candle.2)?;
        let low = decimal(&candle.3)?;
        let close = decimal(&candle.4)?;
        let vwap = decimal(&candle.5)?;
        if low.0 == 0
            || cmp(low, high).is_gt()
            || [open, close, vwap]
                .iter()
                .any(|p| cmp(*p, low).is_lt() || cmp(*p, high).is_gt())
        {
            bail!("inconsistent candle prices");
        }
        // Candle start conservatively bounds the age of EVERY trade in its VWAP.
        let rate = btc_per_xmr_decimal_rate(&candle.5, KRAKEN_SOURCE, candle.0)?;
        return RateEvidence::new(rate, fetched_at, raw_json);
    }
    bail!("no nonempty closed candle available")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn fixture() -> serde_json::Value {
        json!({"error":[],"result":{"XXMRXXBT":[[900,"0.0025","0.003","0.002","0.0026","0.0025","1.2",2],[960,"999","999","999","999","999","1",1]],"last":900}})
    }
    #[test]
    fn parses_exact_direct_rate_and_ignores_unfinished_last_candle() {
        let r = parse_kraken_ohlc(fixture().to_string(), 980).unwrap();
        assert_eq!(r.rate().ratio(), (250_000, 1));
        assert_eq!(r.rate().observed_at(), 900);
        assert_eq!(
            btc_per_xmr_decimal_rate("0.000000000001", KRAKEN_SOURCE, 0)
                .unwrap()
                .ratio(),
            (1, 10000)
        );
    }
    #[test]
    fn decimal_parser_rejects_ambiguous_nonfinite_and_overflow_money() {
        for value in [
            "",
            "-1",
            "+1",
            " 1",
            "1 ",
            "1e-3",
            "NaN",
            "Infinity",
            ".1",
            "1.",
            "1.2.3",
            "0",
            "0.0000000000000000001",
            "99999999999999999999999999999999999999",
            "١",
        ] {
            assert!(
                btc_per_xmr_decimal_rate(value, KRAKEN_SOURCE, 0).is_err(),
                "{value}"
            );
        }
    }
    #[test]
    fn rejects_wrong_pair_duplicated_fields_numeric_prices_and_errors() {
        let source = fixture().to_string();
        for text in [
            source.replace("XXMRXXBT", "XXBTZUSD"),
            source.replace("\"error\":[]", "\"error\":[],\"error\":[]"),
            source.replace("\"0.0025\"", "0.0025"),
            source.replace("\"error\":[]", "\"error\":[\"EGeneral\"]"),
        ] {
            assert!(parse_kraken_ohlc(text, 980).is_err());
        }
    }
    #[test]
    fn chronological_and_price_inconsistency_fails_closed() {
        for (index, value) in [
            (0, json!(961)),
            (0, json!(-60)),
            (2, json!("0.001")),
            (5, json!("0.1")),
            (6, json!("0")),
        ] {
            let mut f = fixture();
            f["result"]["XXMRXXBT"][0][index] = value;
            assert!(parse_kraken_ohlc(f.to_string(), 980).is_err());
        }
        assert!(parse_kraken_ohlc(fixture().to_string(), 950).is_err());
    }
    #[test]
    fn empty_closed_candles_never_become_fresh_rate_evidence() {
        let mut f = fixture();
        f["result"]["XXMRXXBT"][0][7] = json!(0);
        f["result"]["XXMRXXBT"][0][6] = json!("0");
        assert!(parse_kraken_ohlc(f.to_string(), 980).is_err());
        assert!(
            parse_kraken_ohlc(fixture().to_string(), 10_000)
                .unwrap()
                .rate()
                .quote(1000, 10000, 10100, 300)
                .is_err()
        );
    }
    #[tokio::test]
    async fn transport_rejects_redirects_errors_types_sizes_and_stalls() {
        use axum::{Router, body::Body, response::Response, routing::get};
        use std::sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        };
        let target = Arc::new(AtomicUsize::new(0));
        let calls = target.clone();
        let app = Router::new()
            .route(
                "/redirect",
                get(|| async {
                    Response::builder()
                        .status(302)
                        .header("location", "/target")
                        .body(Body::empty())
                        .unwrap()
                }),
            )
            .route(
                "/target",
                get(move || {
                    let calls = calls.clone();
                    async move {
                        calls.fetch_add(1, Ordering::SeqCst);
                        "unexpected"
                    }
                }),
            )
            .route(
                "/limited",
                get(|| async {
                    Response::builder()
                        .status(429)
                        .body(Body::from("private upstream error"))
                        .unwrap()
                }),
            )
            .route("/html", get(|| async { "<html>not json</html>" }))
            .route(
                "/large",
                get(|| async {
                    Response::builder()
                        .header("content-type", "application/json")
                        .body(Body::from_stream(futures_util::stream::iter(vec![
                            Ok::<_, std::convert::Infallible>(vec![b'x'; MAX_RATE_EVIDENCE_BYTES]),
                            Ok(vec![b'x'; 1]),
                        ])))
                        .unwrap()
                }),
            )
            .route(
                "/stall",
                get(|| async {
                    tokio::time::sleep(Duration::from_secs(2)).await;
                    "late"
                }),
            )
            .route("/ok", get(|| async { axum::Json(fixture()) }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        for path in ["redirect", "limited", "html", "large", "stall"] {
            let oracle = KrakenOhlcOracle {
                client: reqwest::Client::builder()
                    .no_proxy()
                    .redirect(reqwest::redirect::Policy::none())
                    .timeout(Duration::from_millis(200))
                    .build()
                    .unwrap(),
                endpoint: format!("http://{addr}/{path}").parse().unwrap(),
            };
            let error = oracle.request().await.err().unwrap();
            assert!(!format!("{error:#}").contains("private upstream error"));
        }
        assert_eq!(target.load(Ordering::SeqCst), 0);
        let oracle = KrakenOhlcOracle {
            client: reqwest::Client::builder()
                .no_proxy()
                .redirect(reqwest::redirect::Policy::none())
                .timeout(Duration::from_secs(1))
                .build()
                .unwrap(),
            endpoint: format!("http://{addr}/ok").parse().unwrap(),
        };
        assert_eq!(oracle.request().await.unwrap().rate().ratio(), (250_000, 1));
        server.abort();
    }
}
