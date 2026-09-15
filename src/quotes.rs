//! Inactive-by-default XMR quote service. No wallet calls, credit grants or HTTP
//! routes are installed here. The #98 boundary must authenticate callers and
//! derive client_bucket itself; a quote UUID is not an authorization capability.

pub mod oracle;

use anyhow::{Context, Result, bail};
use futures_util::future::BoxFuture;
use serde::{Deserialize, Serialize};
use std::{
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::sync::Semaphore;
use uuid::Uuid;

use crate::{
    domain::credit::{MAX_ACCOUNTING_UNITS, PICONERO_PER_XMR, XmrBtcRate, XmrQuote},
    ledger::{LedgerStore, XmrCreditIntent, XmrNetwork},
};

pub const MAX_RATE_EVIDENCE_BYTES: usize = 256 * 1024;
pub(crate) const RESERVATION_SECONDS: i64 = 30;
const SOURCE_DEADLINE: Duration = Duration::from_secs(10);

/// All monetary and admission limits are explicit operator policy, not defaults.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QuotePolicy {
    pub min_target_sats: u64,
    pub max_target_sats: u64,
    pub max_credit_sats: u64,
    pub max_piconero: u64,
    pub lifetime_seconds: u64,
    pub max_rate_age_seconds: u64,
    pub min_sats_per_xmr: u64,
    pub max_sats_per_xmr: u64,
    pub admission_window_seconds: u64,
    pub max_requests_per_window: u32,
    pub max_requests_per_client_window: u32,
    pub max_pending_requests: u32,
    pub max_retained_requests: u32,
}
impl QuotePolicy {
    pub fn validate(&self) -> Result<()> {
        if self.min_target_sats == 0
            || self.min_target_sats > self.max_target_sats
            || self.max_target_sats > self.max_credit_sats
            || self.max_credit_sats > MAX_ACCOUNTING_UNITS
            || self.max_piconero == 0
            || self.max_piconero > MAX_ACCOUNTING_UNITS
            || self.min_sats_per_xmr == 0
            || self.min_sats_per_xmr > self.max_sats_per_xmr
            || self.max_sats_per_xmr > MAX_ACCOUNTING_UNITS
            || self.lifetime_seconds == 0
            || self.lifetime_seconds > 86_400
            || self.max_rate_age_seconds == 0
            || self.max_rate_age_seconds > 86_400
            || self.admission_window_seconds == 0
            || self.admission_window_seconds > 86_400
            || self.max_requests_per_client_window == 0
            || self.max_requests_per_window < self.max_requests_per_client_window
            || self.max_pending_requests == 0
            || self.max_pending_requests > self.max_requests_per_window
            || self.max_retained_requests < self.max_requests_per_window
        {
            bail!("invalid XMR quote policy");
        }
        Ok(())
    }
}

/// Internal, trusted request. Never accept client_bucket from a browser payload.
#[derive(Clone)]
pub struct QuoteRequest {
    pub id: Uuid,
    pub client_bucket: String,
    pub address_user: String,
    pub target_sats: u64,
}
#[derive(Clone)]
pub struct QuoteContext {
    pub provider: String,
    pub account_scope: String,
    pub network: XmrNetwork,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Identity {
    id: Uuid,
    client_bucket: String,
    address_user: String,
    target_sats: u64,
    provider: String,
    account_scope: String,
    network: String,
}
impl Identity {
    fn new(request: &QuoteRequest, context: &QuoteContext) -> Result<Self> {
        if request.id.is_nil()
            || request.client_bucket.len() != 64
            || !request
                .client_bucket
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            || !["herd", "dexter", "rowan", "cosmo", "newton", "nova"]
                .contains(&request.address_user.as_str())
            || request.target_sats == 0
            || request.target_sats > MAX_ACCOUNTING_UNITS
            || context.provider.is_empty()
            || context.provider.len() > 32
            || !context
                .provider
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'-' | b'_'))
            || context.account_scope.is_empty()
            || context.account_scope.len() > 256
            || context.account_scope.chars().any(char::is_control)
        {
            bail!("invalid quote request or trusted context");
        }
        Ok(Self {
            id: request.id,
            client_bucket: request.client_bucket.clone(),
            address_user: request.address_user.clone(),
            target_sats: request.target_sats,
            provider: context.provider.clone(),
            account_scope: context.account_scope.clone(),
            network: match context.network {
                XmrNetwork::Mainnet => "mainnet",
                XmrNetwork::Testnet => "testnet",
                XmrNetwork::Stagenet => "stagenet",
            }
            .into(),
        })
    }
}

