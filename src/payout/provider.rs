//! Outgoing Strike protocol. Only committed journal capabilities may cross the
//! quote/execute boundary. Provider errors never become public message bodies.
use std::time::Duration;

use anyhow::{Context, Result, bail};
use reqwest::{StatusCode, Url};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;
use zeroize::Zeroizing;

use super::funding::{ProviderScope, SourceVerifier};

pub(crate) const QUOTE_LIFETIME: i64 = 30;
const MAX_BODY: usize = 32_768;

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct QuoteDocument {
    pub id: Uuid,
    pub invoice_id: Uuid,
    pub scope: ProviderScope,
    pub encoded: String,
    pub payment_hash: String,
    pub description_hash: String,
    pub amount_sats: u64,
    pub fee_limit_sats: u64,
    pub gross_sats: u64,
    pub invoice_expires_at: u64,
    pub created_at: i64,
    pub nip57: bool,
}

impl QuoteDocument {
    pub(crate) fn validate(&self) -> Result<()> {
        crate::nostr_event::validate_hex(&self.scope.credential_fingerprint, 32)?;
        if self.id.get_version_num() != 4
            || self.invoice_id.is_nil()
            || self.scope.environment != "strike-production"
            || self.scope.account_id.is_nil()
            || self.created_at < 0
            || self.encoded.len() > 8192
            || self.amount_sats == 0
            || self.amount_sats > i64::MAX as u64 / 1000
            || self.amount_sats.checked_add(self.fee_limit_sats) != Some(self.gross_sats)
            || self.gross_sats > i64::MAX as u64
        {
            bail!("invalid committed payout quote intent");
        }
        let invoice = crate::bolt11::verify(
            &self.encoded,
            self.amount_sats * 1000,
            &self.payment_hash,
            &self.description_hash,
        )
        .map_err(|_| anyhow::anyhow!("committed quote invoice is invalid"))?;
        if invoice.expires_at().map(|at| at.as_secs()) != Some(self.invoice_expires_at) {
            bail!("quote invoice expiry changed");
        }
        Ok(())
    }
}

