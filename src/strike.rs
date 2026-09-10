use std::{net::IpAddr, str::FromStr, time::Duration};

use anyhow::{Context, Result, bail};
use hmac::{Hmac, Mac};
use reqwest::{Client, StatusCode, Url};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::Sha256;
use uuid::Uuid;
use zeroize::Zeroizing;

use crate::{
    config::StrikeConfig,
    domain::payment::SettledPayment,
    ledger::{LedgerStore, SettlementOutcome, StoredStrikeReceiveRequest},
    secrets::read_systemd_credential,
};

type HmacSha256 = Hmac<Sha256>;

const STRIKE_SOURCE: &str = "strike";
const COMPLETED_RECEIVE_EVENT: &str = "receive-request.receive-completed";

#[derive(Clone)]
pub struct StrikeClient {
    client: Client,
    base_url: Url,
    api_key: Zeroizing<String>,
}

#[derive(Clone)]
pub struct StrikeWebhookVerifier {
    secret: Zeroizing<String>,
}

#[derive(Clone)]
pub struct StrikeRuntime {
    client: StrikeClient,
    verifier: StrikeWebhookVerifier,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreatedStrikeReceiveRequest {
    pub receive_request_id: Uuid,
    pub invoice: String,
    pub payment_hash: String,
    pub description_hash: String,
    pub created_provider: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StrikeCompletedReceiveEvent {
    pub event_id: Uuid,
    pub receive_request_id: Uuid,
    pub receive_id: Uuid,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReconciledStrikeReceive {
    pub receive_request_id: Uuid,
    pub receive_id: Uuid,
    pub receive_type: String,
    pub amount_msat: u64,
    pub payment_hash: Option<String>,
    pub completed_provider: Option<String>,
}

impl StrikeClient {
    pub async fn from_config(config: &StrikeConfig) -> Result<Self> {
        let api_key = read_systemd_credential("strike-api-key").await?;
        Self::new(&config.api_url, api_key)
    }

    pub fn new(base_url: &str, api_key: String) -> Result<Self> {
        if api_key.trim().is_empty() {
            bail!("Strike API key is empty");
        }
        let mut base_url = Url::parse(base_url).context("invalid Strike API URL")?;
        validate_provider_url(&base_url)?;
        if !base_url.path().ends_with('/') {
            base_url.set_path(&format!("{}/", base_url.path()));
        }
        if base_url.query().is_some() || base_url.fragment().is_some() {
            bail!("Strike API base URL must not contain a query or fragment");
        }

        let client = Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(10))
            .build()
            .context("failed building Strike HTTP client")?;

        Ok(Self {
            client,
            base_url,
            api_key: Zeroizing::new(api_key),
        })
    }

    pub async fn create_bolt11_receive_request(
        &self,
        amount_msat: u64,
        description_hash: &str,
        expiry_seconds: u64,
    ) -> Result<CreatedStrikeReceiveRequest> {
        if amount_msat == 0 {
            bail!("Strike receive amount must be greater than zero");
        }
        validate_hex32(description_hash, "Strike descriptionHash")?;
        if expiry_seconds == 0 {
            bail!("Strike receive expiry must be greater than zero");
        }

        let endpoint = self
            .base_url
            .join("v1/receive-requests")
            .context("failed constructing Strike receive-request URL")?;
        let body = CreateReceiveRequestBody {
            bolt11: CreateBolt11Request {
                amount: StrikeAmount {
                    amount: msat_to_btc_decimal(amount_msat),
                    currency: "BTC",
                },
                description_hash: description_hash.to_ascii_lowercase(),
                expiry_in_seconds: expiry_seconds,
            },
            target_currency: "BTC",
        };

        let response: ReceiveRequestResponse = self
            .send_json(
                self.client.post(endpoint).json(&body),
                "create receive request",
            )
            .await?;
        validate_receive_request_response(&response, amount_msat, description_hash)?;

        let bolt11 = response
            .bolt11
            .context("Strike create receive request omitted bolt11 response")?;
        Ok(CreatedStrikeReceiveRequest {
            receive_request_id: response.receive_request_id,
            invoice: bolt11.invoice,
            payment_hash: bolt11.payment_hash.to_ascii_lowercase(),
            description_hash: bolt11
                .description_hash
                .unwrap_or_else(|| description_hash.to_ascii_lowercase()),
            created_provider: response.created,
        })
    }

    pub async fn find_receive_request(&self, id: Uuid) -> Result<ReceiveRequestResponse> {
        let endpoint = self
            .base_url
            .join(&format!("v1/receive-requests/{id}"))
            .context("failed constructing Strike receive-request lookup URL")?;
        self.send_json(self.client.get(endpoint), "find receive request")
            .await
    }

    pub async fn find_receive(
        &self,
        receive_request_id: Uuid,
        receive_id: Uuid,
    ) -> Result<StrikeReceive> {
        let mut endpoint = self
            .base_url
            .join(&format!(
                "v1/receive-requests/{receive_request_id}/receives"
            ))
            .context("failed constructing Strike receives URL")?;
        endpoint
            .query_pairs_mut()
            .append_pair("$receiveId", &receive_id.to_string())
            .append_pair("$top", "2");

        let page: StrikeReceivePage = self
            .send_json(self.client.get(endpoint), "find completed receive")
            .await?;
        let mut matches = page
            .items
            .into_iter()
            .filter(|item| item.receive_id == receive_id);
        let receive = matches
            .next()
            .context("Strike did not return the referenced receiveId")?;
        if matches.next().is_some() {
            bail!("Strike returned duplicate rows for one receiveId");
        }
        Ok(receive)
    }

    async fn receive_page(&self, id: Uuid, offset: u32) -> Result<Vec<StrikeReceive>> {
        let mut endpoint = self
            .base_url
            .join(&format!("v1/receive-requests/{id}/receives"))?;
        endpoint
            .query_pairs_mut()
            .append_pair("$skip", &offset.to_string())
            .append_pair("$top", "100");
        let page: StrikeReceivePage = self
            .send_json(self.client.get(endpoint), "scan receives")
            .await?;
        if page.items.len() > 100 || page.items.iter().any(|r| r.receive_request_id != id) {
            bail!("invalid receive scan page");
        }
        Ok(page.items)
    }

    pub async fn reconcile_completed_receive(
        &self,
        stored: &StoredStrikeReceiveRequest,
        receive_id: Uuid,
    ) -> Result<ReconciledStrikeReceive> {
        let request = self.find_receive_request(stored.receive_request_id).await?;
        validate_stored_request(&request, stored)?;

        let receive = self
            .find_receive(stored.receive_request_id, receive_id)
            .await?;
        if receive.receive_request_id != stored.receive_request_id {
            bail!("Strike receive points to the wrong receive request");
        }
        if receive.state != "COMPLETED" {
            bail!("Strike receive is not completed (state={})", receive.state);
        }
        if receive.amount_received.currency != "BTC" {
            bail!(
                "Strike completed receive amount is not denominated in BTC ({})",
                receive.amount_received.currency
            );
        }
        let amount_msat = btc_decimal_to_msat(&receive.amount_received.amount)
            .context("invalid Strike completed BTC amount")?;
        if amount_msat != stored.amount_msat {
            bail!(
                "Strike completed receive amount mismatch: expected {} msat, got {amount_msat}",
                stored.amount_msat
            );
        }

        let payment_hash = match receive.receive_type.as_str() {
            "LIGHTNING" => {
                let lightning = receive
                    .lightning
                    .as_ref()
                    .context("Strike LIGHTNING receive omitted lightning details")?;
                validate_hex32(&lightning.payment_hash, "Strike receive paymentHash")?;
                if !lightning
                    .payment_hash
                    .eq_ignore_ascii_case(&stored.payment_hash)
                {
                    bail!("Strike completed receive payment hash does not match issued invoice");
                }
                if lightning.invoice != stored.invoice {
                    bail!("Strike completed receive invoice does not match issued invoice");
                }
                if let Some(hash) = lightning.description_hash.as_deref()
                    && !hash.eq_ignore_ascii_case(&stored.description_hash)
                {
                    bail!(
                        "Strike completed receive description hash does not match issued invoice"
                    );
                }
                Some(lightning.payment_hash.to_ascii_lowercase())
            }
            "P2P" => None,
            other => bail!("unexpected Strike receive type {other}; refusing feeder credit"),
        };

        Ok(ReconciledStrikeReceive {
            receive_request_id: stored.receive_request_id,
            receive_id,
            receive_type: receive.receive_type,
            amount_msat,
            payment_hash,
            completed_provider: receive.completed,
        })
    }

    async fn send_json<T: for<'de> Deserialize<'de>>(
        &self,
        request: reqwest::RequestBuilder,
        operation: &str,
    ) -> Result<T> {
        let response = request
            .bearer_auth(self.api_key.as_str())
            .header("Accept", "application/json")
            .send()
            .await
            .with_context(|| format!("Strike {operation} request failed"))?;
        let status = response.status();
        let body = crate::http::bounded_body(response, 256 * 1024).await?;
        if !status.is_success() {
            if status == StatusCode::TOO_MANY_REQUESTS {
                bail!("Strike {operation} rate limited (HTTP 429)");
            }
            bail!("Strike {operation} failed with HTTP {status}");
        }
        serde_json::from_slice(&body)
            .with_context(|| format!("Strike {operation} returned malformed JSON"))
    }
}

impl StrikeWebhookVerifier {
    pub async fn from_systemd_credential() -> Result<Self> {
        let secret = read_systemd_credential("strike-webhook-secret").await?;
        Self::new(secret)
    }

    pub fn new(secret: String) -> Result<Self> {
        if secret.is_empty() {
            bail!("Strike webhook secret is empty");
        }
        Ok(Self {
            secret: Zeroizing::new(secret),
        })
    }

    pub fn verify_signature(&self, body: &[u8], signature_hex: &str) -> Result<()> {
        let signature = hex::decode(signature_hex.trim())
            .context("Strike X-Webhook-Signature is not valid hex")?;
        if signature.len() != 32 {
            bail!("Strike X-Webhook-Signature must be 32 bytes");
        }
        let mut mac = HmacSha256::new_from_slice(self.secret.as_bytes())
            .context("failed constructing Strike webhook HMAC")?;
        mac.update(body);
        mac.verify_slice(&signature)
            .context("Strike webhook signature verification failed")
    }

    pub fn parse_completed_event(&self, body: &[u8]) -> Result<StrikeCompletedReceiveEvent> {
        let event: WebhookEvent =
            serde_json::from_slice(body).context("malformed Strike webhook JSON")?;
        if event.event_type != COMPLETED_RECEIVE_EVENT {
            bail!("unsupported Strike webhook event type {}", event.event_type);
        }
        if event.webhook_version != "v1" {
            bail!(
                "unsupported Strike webhook version {}",
                event.webhook_version
            );
        }
        Ok(StrikeCompletedReceiveEvent {
            event_id: event.id,
            receive_request_id: event.data.entity_id,
            receive_id: event.data.receive_id,
        })
    }
}

impl StrikeRuntime {
    pub async fn from_config(config: &StrikeConfig) -> Result<Self> {
        Ok(Self {
            client: StrikeClient::from_config(config).await?,
            verifier: StrikeWebhookVerifier::from_systemd_credential().await?,
        })
    }

    pub fn new(client: StrikeClient, verifier: StrikeWebhookVerifier) -> Self {
        Self { client, verifier }
    }

    pub fn verify_webhook_signature(&self, body: &[u8], signature_hex: &str) -> Result<()> {
        self.verifier.verify_signature(body, signature_hex)
    }

    pub fn parse_completed_event(&self, body: &[u8]) -> Result<StrikeCompletedReceiveEvent> {
        self.verifier.parse_completed_event(body)
    }

    /// One bounded inbox item plus one scan page. Every path converges on the
    /// same authoritative reconciliation and atomic settlement transaction.
    pub async fn recovery_step(&self, ledger: &LedgerStore) -> Result<()> {
        if let Some(work) = ledger.due_strike_work().await? {
            match self.reconcile_and_credit(ledger, &work.event).await {
                Ok(_) => ledger.finish_strike_work(&work.key).await?,
                Err(error) => {
                    tracing::warn!(%error,"Strike durable work retained for retry");
                    ledger.retry_strike_work(&work.key, work.attempts).await?;
                }
            }
        }
        if let Some((stored, offset, attempts)) = ledger.due_strike_scan().await? {
            match self
                .client
                .receive_page(stored.receive_request_id, offset)
                .await
            {
                Ok(page) => {
                    let next = if page.len() == 100 && offset < 1_000_000 {
                        offset + 100
                    } else {
                        0
                    };
                    for receive in page {
                        if receive.state == "COMPLETED" {
                            let event = StrikeCompletedReceiveEvent {
                                event_id: receive.receive_id,
                                receive_request_id: stored.receive_request_id,
                                receive_id: receive.receive_id,
                            };
                            ledger
                                .enqueue_strike_work(
                                    &format!("recovery:{}", receive.receive_id),
                                    &event,
                                )
                                .await?;
                        }
                    }
                    // Full rescans are essential: offset pagination is not a
                    // stable provider snapshot and notifications can be absent.
                    ledger
                        .record_strike_scan(
                            stored.receive_request_id,
                            next,
                            0,
                            if next == 0 { 300 } else { 1 },
                        )
                        .await?;
                }
                Err(error) => {
                    tracing::warn!(%error,"Strike scan retained for retry");
                    ledger
                        .record_strike_scan(
                            stored.receive_request_id,
                            offset,
                            attempts.saturating_add(1).min(30),
                            (2_i64.pow(attempts.min(8))).min(300),
                        )
                        .await?;
                }
            }
        }
        Ok(())
    }

    pub async fn run_recovery_worker(self, ledger: LedgerStore) -> Result<()> {
        loop {
            if let Err(error) = self.recovery_step(&ledger).await {
                tracing::error!(%error,"Strike recovery will retry after storage failure");
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    }

    pub async fn create_and_record_receive_request(
        &self,
        ledger: &LedgerStore,
        address_user: &str,
        credit_pool: &str,
        amount_msat: u64,
        description_hash: &str,
        expiry_seconds: u64,
    ) -> Result<CreatedStrikeReceiveRequest> {
        crate::domain::invoice::validate_user(address_user)?;
        crate::domain::invoice::validate_user(credit_pool)?;
        let created = self
            .client
            .create_bolt11_receive_request(amount_msat, description_hash, expiry_seconds)
            .await?;
        ledger
            .record_strike_receive_request(&StoredStrikeReceiveRequest {
                receive_request_id: created.receive_request_id,
                address_user: address_user.to_owned(),
                credit_pool: credit_pool.to_owned(),
                amount_msat,
                description_hash: created.description_hash.clone(),
                payment_hash: created.payment_hash.clone(),
                invoice: created.invoice.clone(),
                created_provider: created.created_provider.clone(),
            })
            .await?;
        Ok(created)
    }

    pub async fn reconcile_and_credit(
        &self,
        ledger: &LedgerStore,
        event: &StrikeCompletedReceiveEvent,
    ) -> Result<SettlementOutcome> {
        let stored = ledger
            .strike_receive_request(event.receive_request_id)
            .await?
            .with_context(|| {
                format!(
                    "unknown Strike receiveRequestId {}; refusing credit",
                    event.receive_request_id
                )
            })?;
        let receive = self
            .client
            .reconcile_completed_receive(&stored, event.receive_id)
            .await?;
        let context_json = json!({
            "receive_request_id": receive.receive_request_id,
            "receive_type": receive.receive_type,
            "completed_provider": receive.completed_provider
        })
        .to_string();
        ledger
            .record_payment(&SettledPayment {
                source: STRIKE_SOURCE.to_owned(),
                source_id: receive.receive_id.to_string(),
                payment_hash: receive.payment_hash,
                address_user: stored.address_user,
                credit_pool: stored.credit_pool,
                amount_msat: receive.amount_msat,
                settled_at: None,
                context_json: Some(context_json),
            })
            .await
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CreateReceiveRequestBody<'a> {
    bolt11: CreateBolt11Request<'a>,
    target_currency: &'a str,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CreateBolt11Request<'a> {
    amount: StrikeAmount<'a>,
    description_hash: String,
    expiry_in_seconds: u64,
}

#[derive(Debug, Serialize)]
struct StrikeAmount<'a> {
    amount: String,
    currency: &'a str,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReceiveRequestResponse {
    receive_request_id: Uuid,
    #[serde(default)]
    created: Option<String>,
    #[serde(default)]
    target_currency: Option<String>,
    #[serde(default)]
    bolt11: Option<Bolt11ReceiveRequest>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Bolt11ReceiveRequest {
    invoice: String,
    #[serde(default)]
    requested_amount: Option<ProviderAmount>,
    #[serde(default)]
    btc_amount: Option<String>,
    #[serde(default)]
    description_hash: Option<String>,
    payment_hash: String,
}

#[derive(Debug, Clone, Deserialize)]
struct ProviderAmount {
    amount: String,
    currency: String,
}

#[derive(Debug, Deserialize)]
struct StrikeReceivePage {
    #[serde(default)]
    items: Vec<StrikeReceive>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StrikeReceive {
    receive_id: Uuid,
    receive_request_id: Uuid,
    #[serde(rename = "type")]
    receive_type: String,
    state: String,
    amount_received: ProviderAmount,
    #[serde(default)]
    completed: Option<String>,
    #[serde(default)]
    lightning: Option<LightningReceive>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LightningReceive {
    invoice: String,
    #[serde(default)]
    description_hash: Option<String>,
    payment_hash: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WebhookEvent {
    id: Uuid,
    event_type: String,
    webhook_version: String,
    data: WebhookEventData,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WebhookEventData {
    entity_id: Uuid,
    receive_id: Uuid,
}

fn validate_provider_url(url: &Url) -> Result<()> {
    if url.host_str().is_none() {
        bail!("Strike API URL is missing a host");
    }
    if !url.username().is_empty() || url.password().is_some() {
        bail!("Strike API credentials must not be embedded in the URL");
    }
    if url.scheme() == "https" {
        return Ok(());
    }
    if url.scheme() == "http" {
        let host = url.host_str().context("Strike API URL is missing a host")?;
        if host.eq_ignore_ascii_case("localhost") {
            return Ok(());
        }
        if IpAddr::from_str(host).is_ok_and(|ip| ip.is_loopback()) {
            return Ok(());
        }
    }
    bail!("Strike API URL must use HTTPS (HTTP is allowed only for loopback tests)")
}

fn validate_receive_request_response(
    response: &ReceiveRequestResponse,
    amount_msat: u64,
    expected_description_hash: &str,
) -> Result<()> {
    if response
        .target_currency
        .as_deref()
        .is_some_and(|currency| currency != "BTC")
    {
        bail!("Strike receive request targetCurrency is not BTC");
    }
    let bolt11 = response
        .bolt11
        .as_ref()
        .context("Strike receive request omitted bolt11")?;
    if bolt11.invoice.is_empty() || bolt11.invoice.len() > 8_192 {
        bail!("Strike returned an invalid BOLT11 invoice length");
    }
    validate_hex32(&bolt11.payment_hash, "Strike paymentHash")?;
    if let Some(hash) = bolt11.description_hash.as_deref() {
        validate_hex32(hash, "Strike descriptionHash")?;
        if !hash.eq_ignore_ascii_case(expected_description_hash) {
            bail!("Strike returned a different descriptionHash than requested");
        }
    }
    if let Some(amount) = bolt11.requested_amount.as_ref() {
        if amount.currency != "BTC" {
            bail!("Strike requestedAmount currency is not BTC");
        }
        let returned = btc_decimal_to_msat(&amount.amount)
            .context("invalid Strike requestedAmount BTC value")?;
        if returned != amount_msat {
            bail!("Strike requestedAmount does not match requested millisatoshis");
        }
    }
    if let Some(amount) = bolt11.btc_amount.as_deref() {
        let returned = btc_decimal_to_msat(amount).context("invalid Strike btcAmount")?;
        if returned != amount_msat {
            bail!("Strike btcAmount does not match requested millisatoshis");
        }
    }
    Ok(())
}

fn validate_stored_request(
    response: &ReceiveRequestResponse,
    stored: &StoredStrikeReceiveRequest,
) -> Result<()> {
    if response.receive_request_id != stored.receive_request_id {
        bail!("Strike receive request lookup returned the wrong ID");
    }
    validate_receive_request_response(response, stored.amount_msat, &stored.description_hash)?;
    let bolt11 = response
        .bolt11
        .as_ref()
        .context("Strike receive request omitted bolt11")?;
    if bolt11.invoice != stored.invoice {
        bail!("Strike authoritative invoice differs from stored issued invoice");
    }
    if !bolt11
        .payment_hash
        .eq_ignore_ascii_case(&stored.payment_hash)
    {
        bail!("Strike authoritative payment hash differs from stored issued invoice");
    }
    Ok(())
}

fn validate_hex32(value: &str, field: &str) -> Result<()> {
    if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        bail!("{field} must be a 32-byte hex string");
    }
    Ok(())
}

fn msat_to_btc_decimal(msat: u64) -> String {
    const MSAT_PER_BTC: u64 = 100_000_000_000;
    let whole = msat / MSAT_PER_BTC;
    let fraction = msat % MSAT_PER_BTC;
    format!("{whole}.{fraction:011}")
}

fn btc_decimal_to_msat(value: &str) -> Result<u64> {
    let value = value.trim();
    if value.is_empty() || value.starts_with('-') || value.starts_with('+') {
        bail!("BTC amount must be an unsigned decimal string");
    }
    let mut parts = value.split('.');
    let whole = parts.next().context("BTC amount is empty")?;
    let fraction = parts.next();
    if parts.next().is_some() || whole.is_empty() || !whole.bytes().all(|b| b.is_ascii_digit()) {
        bail!("BTC amount has invalid decimal syntax");
    }
    let whole = whole
        .parse::<u64>()
        .context("BTC whole amount is too large")?;
    let fraction = fraction.unwrap_or("");
    if fraction.len() > 11 || !fraction.bytes().all(|b| b.is_ascii_digit()) {
        bail!("BTC amount has more than 11 decimal places or invalid digits");
    }
    let mut fraction_padded = fraction.to_owned();
    fraction_padded.extend(std::iter::repeat_n('0', 11 - fraction.len()));
    let fraction = if fraction_padded.is_empty() {
        0
    } else {
        fraction_padded
            .parse::<u64>()
            .context("BTC fractional amount is invalid")?
    };
    whole
        .checked_mul(100_000_000_000)
        .and_then(|base| base.checked_add(fraction))
        .context("BTC amount exceeds millisatoshi range")
}

#[cfg(test)]
mod tests {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    use axum::{
        Json, Router,
        extract::{Query, State},
        http::{HeaderMap, StatusCode},
        response::IntoResponse,
        routing::{get, post},
    };
    use serde_json::{Value, json};
    use tempfile::TempDir;
    use tokio::net::TcpListener;

    use super::*;

    #[derive(Clone)]
    struct MockState {
        api_key: &'static str,
        receive_request_id: Uuid,
        receive_id: Uuid,
        payment_hash: String,
        description_hash: String,
        invoice: String,
        amount_btc: String,
        receive_state: &'static str,
        receive_type: &'static str,
        api_calls: Arc<AtomicUsize>,
        unavailable: Arc<AtomicUsize>,
        page_padding: bool,
    }

    async fn create_handler(
        State(state): State<MockState>,
        headers: HeaderMap,
        Json(body): Json<Value>,
    ) -> impl IntoResponse {
        state.api_calls.fetch_add(1, Ordering::SeqCst);
        if headers
            .get("authorization")
            .and_then(|value| value.to_str().ok())
            != Some(&format!("Bearer {}", state.api_key))
        {
            return (StatusCode::UNAUTHORIZED, Json(json!({"error":"auth"})));
        }
        if body["targetCurrency"] != "BTC" || body["bolt11"]["amount"]["currency"] != "BTC" {
            return (StatusCode::BAD_REQUEST, Json(json!({"error":"currency"})));
        }
        (
            StatusCode::CREATED,
            Json(json!({
                "receiveRequestId": state.receive_request_id,
                "created": "2026-09-07T17:00:00Z",
                "targetCurrency": "BTC",
                "bolt11": {
                    "invoice": state.invoice,
                    "requestedAmount": {"amount": state.amount_btc, "currency":"BTC"},
                    "btcAmount": state.amount_btc,
                    "descriptionHash": state.description_hash,
                    "paymentHash": state.payment_hash
                }
            })),
        )
    }

    async fn request_handler(State(state): State<MockState>) -> Json<Value> {
        state.api_calls.fetch_add(1, Ordering::SeqCst);
        Json(json!({
            "receiveRequestId": state.receive_request_id,
            "created": "2026-09-07T17:00:00Z",
            "targetCurrency": "BTC",
            "bolt11": {
                "invoice": state.invoice,
                "requestedAmount": {"amount": state.amount_btc, "currency":"BTC"},
                "btcAmount": state.amount_btc,
                "descriptionHash": state.description_hash,
                "paymentHash": state.payment_hash
            }
        }))
    }

    #[derive(Deserialize)]
    struct ReceiveQuery {
        #[serde(rename = "$receiveId")]
        receive_id: Option<Uuid>,
        #[serde(rename = "$skip", default)]
        skip: u32,
    }

    async fn receive_handler(
        State(state): State<MockState>,
        Query(query): Query<ReceiveQuery>,
    ) -> axum::response::Response {
        state.api_calls.fetch_add(1, Ordering::SeqCst);
        let failure = state.unavailable.load(Ordering::SeqCst);
        if failure != 0 {
            return StatusCode::from_u16(failure as u16)
                .unwrap()
                .into_response();
        }
        let mut items = if query.receive_id.is_none() || query.receive_id == Some(state.receive_id)
        {
            let lightning = (state.receive_type == "LIGHTNING").then(|| {
                json!({
                    "invoice": state.invoice,
                    "preimage": "33".repeat(32),
                    "descriptionHash": state.description_hash,
                    "paymentHash": state.payment_hash
                })
            });
            vec![json!({
                "receiveId": state.receive_id,
                "receiveRequestId": state.receive_request_id,
                "type": state.receive_type,
                "state": state.receive_state,
                "amountReceived": {"amount": state.amount_btc, "currency":"BTC"},
                "completed": "2026-09-07T17:01:00Z",
                "lightning": lightning
            })]
        } else {
            Vec::new()
        };
        if state.page_padding && query.receive_id.is_none() && query.skip == 0 {
            let mut pending = items[0].clone();
            pending["state"] = "PENDING".into();
            items = vec![pending; 100];
        }
        Json(json!({"items":items,"count":items.len(),"isCountUnknown":state.page_padding}))
            .into_response()
    }

    async fn spawn_mock(state: MockState) -> String {
        let request_path = format!("/v1/receive-requests/{}", state.receive_request_id);
        let receives_path = format!("{request_path}/receives");
        let app = Router::new()
            .route("/v1/receive-requests", post(create_handler))
            .route(&request_path, get(request_handler))
            .route(&receives_path, get(receive_handler))
            .with_state(state);
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        format!("http://{address}/")
    }

    fn state() -> MockState {
        MockState {
            api_key: "receive-only-test-key",
            receive_request_id: Uuid::parse_str("0191382f-387c-4eec-bc74-980872bfc5e5").unwrap(),
            receive_id: Uuid::parse_str("24180fae-a62d-4583-a960-759d605d252b").unwrap(),
            payment_hash: "22".repeat(32),
            description_hash: "11".repeat(32),
            invoice: "lnbc2340n1strike-test".to_owned(),
            amount_btc: "0.00002340000".to_owned(),
            receive_state: "COMPLETED",
            receive_type: "LIGHTNING",
            api_calls: Arc::new(AtomicUsize::new(0)),
            unavailable: Arc::new(AtomicUsize::new(0)),
            page_padding: false,
        }
    }

    async fn ledger() -> (TempDir, LedgerStore) {
        let directory = TempDir::new().unwrap();
        let path = directory.path().join("strike.db");
        let ledger = LedgerStore::connect(&format!("sqlite://{}", path.display()))
            .await
            .unwrap();
        (directory, ledger)
    }

    fn signed_body(secret: &str, body: &[u8]) -> String {
        let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).unwrap();
        mac.update(body);
        hex::encode(mac.finalize().into_bytes())
    }

    fn webhook_body(state: &MockState) -> Vec<u8> {
        serde_json::to_vec(&json!({
            "id":"2aa19d2a-22eb-4868-8fad-8ad765491c3b",
            "eventType":COMPLETED_RECEIVE_EVENT,
            "webhookVersion":"v1",
            "data": {
                "entityId":state.receive_request_id,
                "receiveId":state.receive_id
            },
            "created":"2026-09-07T17:01:00Z"
        }))
        .unwrap()
    }

    #[test]
    fn btc_amount_conversion_is_exact() {
        assert_eq!(msat_to_btc_decimal(2_340_000), "0.00002340000");
        assert_eq!(btc_decimal_to_msat("0.00002340").unwrap(), 2_340_000);
        assert_eq!(btc_decimal_to_msat("1").unwrap(), 100_000_000_000);
        assert!(btc_decimal_to_msat("1.000000000001").is_err());
        assert!(btc_decimal_to_msat("1e-8").is_err());
    }

    #[test]
    fn verifies_webhook_hmac_and_rejects_changes() {
        let verifier = StrikeWebhookVerifier::new("secret".to_owned()).unwrap();
        let body = br#"{"eventType":"receive-request.receive-completed"}"#;
        let signature = signed_body("secret", body);
        verifier.verify_signature(body, &signature).unwrap();
        assert!(verifier.verify_signature(b"different", &signature).is_err());
    }

    #[tokio::test]
    async fn create_persist_reconcile_credit_and_replay_is_idempotent() {
        let state = state();
        let base_url = spawn_mock(state.clone()).await;
        let runtime = StrikeRuntime::new(
            StrikeClient::new(&base_url, state.api_key.to_owned()).unwrap(),
            StrikeWebhookVerifier::new("webhook-secret".to_owned()).unwrap(),
        );
        let (_directory, ledger) = ledger().await;

        let created = runtime
            .create_and_record_receive_request(
                &ledger,
                "dexter",
                "herd",
                2_340_000,
                &state.description_hash,
                300,
            )
            .await
            .unwrap();
        assert_eq!(created.receive_request_id, state.receive_request_id);

        let body = webhook_body(&state);
        let signature = signed_body("webhook-secret", &body);
        runtime.verify_webhook_signature(&body, &signature).unwrap();
        let event = runtime.parse_completed_event(&body).unwrap();
        assert_eq!(
            runtime.reconcile_and_credit(&ledger, &event).await.unwrap(),
            SettlementOutcome::Credited {
                sats: 2_340,
                address_user: "dexter".to_owned(),
                credit_pool: "herd".to_owned()
            }
        );
        assert_eq!(
            runtime.reconcile_and_credit(&ledger, &event).await.unwrap(),
            SettlementOutcome::Duplicate
        );
        assert_eq!(ledger.feed_credit_sats().await.unwrap(), 2_340);
        assert_eq!(ledger.events_after(0, 10).await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn invalid_signature_causes_no_provider_or_ledger_activity() {
        let state = state();
        let calls = Arc::clone(&state.api_calls);
        let base_url = spawn_mock(state.clone()).await;
        let runtime = StrikeRuntime::new(
            StrikeClient::new(&base_url, state.api_key.to_owned()).unwrap(),
            StrikeWebhookVerifier::new("webhook-secret".to_owned()).unwrap(),
        );
        let (_directory, ledger) = ledger().await;
        let body = webhook_body(&state);
        assert!(
            runtime
                .verify_webhook_signature(&body, &"00".repeat(32))
                .is_err()
        );
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        assert_eq!(ledger.feed_credit_sats().await.unwrap(), 0);
    }

    #[tokio::test]
    async fn incomplete_receive_fails_closed_without_credit() {
        let mut state = state();
        state.receive_state = "PENDING";
        let base_url = spawn_mock(state.clone()).await;
        let runtime = StrikeRuntime::new(
            StrikeClient::new(&base_url, state.api_key.to_owned()).unwrap(),
            StrikeWebhookVerifier::new("secret".to_owned()).unwrap(),
        );
        let (_directory, ledger) = ledger().await;
        runtime
            .create_and_record_receive_request(
                &ledger,
                "herd",
                "herd",
                2_340_000,
                &state.description_hash,
                300,
            )
            .await
            .unwrap();
        let event = StrikeCompletedReceiveEvent {
            event_id: Uuid::new_v4(),
            receive_request_id: state.receive_request_id,
            receive_id: state.receive_id,
        };
        assert!(runtime.reconcile_and_credit(&ledger, &event).await.is_err());
        assert_eq!(ledger.feed_credit_sats().await.unwrap(), 0);
    }

    #[tokio::test]
    async fn accepts_strike_optimized_p2p_receive_without_fake_payment_hash() {
        let mut state = state();
        state.receive_type = "P2P";
        let base_url = spawn_mock(state.clone()).await;
        let client = StrikeClient::new(&base_url, state.api_key.to_owned()).unwrap();
        let stored = StoredStrikeReceiveRequest {
            receive_request_id: state.receive_request_id,
            address_user: "nova".to_owned(),
            credit_pool: "herd".to_owned(),
            amount_msat: 2_340_000,
            description_hash: state.description_hash.clone(),
            payment_hash: state.payment_hash.clone(),
            invoice: state.invoice.clone(),
            created_provider: None,
        };
        let receive = client
            .reconcile_completed_receive(&stored, state.receive_id)
            .await
            .unwrap();
        assert_eq!(receive.receive_type, "P2P");
        assert_eq!(receive.payment_hash, None);
    }
    #[tokio::test]
    async fn recovery_without_notifications_paginates_and_credits_dotted_user_once() {
        let mut state = state();
        state.page_padding = true;
        let runtime = StrikeRuntime::new(
            StrikeClient::new(&spawn_mock(state.clone()).await, state.api_key.into()).unwrap(),
            StrikeWebhookVerifier::new("secret".into()).unwrap(),
        );
        let (directory, ledger) = ledger().await;
        runtime
            .create_and_record_receive_request(
                &ledger,
                "goat.name",
                "herd",
                2_340_000,
                &state.description_hash,
                300,
            )
            .await
            .unwrap();
        runtime.recovery_step(&ledger).await.unwrap();
        assert_eq!(ledger.feed_credit_sats().await.unwrap(), 0);
        let pool = sqlx::SqlitePool::connect(&format!(
            "sqlite://{}",
            directory.path().join("strike.db").display()
        ))
        .await
        .unwrap();
        let offset: i64 = sqlx::query_scalar("SELECT page_offset FROM strike_recovery_scan")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(offset, 100);
        sqlx::query("UPDATE strike_recovery_scan SET next_attempt=0")
            .execute(&pool)
            .await
            .unwrap();
        runtime.recovery_step(&ledger).await.unwrap();
        assert!(ledger.due_strike_work().await.unwrap().is_some());
        // Reopen the same durable store after discovery, before processing.
        let restored = LedgerStore::connect(&format!(
            "sqlite://{}",
            directory.path().join("strike.db").display()
        ))
        .await
        .unwrap();
        runtime.recovery_step(&restored).await.unwrap();
        assert_eq!(restored.feed_credit_sats().await.unwrap(), 2340);
        let event = runtime
            .parse_completed_event(&webhook_body(&state))
            .unwrap();
        restored.enqueue_strike_event(&event).await.unwrap();
        restored.enqueue_strike_event(&event).await.unwrap();
        runtime.recovery_step(&restored).await.unwrap();
        assert_eq!(restored.feed_credit_sats().await.unwrap(), 2340);
        let events = restored.events_after(0, 100).await.unwrap();
        assert_eq!(events.len(), 1);
        assert!(events[0].payload_json.contains("goat.name"));
    }

    #[tokio::test]
    async fn persisted_inbox_survives_provider_outage_and_post_credit_crash() {
        let state = state();
        let runtime = StrikeRuntime::new(
            StrikeClient::new(&spawn_mock(state.clone()).await, state.api_key.into()).unwrap(),
            StrikeWebhookVerifier::new("secret".into()).unwrap(),
        );
        let (directory, ledger) = ledger().await;
        runtime
            .create_and_record_receive_request(
                &ledger,
                "herd",
                "herd",
                2_340_000,
                &state.description_hash,
                300,
            )
            .await
            .unwrap();
        let event = runtime
            .parse_completed_event(&webhook_body(&state))
            .unwrap();
        ledger.enqueue_strike_event(&event).await.unwrap();
        let mut conflict = event.clone();
        conflict.receive_id = Uuid::new_v4();
        assert!(ledger.enqueue_strike_event(&conflict).await.is_err());
        let pool = sqlx::SqlitePool::connect(&format!(
            "sqlite://{}",
            directory.path().join("strike.db").display()
        ))
        .await
        .unwrap();
        // Simulate exhausted short retries during an extended outage.
        sqlx::query("UPDATE strike_inbox SET attempts=7")
            .execute(&pool)
            .await
            .unwrap();
        for status in [429, 503] {
            state.unavailable.store(status, Ordering::SeqCst);
            runtime.recovery_step(&ledger).await.unwrap();
            assert_eq!(ledger.feed_credit_sats().await.unwrap(), 0);
            let status: String = sqlx::query_scalar(
                "SELECT status FROM strike_inbox WHERE work_key LIKE 'webhook:%'",
            )
            .fetch_one(&pool)
            .await
            .unwrap();
            assert_eq!(status, "quarantined");
            sqlx::query("UPDATE strike_inbox SET next_attempt=0")
                .execute(&pool)
                .await
                .unwrap();
        }
        state.unavailable.store(0, Ordering::SeqCst);
        // Commit settlement, then simulate a crash before marking the inbox done.
        runtime.reconcile_and_credit(&ledger, &event).await.unwrap();
        assert!(ledger.due_strike_work().await.unwrap().is_some());
        let reopened = LedgerStore::connect(&format!(
            "sqlite://{}",
            directory.path().join("strike.db").display()
        ))
        .await
        .unwrap();
        runtime.recovery_step(&reopened).await.unwrap();
        assert_eq!(reopened.feed_credit_sats().await.unwrap(), 2340);
        assert_eq!(reopened.events_after(0, 100).await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn invalid_direct_issuance_names_fail_before_provider_contact() {
        let state = state();
        let runtime = StrikeRuntime::new(
            StrikeClient::new(&spawn_mock(state.clone()).await, state.api_key.into()).unwrap(),
            StrikeWebhookVerifier::new("secret".into()).unwrap(),
        );
        let (_directory, ledger) = ledger().await;
        for user in [
            "Herd",
            "../goat",
            "goat%2ename",
            "goat:one",
            "goat name",
            "🐐",
            "",
        ] {
            assert!(
                runtime
                    .create_and_record_receive_request(
                        &ledger,
                        user,
                        "herd",
                        1000,
                        &state.description_hash,
                        300
                    )
                    .await
                    .is_err()
            );
        }
        assert_eq!(state.api_calls.load(Ordering::SeqCst), 0);
    }
}
