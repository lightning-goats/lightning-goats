use std::{net::IpAddr, str::FromStr, time::Duration};

use anyhow::{Context, Result, bail};
use reqwest::{Certificate, Client, Url};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use zeroize::Zeroizing;

use crate::{
    config::LightningConfig,
    ledger::PaidInvoice,
    secrets::read_systemd_credential,
};

const WAITANYINVOICE_TIMEOUT_CODE: i64 = 904;

#[derive(Clone)]
pub struct ClnRestClient {
    client: Client,
    base_url: Url,
    rune: Zeroizing<String>,
}

impl ClnRestClient {
    pub async fn from_config(config: &LightningConfig) -> Result<Self> {
        let rune = read_systemd_credential("cln-rune").await?;
        let ca_certificate = match &config.clnrest_ca_certificate {
            Some(path) => Some(tokio::fs::read(path).await.with_context(|| {
                format!("failed reading CLNRest CA certificate {}", path.display())
            })?),
            None => None,
        };

        Self::new(&config.clnrest_url, rune, ca_certificate.as_deref())
    }

    pub fn new(base_url: &str, rune: String, ca_certificate_pem: Option<&[u8]>) -> Result<Self> {
        if rune.trim().is_empty() {
            bail!("CLN rune is empty");
        }

        let mut base_url = Url::parse(base_url).context("invalid CLNRest URL")?;
        match base_url.scheme() {
            "http" | "https" => {}
            scheme => bail!("unsupported CLNRest URL scheme: {scheme}"),
        }
        ensure_loopback_url(&base_url)?;
        if !base_url.path().ends_with('/') {
            base_url.set_path(&format!("{}/", base_url.path()));
        }

        let mut builder = Client::builder()
            .https_only(base_url.scheme() == "https")
            .connect_timeout(Duration::from_secs(5));

        if let Some(pem) = ca_certificate_pem {
            let certificate =
                Certificate::from_pem(pem).context("invalid CLNRest CA certificate PEM")?;
            builder = builder.add_root_certificate(certificate);
        }

        let client = builder
            .build()
            .context("failed building CLNRest HTTP client")?;
        Ok(Self {
            client,
            base_url,
            rune: Zeroizing::new(rune),
        })
    }

    pub async fn wait_any_invoice(
        &self,
        last_pay_index: u64,
        timeout_seconds: u64,
    ) -> Result<Option<PaidInvoice>> {
        let endpoint = self
            .base_url
            .join("v1/waitanyinvoice")
            .context("failed constructing waitanyinvoice URL")?;
        let response = self
            .client
            .post(endpoint)
            .header("Rune", self.rune.as_str())
            .timeout(Duration::from_secs(
                timeout_seconds.saturating_add(15).max(15),
            ))
            .json(&WaitAnyInvoiceRequest {
                lastpay_index: last_pay_index,
                timeout: timeout_seconds,
            })
            .send()
            .await
            .context("CLNRest waitanyinvoice request failed")?;

        let status = response.status();
        let body = response
            .bytes()
            .await
            .context("failed reading CLNRest waitanyinvoice response")?;

        if !status.is_success() {
            let value: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
            if rpc_error_code(&value) == Some(WAITANYINVOICE_TIMEOUT_CODE) {
                return Ok(None);
            }
            let message = rpc_error_message(&value).unwrap_or("unknown CLNRest error");
            bail!("CLNRest waitanyinvoice failed with HTTP {status}: {message}");
        }

        let response: WaitAnyInvoiceResponse = serde_json::from_slice(&body)
            .context("invalid CLNRest waitanyinvoice response JSON")?;
        if response.status != "paid" {
            bail!(
                "waitanyinvoice returned unexpected invoice status {}",
                response.status
            );
        }

        Ok(Some(PaidInvoice {
            pay_index: response.pay_index,
            payment_hash: response.payment_hash,
            label: Some(response.label),
            amount_msat: response.amount_received_msat.into_msat()?,
            settled_at: Some(
                i64::try_from(response.paid_at).context("paid_at exceeds i64 range")?,
            ),
        }))
    }
}

fn ensure_loopback_url(url: &Url) -> Result<()> {
    let host = url.host_str().context("CLNRest URL is missing a host")?;
    if host.eq_ignore_ascii_case("localhost") {
        return Ok(());
    }
    let ip = IpAddr::from_str(host).context("CLNRest host must be localhost or a loopback IP")?;
    if !ip.is_loopback() {
        bail!("CLNRest host must be loopback; refusing remote CLN credential transport");
    }
    Ok(())
}