#[derive(Clone)]
pub struct QuoteIntent(pub(crate) QuoteDocument);
impl QuoteIntent {
    pub fn id(&self) -> Uuid {
        self.0.id
    }
    pub fn invoice_id(&self) -> Uuid {
        self.0.invoice_id
    }
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct QuoteRecord {
    pub request: QuoteDocument,
    pub quote_id: Uuid,
    pub fee_sats: u64,
    pub total_sats: u64,
    pub provider_valid_until: Option<i64>,
    pub usable_until: i64,
    pub observed_at: i64,
}
impl QuoteRecord {
    pub(crate) fn validate(&self) -> Result<()> {
        self.request.validate()?;
        let cap = self
            .request
            .created_at
            .checked_add(QUOTE_LIFETIME)
            .context("quote time overflow")?;
        let expected = cap
            .min(self.request.invoice_expires_at.try_into()?)
            .min(self.provider_valid_until.unwrap_or(i64::MAX));
        if self.quote_id.is_nil()
            || self.fee_sats > self.request.fee_limit_sats
            || self.request.amount_sats.checked_add(self.fee_sats) != Some(self.total_sats)
            || self.total_sats > self.request.gross_sats
            || self.usable_until != expected
            || self.observed_at < self.request.created_at
            || self.usable_until <= self.observed_at
        {
            bail!("invalid frozen provider quote");
        }
        Ok(())
    }
}

#[derive(Clone, Serialize)]
pub struct Quote(pub(crate) QuoteRecord);
impl Quote {
    pub fn quote_id(&self) -> Uuid {
        self.0.quote_id
    }
    pub fn intent_id(&self) -> Uuid {
        self.0.request.id
    }
}

#[derive(Clone)]
pub struct DuplicateQuote {
    pub(crate) request: QuoteDocument,
    pub(crate) quote_id: Uuid,
}
pub enum QuoteReply {
    Ready(Quote),
    Duplicate(DuplicateQuote),
}

/// An authenticated fee-discovery response. This is deliberately not a Quote
/// and cannot be supplied to execute. Conversion to a Quote requires all of
/// the original invoice, gross-budget, fee-allowance and deadline checks.
#[derive(Clone, Serialize)]
pub struct FeeObservation(pub(crate) FeeObservationRecord);

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct FeeObservationRecord {
    pub(crate) request: QuoteDocument,
    pub(crate) quote_id: Uuid,
    pub(crate) fee_sats: u64,
    pub(crate) total_sats: u64,
    pub(crate) provider_valid_until: Option<i64>,
    pub(crate) usable_until: i64,
    pub(crate) observed_at: i64,
}

impl FeeObservation {
    pub fn executable_quote(&self) -> Result<Quote> {
        let record = QuoteRecord {
            request: self.0.request.clone(),
            quote_id: self.0.quote_id,
            fee_sats: self.0.fee_sats,
            total_sats: self.0.total_sats,
            provider_valid_until: self.0.provider_valid_until,
            usable_until: self.0.usable_until,
            observed_at: self.0.observed_at,
        };
        record.validate()?;
        Ok(Quote(record))
    }
    pub fn fee_sats(&self) -> u64 {
        self.0.fee_sats
    }
    pub fn total_sats(&self) -> u64 {
        self.0.total_sats
    }
    pub fn quote_id(&self) -> Uuid {
        self.0.quote_id
    }
}

impl FeeObservationRecord {
    pub(crate) fn validate(&self) -> Result<()> {
        self.request.validate()?;
        let expected = self
            .request
            .created_at
            .checked_add(QUOTE_LIFETIME)
            .context("fee observation deadline overflow")?
            .min(self.request.invoice_expires_at.try_into()?)
            .min(self.provider_valid_until.unwrap_or(i64::MAX));
        if self.quote_id.is_nil()
            || self.total_sats > i64::MAX as u64
            || self.request.amount_sats.checked_add(self.fee_sats) != Some(self.total_sats)
            || self.usable_until != expected
            || self.observed_at < self.request.created_at
            || self.observed_at >= self.usable_until
        {
            bail!("invalid persisted fee observation");
        }
        Ok(())
    }
}

pub(crate) fn restore_fee_observation(raw: &[u8]) -> Result<FeeObservation> {
    let record: FeeObservationRecord = decode(raw)?;
    record.validate()?;
    Ok(FeeObservation(record))
}

pub enum FeeProbeReply {
    Observed(FeeObservation),
    Duplicate(DuplicateQuote),
}

/// The journal stores execute intent first. A fresh root/core observation gives
/// this short lifetime; recovery may only PATCH that same original quote.
#[derive(Clone)]
pub struct ExecuteIntent {
    pub(crate) quote: QuoteRecord,
    pub(crate) permitted_until: i64,
    pub(crate) recovery: bool,
    pub(crate) core_uid: u32,
    pub(crate) request: super::execution::Request,
}
impl ExecuteIntent {
    pub fn claim_request(&self) -> &super::execution::Request {
        &self.request
    }
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct BindingRecord {
    pub quote: QuoteRecord,
    pub payment_id: Uuid,
}

#[derive(Clone)]
pub struct ExecutionObservation(pub(crate) BindingRecord);
#[derive(Clone)]
pub struct BoundPayment(pub(crate) BindingRecord);
impl BoundPayment {
    pub fn payment_id(&self) -> Uuid {
        self.0.payment_id
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum PaymentState {
    Pending,
    Completed,
    Failed,
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct PaymentRecord {
    pub binding: BindingRecord,
    pub state: PaymentState,
    pub fee_sats: u64,
    pub total_sats: u64,
    pub completed_at: Option<i64>,
    pub observed_at: i64,
}
#[derive(Clone, Serialize)]
pub struct PaymentObservation(pub(crate) PaymentRecord);
impl PaymentObservation {
    pub fn state(&self) -> PaymentState {
        self.0.state
    }
}

pub struct Client {
    client: reqwest::Client,
    origin: Url,
    scope: ProviderScope,
    key: Zeroizing<String>,
}

impl Client {
    #[cfg(test)]
    pub(crate) fn test_client(scope: ProviderScope, key: String, origin: &str) -> Result<Self> {
        let mut client = Self::new(scope, key)?;
        let url = Url::parse(origin)?;
        if url.scheme() != "http"
            || !url
                .host_str()
                .and_then(|host| host.parse::<std::net::IpAddr>().ok())
                .is_some_and(|ip| ip.is_loopback())
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
        {
            bail!("test provider must be literal loopback");
        }
        client.origin = url;
        Ok(client)
    }
    pub fn new(scope: ProviderScope, key: String) -> Result<Self> {
        if SourceVerifier::scope(scope.account_id, &scope.credential_fingerprint, &key)? != scope {
            bail!("outgoing credential differs from pinned scope");
        }
        Ok(Self {
            client: reqwest::Client::builder()
                .no_proxy()
                .redirect(reqwest::redirect::Policy::none())
                .connect_timeout(Duration::from_secs(5))
                .timeout(Duration::from_secs(10))
                .build()?,
            origin: Url::parse("https://api.strike.me/")?,
            scope,
            key: Zeroizing::new(key),
        })
    }
    fn scope(&self, doc: &QuoteDocument) -> Result<()> {
        doc.validate()?;
        if doc.scope != self.scope {
            bail!("payout request belongs to another credential/account");
        }
        Ok(())
    }
    async fn send(&self, request: reqwest::RequestBuilder) -> Result<(StatusCode, Vec<u8>)> {
        tokio::time::timeout(Duration::from_secs(12), async {
            let response = request
                .bearer_auth(self.key.as_str())
                .header(reqwest::header::ACCEPT, "application/json")
                .send()
                .await
                .map_err(|_| anyhow::anyhow!("Strike payout response unavailable"))?;
            let status = response.status();
            let json = response
                .headers()
                .get(reqwest::header::CONTENT_TYPE)
                .and_then(|v| v.to_str().ok())
                .is_some_and(|v| {
                    v.split(';')
                        .next()
                        .is_some_and(|v| v.trim().eq_ignore_ascii_case("application/json"))
                });
            if !json {
                bail!("Strike payout response was not JSON");
            }
            let raw = crate::http::bounded_body(response, MAX_BODY)
                .await
                .map_err(|_| anyhow::anyhow!("Strike payout body unavailable or excessive"))?;
            Ok((status, raw))
        })
        .await
        .map_err(|_| anyhow::anyhow!("Strike payout deadline exceeded"))?
    }

    pub async fn create_quote(&self, intent: &QuoteIntent) -> Result<QuoteReply> {
        let (status, raw) = self.request_quote(intent).await?;
        parse_quote(intent, status, &raw, crate::cyberherd::calendar::now()?)
    }

    pub async fn probe_fee(&self, intent: &QuoteIntent) -> Result<FeeProbeReply> {
        let (status, raw) = self.request_quote(intent).await?;
        parse_fee_probe(intent, status, &raw, crate::cyberherd::calendar::now()?)
    }

    async fn request_quote(&self, intent: &QuoteIntent) -> Result<(StatusCode, Vec<u8>)> {
        self.scope(&intent.0)?;
        let now = crate::cyberherd::calendar::now()?;
        if now < intent.0.created_at
            || now >= intent.0.created_at.saturating_add(QUOTE_LIFETIME)
            || u64::try_from(now)? >= intent.0.invoice_expires_at
        {
            bail!("quote request is no longer current");
        }
        let (status, raw) = self
            .send(
                self.client
                    .post(self.origin.join("v1/payment-quotes/lightning")?)
                    .header("idempotency-key", intent.id().to_string())
                    .json(
                        &serde_json::json!({"lnInvoice":intent.0.encoded,"sourceCurrency":"BTC"}),
                    ),
            )
            .await?;
        Ok((status, raw))
    }

    pub async fn execute(
        &self,
        intent: &ExecuteIntent,
        claim: &super::execution::AuthenticatedLease,
    ) -> Result<ExecutionObservation> {
        self.scope(&intent.quote.request)?;
        intent.quote.validate()?;
        let now = crate::cyberherd::calendar::now()?;
        if intent.request.quote_json != serde_json::to_string(&intent.quote)? {
            bail!("execute capability changed claimed quote");
        }
        claim.require(&intent.request, intent.core_uid, now)?;
        if now > intent.permitted_until
            || intent.permitted_until > now.saturating_add(5)
            || (!intent.recovery
                && (now >= intent.quote.usable_until
                    || u64::try_from(now)? >= intent.quote.request.invoice_expires_at))
        {
            bail!("payout execution permission expired");
        }
        let (status, raw) = self
            .send(
                self.client
                    .patch(self.origin.join(&format!(
                        "v1/payment-quotes/{}/execute",
                        intent.quote.quote_id
                    ))?)
                    .header(reqwest::header::CONTENT_LENGTH, 0),
            )
            .await?;
        let payment_id = if matches!(status, StatusCode::OK | StatusCode::ACCEPTED) {
            let wire: PaymentWire = decode(&raw)?;
            validate_payment(&intent.quote, &wire, crate::cyberherd::calendar::now()?)?;
            wire.payment_id
        } else {
            error_id(status, &raw, "PAYMENT_PROCESSED", "paymentId")?
        };
        if payment_id.is_nil() {
            bail!("provider omitted original payment identity");
        }
        Ok(ExecutionObservation(BindingRecord {
            quote: intent.quote.clone(),
            payment_id,
        }))
    }

    pub async fn read_payment(&self, payment: &BoundPayment) -> Result<PaymentObservation> {
        self.scope(&payment.0.quote.request)?;
        if payment.0.payment_id.is_nil() {
            bail!("invalid bound payment identity");
        }
        let (status, raw) = self
            .send(
                self.client.get(
                    self.origin
                        .join(&format!("v1/payments/{}", payment.0.payment_id))?,
                ),
            )
            .await?;
        if status != StatusCode::OK {
            bail!("original payment cannot yet be reconciled");
        }
        let wire: PaymentWire = decode(&raw)?;
        if wire.payment_id != payment.0.payment_id {
            bail!("provider returned another payment identity");
        }
        let now = crate::cyberherd::calendar::now()?;
        let (fee, total, completed) = validate_payment(&payment.0.quote, &wire, now)?;
        Ok(PaymentObservation(PaymentRecord {
            binding: payment.0.clone(),
            state: wire.state,
            fee_sats: fee,
            total_sats: total,
            completed_at: completed,
            observed_at: now,
        }))
    }
}

fn decode<T: serde::de::DeserializeOwned>(raw: &[u8]) -> Result<T> {
    if raw.len() > MAX_BODY {
        bail!("excessive Strike payout JSON");
    }
    serde_json::from_slice(raw).map_err(|_| anyhow::anyhow!("invalid Strike payout JSON"))
}

fn error_id(status: StatusCode, raw: &[u8], expected: &str, property: &str) -> Result<Uuid> {
    #[derive(Deserialize)]
    struct Error {
        data: Details,
    }
    #[derive(Deserialize)]
    struct Details {
        status: u16,
        code: String,
        values: std::collections::BTreeMap<String, serde_json::Value>,
    }
    let error: Error = decode(raw)?;
    if status != StatusCode::UNPROCESSABLE_ENTITY
        || error.data.status != status.as_u16()
        || error.data.code != expected
    {
        bail!("Strike payout outcome is unavailable; original identity retained");
    }
    let id: Uuid = error
        .data
        .values
        .get(property)
        .and_then(serde_json::Value::as_str)
        .context("provider omitted correlation identity")?
        .parse()
        .map_err(|_| anyhow::anyhow!("invalid provider correlation identity"))?;
    if id.is_nil() {
        bail!("invalid provider correlation identity");
    }
    Ok(id)
}

#[derive(Deserialize)]
struct Amount {
    amount: String,
    currency: String,
}
impl Amount {
    fn sats(&self) -> Result<u64> {
        if self.currency != "BTC" || self.amount.trim() != self.amount {
            bail!("payout must remain exact BTC");
        }
        let msat = crate::strike::btc_decimal_to_msat(&self.amount)
            .map_err(|_| anyhow::anyhow!("invalid payout BTC decimal"))?;
        if !msat.is_multiple_of(1000) {
            bail!("payout amount is not whole sats");
        }
        Ok(msat / 1000)
    }
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Conversion {
    amount: String,
    source_currency: String,
    target_currency: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Money {
    amount: Amount,
    total_amount: Amount,
    total_fee: Option<Amount>,
    lightning_network_fee: Option<Amount>,
    conversion_rate: Option<Conversion>,
}
impl Money {
    fn validate(&self, doc: &QuoteDocument, fee_cap: u64) -> Result<(u64, u64)> {
        let (fee, total) = self.observe(doc)?;
        if fee > fee_cap || total > doc.gross_sats {
            bail!("provider payout amounts exceed frozen budget");
        }
        Ok((fee, total))
    }

    fn observe(&self, doc: &QuoteDocument) -> Result<(u64, u64)> {
        let net = self.amount.sats()?;
        let total = self.total_amount.sats()?;
        let fee = total
            .checked_sub(net)
            .context("provider payout fee is negative")?;
        if net != doc.amount_sats
            || total > i64::MAX as u64
            || self
                .total_fee
                .as_ref()
                .map(Amount::sats)
                .transpose()?
                .is_some_and(|v| v != fee)
            || self
                .lightning_network_fee
                .as_ref()
                .map(Amount::sats)
                .transpose()?
                .is_some_and(|v| v > fee)
        {
            bail!("provider payout amounts exceed or contradict frozen terms");
        }
        if let Some(rate) = &self.conversion_rate {
            if rate.source_currency != "BTC"
                || rate.target_currency != "BTC"
                || rate.amount.parse::<u64>().ok() != Some(1)
            {
                // Exact decimal one is accepted too, without a floating point conversion.
                let one = Amount {
                    amount: rate.amount.clone(),
                    currency: "BTC".into(),
                }
                .sats()?;
                if rate.source_currency != "BTC"
                    || rate.target_currency != "BTC"
                    || one != 100_000_000
                {
                    bail!("outgoing rewards cannot perform currency conversion");
                }
            }
        }
        Ok((fee, total))
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct QuoteWire {
    payment_quote_id: Uuid,
    valid_until: Option<String>,
    #[serde(flatten)]
    money: Money,
}

fn parse_fee_probe(
    intent: &QuoteIntent,
    status: StatusCode,
    raw: &[u8],
    now: i64,
) -> Result<FeeProbeReply> {
    intent.0.validate()?;
    if status != StatusCode::OK {
        let quote_id = error_id(status, raw, "DUPLICATE_PAYMENT_QUOTE", "paymentQuoteId")?;
        return Ok(FeeProbeReply::Duplicate(DuplicateQuote {
            request: intent.0.clone(),
            quote_id,
        }));
    }
    let wire: QuoteWire = decode(raw)?;
    let (fee_sats, total_sats) = wire.money.observe(&intent.0)?;
    let until = wire.valid_until.as_deref().map(timestamp).transpose()?;
    let usable_until = intent
        .0
        .created_at
        .checked_add(QUOTE_LIFETIME)
        .context("fee probe time overflow")?
        .min(intent.0.invoice_expires_at.try_into()?)
        .min(until.unwrap_or(i64::MAX));
    if wire.payment_quote_id.is_nil() || now < intent.0.created_at || now >= usable_until {
        bail!("fee discovery quote is invalid or expired");
    }
    let observation = FeeObservationRecord {
        request: intent.0.clone(),
        quote_id: wire.payment_quote_id,
        fee_sats,
        total_sats,
        provider_valid_until: until,
        usable_until,
        observed_at: now,
    };
    observation.validate()?;
    Ok(FeeProbeReply::Observed(FeeObservation(observation)))
}
fn parse_quote(
    intent: &QuoteIntent,
    status: StatusCode,
    raw: &[u8],
    now: i64,
) -> Result<QuoteReply> {
    if status != StatusCode::OK {
        let quote_id = error_id(status, raw, "DUPLICATE_PAYMENT_QUOTE", "paymentQuoteId")?;
        return Ok(QuoteReply::Duplicate(DuplicateQuote {
            request: intent.0.clone(),
            quote_id,
        }));
    }
    let wire: QuoteWire = decode(raw)?;
    let (fee, total) = wire.money.validate(&intent.0, intent.0.fee_limit_sats)?;
    let until = wire.valid_until.as_deref().map(timestamp).transpose()?;
    let usable_until = intent
        .0
        .created_at
        .checked_add(QUOTE_LIFETIME)
        .context("quote time overflow")?
        .min(intent.0.invoice_expires_at.try_into()?)
        .min(until.unwrap_or(i64::MAX));
    let record = QuoteRecord {
        request: intent.0.clone(),
        quote_id: wire.payment_quote_id,
        fee_sats: fee,
        total_sats: total,
        provider_valid_until: until,
        usable_until,
        observed_at: now,
    };
    record.validate()?;
    Ok(QuoteReply::Ready(Quote(record)))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Lightning {
    network_fee: Option<Amount>,
    pre_image: Option<String>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PaymentWire {
    payment_id: Uuid,
    state: PaymentState,
    completed: Option<String>,
    lightning: Option<Lightning>,
    onchain: Option<serde_json::Value>,
    #[serde(flatten)]
    money: Money,
}
pub(crate) fn verify_read_record(
    binding: &BindingRecord,
    wire: PaymentWire,
    now: i64,
) -> Result<PaymentRecord> {
    if wire.payment_id != binding.payment_id {
        bail!("provider returned another payment identity");
    }
    let (fee, total, completed) = validate_payment(&binding.quote, &wire, now)?;
    Ok(PaymentRecord {
        binding: binding.clone(),
        state: wire.state,
        fee_sats: fee,
        total_sats: total,
        completed_at: completed,
        observed_at: now,
    })
}
fn validate_payment(
    quote: &QuoteRecord,
    wire: &PaymentWire,
    now: i64,
) -> Result<(u64, u64, Option<i64>)> {
    quote.validate()?;
    if wire.payment_id.is_nil() || wire.onchain.is_some() || now < quote.observed_at {
        bail!("payment contradicts quoted method/time");
    }
    let (fee, total) = wire.money.validate(&quote.request, quote.fee_sats)?;
    let completed = wire.completed.as_deref().map(timestamp).transpose()?;
    if completed.is_some_and(|at| at < quote.request.created_at || at > now.saturating_add(30)) {
        bail!("invalid provider completion time");
    }
    if let Some(ln) = &wire.lightning {
        if ln
            .network_fee
            .as_ref()
            .map(Amount::sats)
            .transpose()?
            .is_some_and(|v| v > fee)
        {
            bail!("network fee exceeds quoted cost");
        }
        if let Some(preimage) = &ln.pre_image {
            crate::nostr_event::validate_hex(preimage, 32)?;
            if hex::encode(Sha256::digest(hex::decode(preimage)?)) != quote.request.payment_hash {
                bail!("provider preimage belongs to another invoice");
            }
        }
    }
    Ok((fee, total, completed))
}
fn timestamp(value: &str) -> Result<i64> {
    if value.len() > 64 {
        bail!("invalid provider timestamp");
    }
    let at = chrono::DateTime::parse_from_rfc3339(value)
        .map_err(|_| anyhow::anyhow!("invalid provider timestamp"))?
        .timestamp();
    if at < 0 {
        bail!("invalid provider timestamp");
    }
    Ok(at)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn inclusive_fee_discovery_cannot_promote_an_oversized_quote_to_execution() {
        let mut request = intent();
        request.0.gross_sats = 1;
        request.0.fee_limit_sats = 0;
        let id = Uuid::new_v4();
        let raw = serde_json::to_vec(&json!({"paymentQuoteId":id,
            "amount":{"amount":"0.00000001","currency":"BTC"},
            "totalAmount":{"amount":"0.00000002","currency":"BTC"},
            "totalFee":{"amount":"0.00000001","currency":"BTC"}}))
        .unwrap();
        assert!(parse_quote(&request, StatusCode::OK, &raw, 1001).is_err());
        let FeeProbeReply::Observed(observed) =
            parse_fee_probe(&request, StatusCode::OK, &raw, 1001).unwrap()
        else {
            panic!("expected fee evidence");
        };
        assert_eq!(
            (
                observed.quote_id(),
                observed.fee_sats(),
                observed.total_sats()
            ),
            (id, 1, 2)
        );
        assert!(observed.executable_quote().is_err());
        let saved = serde_json::to_vec(&observed).unwrap();
        let restored = restore_fee_observation(&saved).unwrap();
        assert_eq!(
            (
                restored.quote_id(),
                restored.fee_sats(),
                restored.total_sats()
            ),
            (id, 1, 2)
        );
        assert!(restored.executable_quote().is_err());
        let mut corrupt: serde_json::Value = serde_json::from_slice(&saved).unwrap();
        corrupt["fee_sats"] = json!(0);
        assert!(restore_fee_observation(&serde_json::to_vec(&corrupt).unwrap()).is_err());
        let mut extra: serde_json::Value = serde_json::from_slice(&saved).unwrap();
        extra["unexpected"] = json!(true);
        assert!(restore_fee_observation(&serde_json::to_vec(&extra).unwrap()).is_err());
        assert!(parse_fee_probe(&request, StatusCode::OK, &raw, 1030).is_err());
        let mut changed: serde_json::Value = serde_json::from_slice(&raw).unwrap();
        changed["amount"]["amount"] = json!("0.00000002");
        assert!(
            parse_fee_probe(
                &request,
                StatusCode::OK,
                &serde_json::to_vec(&changed).unwrap(),
                1001
            )
            .is_err()
        );
    }
    fn intent() -> QuoteIntent {
        QuoteIntent(QuoteDocument {
            id: Uuid::new_v4(),
            invoice_id: Uuid::new_v4(),
            scope: ProviderScope {
                environment: "strike-production".into(),
                account_id: Uuid::new_v4(),
                credential_fingerprint: "ab".repeat(32),
            },
            encoded: crate::test_invoices::invoice_at(
                1000,
                &"11".repeat(32),
                Duration::from_secs(1000),
            ),
            payment_hash: "22".repeat(32),
            description_hash: "11".repeat(32),
            amount_sats: 1,
            fee_limit_sats: 1,
            gross_sats: 2,
            invoice_expires_at: 1300,
            created_at: 1000,
            nip57: false,
        })
    }
    fn body() -> serde_json::Value {
        json!({"paymentQuoteId":Uuid::new_v4(),"amount":{"amount":"0.00000001000","currency":"BTC"},
            "totalAmount":{"amount":"0.00000002","currency":"BTC"}})
    }
    #[test]
    fn optional_quote_fields_preserve_exact_cost_and_local_not_invented_provider_expiry() {
        let i = intent();
        let mut value = body();
        let QuoteReply::Ready(q) = parse_quote(
            &i,
            StatusCode::OK,
            &serde_json::to_vec(&value).unwrap(),
            1001,
        )
        .unwrap() else {
            panic!()
        };
        assert_eq!(
            (
                q.0.fee_sats,
                q.0.total_sats,
                q.0.usable_until,
                q.0.provider_valid_until
            ),
            (1, 2, 1030, None)
        );
        value["validUntil"] = json!("1970-01-01T00:16:45Z");
        value["totalFee"] = json!({"amount":"0.00000001","currency":"BTC"});
        value["conversionRate"] =
            json!({"amount":"1.00000000","sourceCurrency":"BTC","targetCurrency":"BTC"});
        let QuoteReply::Ready(q) = parse_quote(
            &i,
            StatusCode::OK,
            &serde_json::to_vec(&value).unwrap(),
            1001,
        )
        .unwrap() else {
            panic!()
        };
        assert_eq!(
            (q.0.usable_until, q.0.provider_valid_until),
            (1005, Some(1005))
        );
        assert!(
            parse_quote(
                &i,
                StatusCode::OK,
                &serde_json::to_vec(&value).unwrap(),
                1005
            )
            .is_err()
        );
    }
    #[test]
    fn structured_errors_require_actual_status_code_property_and_original_id() {
        let id = Uuid::new_v4();
        let valid = json!({"data":{"status":422,"code":"PAYMENT_PROCESSED","values":{"paymentId":id,"future":true},"message":"synthetic-private"}});
        assert_eq!(
            error_id(
                StatusCode::UNPROCESSABLE_ENTITY,
                &serde_json::to_vec(&valid).unwrap(),
                "PAYMENT_PROCESSED",
                "paymentId"
            )
            .unwrap(),
            id
        );
        assert!(
            error_id(
                StatusCode::BAD_REQUEST,
                &serde_json::to_vec(&valid).unwrap(),
                "PAYMENT_PROCESSED",
                "paymentId"
            )
            .is_err()
        );
        for change in [
            json!({"status":400,"code":"PAYMENT_PROCESSED","values":{"paymentId":id}}),
            json!({"status":422,"code":"LN_INVOICE_PROCESSED","message":"PAYMENT_PROCESSED synthetic-private","values":{"paymentId":id}}),
            json!({"status":422,"code":"PAYMENT_PROCESSED","values":{"paymentQuoteId":id}}),
        ] {
            let err = error_id(
                StatusCode::UNPROCESSABLE_ENTITY,
                &serde_json::to_vec(&json!({"data":change})).unwrap(),
                "PAYMENT_PROCESSED",
                "paymentId",
            )
            .unwrap_err();
            assert!(!format!("{err:#}").contains("synthetic-private"));
        }
    }
    #[test]
    fn completed_payment_preimage_if_present_must_match_and_onchain_cannot_substitute() {
        let mut i = intent();
        let preimage = [0x33; 32];
        let hash: [u8; 32] = Sha256::digest(preimage).into();
        i.0.payment_hash = hex::encode(hash);
        i.0.encoded = crate::test_invoices::invoice_with_payment_hash(
            1000,
            &i.0.description_hash,
            Duration::from_secs(1000),
            hash,
        );
        let QuoteReply::Ready(q) = parse_quote(
            &i,
            StatusCode::OK,
            &serde_json::to_vec(&body()).unwrap(),
            1001,
        )
        .unwrap() else {
            panic!()
        };
        let mut value = body();
        value.as_object_mut().unwrap().remove("paymentQuoteId");
        value["paymentId"] = json!(Uuid::new_v4());
        value["state"] = json!("COMPLETED");
        value["lightning"] = json!({"preImage":hex::encode(preimage),"networkFee":{"amount":"0.00000001","currency":"BTC"}});
        let wire: PaymentWire = decode(&serde_json::to_vec(&value).unwrap()).unwrap();
        assert!(validate_payment(&q.0, &wire, 1002).is_ok());
        value["lightning"]["preImage"] = json!("44".repeat(32));
        let wire: PaymentWire = decode(&serde_json::to_vec(&value).unwrap()).unwrap();
        assert!(validate_payment(&q.0, &wire, 1002).is_err());
        value.as_object_mut().unwrap().remove("lightning");
        value["onchain"] = json!({});
        let wire: PaymentWire = decode(&serde_json::to_vec(&value).unwrap()).unwrap();
        assert!(validate_payment(&q.0, &wire, 1002).is_err());
    }
}
