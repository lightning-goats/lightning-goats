//! Wire DTOs for MoneroPay cbf644ce025914857fdafa8e7e7aeacb849c6159.
//! No balance, transfer, DELETE, wallet-RPC or arbitrary URL method exists here.
use anyhow::{Result, bail};
use reqwest::{Client, Url};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeSet, time::Duration};

#[derive(Clone)]
pub struct Provider {
    client: Client,
    base: Url,
    limit: usize,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Created {
    pub address: String,
    pub amount: u64,
    pub description: String,
    pub created_at: String,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Transaction {
    pub amount: u64,
    pub confirmations: u64,
    pub double_spend_seen: bool,
    pub fee: u64,
    pub height: u64,
    pub timestamp: String,
    pub tx_hash: String,
    pub unlock_time: u64,
    pub locked: bool,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Coverage {
    pub total: u64,
    pub unlocked: u64,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Amount {
    pub expected: u64,
    pub covered: Coverage,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Receive {
    pub amount: Amount,
    pub complete: bool,
    pub description: String,
    pub created_at: String,
    // Go serializes a nil slice as null when no receipts have arrived.
    pub transactions: Option<Vec<Transaction>>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Health {
    status: u16,
    services: Services,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Services {
    walletrpc: bool,
    #[serde(default)]
    postgresql: bool,
    #[serde(default)]
    sqlite: bool,
}

pub fn loopback_base(value: &str) -> Result<Url> {
    let url = Url::parse(value).map_err(|_| anyhow::anyhow!("invalid local provider URL"))?;
    if url.scheme() != "http"
        || url.host_str() != Some("127.0.0.1")
        || url.port().is_none_or(|p| p == 0 || p == 5000)
        || !url.username().is_empty()
        || url.password().is_some()
        || url.path() != "/"
        || url.query().is_some()
        || url.fragment().is_some()
    {
        bail!("provider must use an explicit 127.0.0.1 HTTP port other than weather5000");
    }
    Ok(url)
}

impl Provider {
    pub fn new(base: &str, timeout: Duration, limit: usize) -> Result<Self> {
        let base = loopback_base(base)?;
        let client = Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(timeout)
            .timeout(timeout)
            .build()?;
        Ok(Self {
            client,
            base,
            limit,
        })
    }

    async fn decode<T: serde::de::DeserializeOwned>(
        &self,
        request: reqwest::RequestBuilder,
    ) -> Result<T> {
        let response = request
            .send()
            .await
            .map_err(|_| anyhow::anyhow!("provider unavailable"))?;
        if response.status() != reqwest::StatusCode::OK
            || response
                .headers()
                .get("content-type")
                .and_then(|v| v.to_str().ok())
                .is_none_or(|v| {
                    !v.split(';')
                        .next()
                        .unwrap_or("")
                        .trim()
                        .eq_ignore_ascii_case("application/json")
                })
        {
            bail!("provider status or content type rejected");
        }
        let bytes = crate::http::bounded_body(response, self.limit)
            .await
            .map_err(|_| anyhow::anyhow!("provider body rejected"))?;
        serde_json::from_slice(&bytes).map_err(|_| anyhow::anyhow!("provider schema rejected"))
    }

    pub async fn health(&self) -> Result<()> {
        let h: Health = self
            .decode(self.client.get(self.base.join("health")?))
            .await?;
        if h.status != 200 || !h.services.walletrpc || !(h.services.postgresql || h.services.sqlite)
        {
            bail!("provider health unavailable");
        }
        // This is availability/refresh, NOT independent evidence of chain sync.
        Ok(())
    }

    pub async fn create(&self, amount: u64, description: &str, callback: &str) -> Result<Created> {
        self.decode(
            self.client
                .post(self.base.join("receive")?)
                .json(&serde_json::json!({
                    "amount": amount, "description": description, "callback_url": callback
                })),
        )
        .await
    }

    pub async fn receive(&self, address: &str) -> Result<Receive> {
        // Address already passed validation; no path/query fragments can be supplied.
        if !address_shape(address) {
            bail!("invalid receive address");
        }
        self.decode(
            self.client
                .get(self.base.join(&format!("receive/{address}"))?),
        )
        .await
    }
}

pub fn address_shape(s: &str) -> bool {
    const B58: &[u8] = b"123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";
    s.len() == 95 && s.bytes().all(|b| B58.contains(&b))
}

pub fn time(s: &str) -> Result<i64> {
    let t = chrono::DateTime::parse_from_rfc3339(s)
        .map_err(|_| anyhow::anyhow!("invalid provider timestamp"))?
        .timestamp_micros();
    if t < 0 {
        bail!("negative provider timestamp");
    }
    Ok(t)
}

impl Receive {
    pub fn validate(&mut self, created: &Created, max_receipts: usize, height: u64) -> Result<()> {
        if self.amount.expected != created.amount || self.description != created.description
            // PostgreSQL timestamp storage may round POST nanoseconds to microseconds.
            || time(&self.created_at)?.abs_diff(time(&created.created_at)?) > 1
        {
            bail!("provider receive binding changed");
        }
        let transactions = self.transactions.get_or_insert_with(Vec::new);
        if transactions.len() > max_receipts {
            bail!("receipt capacity exceeded");
        }
        let mut ids = BTreeSet::new();
        let mut total = 0u64;
        let mut unlocked = 0u64;
        for t in transactions.iter() {
            if t.amount == 0
                || t.amount > i64::MAX as u64
                || t.tx_hash.len() != 64
                || !t
                    .tx_hash
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
                || !ids.insert(t.tx_hash.clone())
                || t.height > height
                || t.double_spend_seen
                || (!t.locked && (t.confirmations < 10 || t.height == 0))
            {
                bail!("receipt identity or finality rejected");
            }
            time(&t.timestamp)?;
            total = total
                .checked_add(t.amount)
                .filter(|v| *v <= i64::MAX as u64)
                .ok_or_else(|| anyhow::anyhow!("receipt amount overflow"))?;
            if !t.locked {
                unlocked = unlocked
                    .checked_add(t.amount)
                    .ok_or_else(|| anyhow::anyhow!("receipt amount overflow"))?;
            }
        }
        if total != self.amount.covered.total
            || unlocked != self.amount.covered.unlocked
            || self.complete != (unlocked >= self.amount.expected)
        {
            bail!("incomplete or inconsistent provider history");
        }
        transactions.sort_by(|a, b| a.tx_hash.cmp(&b.tx_hash));
        Ok(())
    }
}
