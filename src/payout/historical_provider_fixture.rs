//! Loopback-only fixture for constructing and replaying historical journal records.
use super::super::funding::tests::SourceVerifier;
use super::*;
use reqwest::{StatusCode, Url};
use std::time::Duration;
use zeroize::Zeroizing;
pub struct Client {
    client: reqwest::Client,
    origin: Url,
    scope: ProviderScope,
    key: Zeroizing<String>,
}

impl Client {
    pub(crate) fn test_client(scope: ProviderScope, key: String, origin: &str) -> Result<Self> {
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
            origin: url,
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

pub(super) fn error_id(
    status: StatusCode,
    raw: &[u8],
    expected: &str,
    property: &str,
) -> Result<Uuid> {
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
#[serde(rename_all = "camelCase")]
struct QuoteWire {
    payment_quote_id: Uuid,
    valid_until: Option<String>,
    #[serde(flatten)]
    money: Money,
}

pub(super) fn parse_fee_probe(
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
pub(super) fn parse_quote(
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