/// Evidence stays private; no Debug/Serialize on the public wrapper.
#[derive(Clone)]
pub struct RateEvidence {
    rate: XmrBtcRate,
    fetched_at: i64,
    raw_json: String,
}
impl RateEvidence {
    pub fn new(rate: XmrBtcRate, fetched_at: i64, raw_json: String) -> Result<Self> {
        if fetched_at < rate.observed_at()
            || raw_json.is_empty()
            || raw_json.len() > MAX_RATE_EVIDENCE_BYTES
        {
            bail!("invalid rate evidence bounds or chronology");
        }
        let _: serde_json::Value =
            serde_json::from_str(&raw_json).context("invalid rate evidence JSON")?;
        Ok(Self {
            rate,
            fetched_at,
            raw_json,
        })
    }
    pub fn rate(&self) -> &XmrBtcRate {
        &self.rate
    }
}
pub trait RateProvider: Send + Sync {
    fn source_id(&self) -> &str;
    fn fetch(&self) -> BoxFuture<'_, Result<RateEvidence>>;
}
pub trait QuoteClock: Send + Sync {
    fn now(&self) -> Result<i64>;
}
pub struct SystemQuoteClock;
impl QuoteClock for SystemQuoteClock {
    fn now(&self) -> Result<i64> {
        i64::try_from(SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs())
            .context("clock outside accounting range")
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Document {
    version: u8,
    identity: String,
    policy: QuotePolicy,
    numerator: String,
    denominator: String,
    source: String,
    observed_at: i64,
    fetched_at: i64,
    issued_at: i64,
    expires_at: i64,
    expected_atomic: u64,
    evidence: String,
    evidence_sha256: String,
}
/// Contains private provenance. Only terms_json() is an explicit payer projection;
/// it is NOT a payable invoice, and must not be broadcast as an overlay event.
pub struct StoredXmrQuote {
    pub(crate) document_json: String,
    document: Document,
    identity: Identity,
    quote: XmrQuote,
}
impl StoredXmrQuote {
    pub(crate) fn decode(document_json: String, identity_json: &str) -> Result<Self> {
        use sha2::{Digest, Sha256};
        let document: Document = serde_json::from_str(&document_json)?;
        if document.version != 1 || document.identity != identity_json {
            bail!("stored quote identity/version mismatch");
        }
        let identity: Identity = serde_json::from_str(identity_json)?;
        document.policy.validate()?;
        let rate = XmrBtcRate::new(
            document.numerator.parse()?,
            document.denominator.parse()?,
            &document.source,
            document.observed_at,
        )?;
        validate_rate(&rate, &document.policy)?;
        let quote = rate.quote(
            identity.target_sats,
            document.issued_at,
            document.expires_at,
            document.policy.max_rate_age_seconds,
        )?;
        if identity.target_sats < document.policy.min_target_sats
            || identity.target_sats > document.policy.max_target_sats
            || document.expires_at.checked_sub(document.issued_at)
                != Some(document.policy.lifetime_seconds as i64)
            || document.fetched_at < document.observed_at
            || document.fetched_at > document.issued_at
            || document.expected_atomic != quote.terms().expected_atomic()
            || document.expected_atomic > document.policy.max_piconero
            || document.evidence.len() > MAX_RATE_EVIDENCE_BYTES
            || document.evidence.is_empty()
            || document.evidence_sha256 != hex::encode(Sha256::digest(document.evidence.as_bytes()))
        {
            bail!("stored quote evidence or terms mismatch");
        }
        Ok(Self {
            document_json,
            document,
            identity,
            quote,
        })
    }
    pub fn id(&self) -> Uuid {
        self.identity.id
    }
    pub fn quote(&self) -> &XmrQuote {
        &self.quote
    }
    pub fn expired_at(&self, now: i64) -> bool {
        now >= self.quote.expires_at()
    }
    pub fn terms_json(&self) -> Result<String> {
        serde_json::to_string(&serde_json::json!({"version":1,"quote_id":self.id(),"asset":"XMR","amount_atomic":self.quote.terms().expected_atomic().to_string(),"amount":format_xmr(self.quote.terms().expected_atomic()),"target_credit_sats":self.quote.terms().target_sats(),"issued_at":self.quote.issued_at(),"expires_at":self.quote.expires_at()})).context("encoding quote terms")
    }
    pub(crate) fn credit_intent(&self, receive_scope: &str) -> Result<XmrCreditIntent> {
        Ok(XmrCreditIntent {
            id: self.id(),
            provider: self.identity.provider.clone(),
            account_scope: self.identity.account_scope.clone(),
            network: match self.identity.network.as_str() {
                "mainnet" => XmrNetwork::Mainnet,
                "testnet" => XmrNetwork::Testnet,
                "stagenet" => XmrNetwork::Stagenet,
                _ => bail!("invalid quote network"),
            },
            receive_scope: receive_scope.into(),
            address_user: self.identity.address_user.clone(),
            credit_pool: "herd".into(),
            quote: self.quote.clone(),
            max_credit_sats: self.document.policy.max_credit_sats,
        })
    }
}
fn validate_rate(rate: &XmrBtcRate, policy: &QuotePolicy) -> Result<()> {
    let (n, d) = rate.ratio();
    if u128::from(n) < u128::from(policy.min_sats_per_xmr) * u128::from(d)
        || u128::from(n) > u128::from(policy.max_sats_per_xmr) * u128::from(d)
    {
        bail!("rate outside configured bounds");
    }
    Ok(())
}
pub fn format_xmr(atomic: u64) -> String {
    let whole = atomic / PICONERO_PER_XMR;
    let fractional = atomic % PICONERO_PER_XMR;
    if fractional == 0 {
        whole.to_string()
    } else {
        format!(
            "{whole}.{}",
            format!("{fractional:012}").trim_end_matches('0')
        )
    }
}

#[derive(Clone)]
pub struct QuoteService {
    ledger: LedgerStore,
    context: QuoteContext,
    policy: QuotePolicy,
    provider: Arc<dyn RateProvider>,
    clock: Arc<dyn QuoteClock>,
    slots: Arc<Semaphore>,
}
impl QuoteService {
    pub fn new(
        ledger: LedgerStore,
        context: QuoteContext,
        policy: QuotePolicy,
        provider: Arc<dyn RateProvider>,
        clock: Arc<dyn QuoteClock>,
    ) -> Result<Self> {
        policy.validate()?;
        // Validate trusted configuration before starting any network operation.
        Identity::new(
            &QuoteRequest {
                id: Uuid::new_v4(),
                client_bucket: "0".repeat(64),
                address_user: "herd".into(),
                target_sats: 1,
            },
            &context,
        )?;
        XmrBtcRate::new(1, 1, provider.source_id(), 0)?;
        Ok(Self {
            ledger,
            context,
            policy,
            provider,
            clock,
            slots: Arc::new(Semaphore::new(4)),
        })
    }
    pub async fn create(&self, request: &QuoteRequest) -> Result<StoredXmrQuote> {
        use sha2::{Digest, Sha256};
        let identity = serde_json::to_string(&Identity::new(request, &self.context)?)?;
        let _slot = self
            .slots
            .clone()
            .try_acquire_owned()
            .context("quote service busy")?;
        let now = self.clock.now()?;
        if let Some(existing) = self
            .ledger
            .reserve_xmr_quote(request, &identity, &self.policy, now)
            .await?
        {
            return StoredXmrQuote::decode(existing, &identity);
        }
        let result = async {
            let evidence = tokio::time::timeout(SOURCE_DEADLINE, self.provider.fetch())
                .await
                .context("rate source deadline")??;
            let issued_at = self.clock.now()?;
            if evidence.rate.source() != self.provider.source_id()
                || evidence.fetched_at < now
                || evidence.fetched_at > issued_at
            {
                bail!("rate source identity or fetch chronology mismatch");
            }
            validate_rate(&evidence.rate, &self.policy)?;
            let expires_at = issued_at
                .checked_add(self.policy.lifetime_seconds as i64)
                .context("quote expiry overflow")?;
            let quote = evidence.rate.quote(
                request.target_sats,
                issued_at,
                expires_at,
                self.policy.max_rate_age_seconds,
            )?;
            if quote.terms().expected_atomic() > self.policy.max_piconero {
                bail!("quoted amount exceeds configured limit");
            }
            let (n, d) = evidence.rate.ratio();
            let document = Document {
                version: 1,
                identity: identity.clone(),
                policy: self.policy.clone(),
                numerator: n.to_string(),
                denominator: d.to_string(),
                source: evidence.rate.source().into(),
                observed_at: evidence.rate.observed_at(),
                fetched_at: evidence.fetched_at,
                issued_at,
                expires_at,
                expected_atomic: quote.terms().expected_atomic(),
                evidence_sha256: hex::encode(Sha256::digest(evidence.raw_json.as_bytes())),
                evidence: evidence.raw_json,
            };
            let stored = StoredXmrQuote::decode(serde_json::to_string(&document)?, &identity)?;
            self.ledger
                .complete_xmr_quote(request.id, &identity, &stored, issued_at)
                .await?;
            Ok(stored)
        }
        .await;
        if result.is_err() {
            // If commit actually succeeded but its reply was lost, this conditional
            // update cannot replace it. Same request then reads the immutable quote.
            self.ledger.fail_xmr_quote(request.id).await?;
        }
        result
    }
    pub async fn lookup(&self, request: &QuoteRequest) -> Result<StoredXmrQuote> {
        let identity = serde_json::to_string(&Identity::new(request, &self.context)?)?;
        let _slot = self
            .slots
            .clone()
            .try_acquire_owned()
            .context("quote service busy")?;
        let stored = self
            .ledger
            .read_xmr_quote(request.id, &identity, self.clock.now()?)
            .await?;
        StoredXmrQuote::decode(stored, &identity)
    }
    /// Called ONLY after #97 verifies the provider's immutable receive binding.
    /// A quote is not payable until this commits; expiry forbids a new binding,
    /// but an identical already-bound retry survives expiry without repricing.
    pub async fn bind(
        &self,
        request: &QuoteRequest,
        receive_scope: &str,
    ) -> Result<XmrCreditIntent> {
        let stored = self.lookup(request).await?;
        let intent = stored.credit_intent(receive_scope)?;
        self.ledger
            .bind_xmr_quote(&stored, &intent, self.clock.now()?)
            .await?;
        Ok(intent)
    }
}