#[derive(Debug, Serialize)]
struct WaitAnyInvoiceRequest {
    lastpay_index: u64,
    timeout: u64,
}

#[derive(Debug, Deserialize)]
struct WaitAnyInvoiceResponse {
    label: String,
    payment_hash: String,
    status: String,
    pay_index: u64,
    amount_received_msat: MsatValue,
    paid_at: u64,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum MsatValue {
    Integer(u64),
    Text(String),
    Object { msat: u64 },
}

impl MsatValue {
    fn into_msat(self) -> Result<u64> {
        match self {
            Self::Integer(value) | Self::Object { msat: value } => Ok(value),
            Self::Text(value) => value
                .strip_suffix("msat")
                .unwrap_or(&value)
                .trim()
                .parse::<u64>()
                .context("invalid millisatoshi amount from CLNRest"),
        }
    }
}

fn rpc_error_code(value: &Value) -> Option<i64> {
    value
        .get("error")
        .and_then(|error| error.get("code"))
        .or_else(|| value.get("code"))
        .and_then(Value::as_i64)
}

fn rpc_error_message(value: &Value) -> Option<&str> {
    value
        .get("error")
        .and_then(|error| error.get("message"))
        .or_else(|| value.get("message"))
        .and_then(Value::as_str)
}

#[cfg(test)]
mod tests {
    use axum::{
        Json, Router,
        extract::State,
        http::{HeaderMap, StatusCode},
        routing::post,
    };
    use serde_json::json;
    use tokio::net::TcpListener;

    use super::*;

    #[derive(Clone)]
    struct MockState {
        rune: &'static str,
    }

    async fn successful_wait(
        State(state): State<MockState>,
        headers: HeaderMap,
        Json(request): Json<Value>,
    ) -> Result<Json<Value>, StatusCode> {
        if headers.get("Rune").and_then(|value| value.to_str().ok()) != Some(state.rune) {
            return Err(StatusCode::UNAUTHORIZED);
        }
        if request.get("lastpay_index").and_then(Value::as_u64) != Some(41) {
            return Err(StatusCode::BAD_REQUEST);
        }
        Ok(Json(json!({
            "label": "clnaddress:v1:herd:550e8400-e29b-41d4-a716-446655440000",
            "payment_hash": "abc123",
            "status": "paid",
            "pay_index": 42,
            "amount_received_msat": "2340000msat",
            "paid_at": 1700000000
        })))
    }

    async fn timeout_wait() -> (StatusCode, Json<Value>) {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({
                "error": {
                    "code": 904,
                    "message": "Timed out while waiting"
                }
            })),
        )
    }

    async fn spawn_mock(handler: Router) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, handler).await.unwrap();
        });
        format!("http://{address}/")
    }

    #[tokio::test]
    async fn parses_paid_invoice_and_sends_rune() {
        let app = Router::new()
            .route("/v1/waitanyinvoice", post(successful_wait))
            .with_state(MockState { rune: "test-rune" });
        let base_url = spawn_mock(app).await;
        let client = ClnRestClient::new(&base_url, "test-rune".to_owned(), None).unwrap();

        let invoice = client.wait_any_invoice(41, 1).await.unwrap().unwrap();
        assert_eq!(invoice.pay_index, 42);
        assert_eq!(invoice.amount_msat, 2_340_000);
        assert_eq!(invoice.payment_hash, "abc123");
    }

    #[tokio::test]
    async fn treats_rpc_904_as_poll_timeout() {
        let app = Router::new().route("/v1/waitanyinvoice", post(timeout_wait));
        let base_url = spawn_mock(app).await;
        let client = ClnRestClient::new(&base_url, "test-rune".to_owned(), None).unwrap();

        assert!(client.wait_any_invoice(41, 1).await.unwrap().is_none());
    }

    #[test]
    fn refuses_non_loopback_clnrest() {
        assert!(
            ClnRestClient::new("https://example.com:3010", "rune".to_owned(), None).is_err()
        );
    }

    #[test]
    fn parses_common_msat_representations() {
        assert_eq!(MsatValue::Integer(1000).into_msat().unwrap(), 1000);
        assert_eq!(
            MsatValue::Text("1000msat".to_owned()).into_msat().unwrap(),
            1000
        );
        assert_eq!(
            MsatValue::Object { msat: 1000 }.into_msat().unwrap(),
            1000
        );
    }
}
