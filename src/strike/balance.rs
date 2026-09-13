//! Authoritative read-only balance boundary for private operational alerts.
use anyhow::{Result, bail};
use serde::Deserialize;

use super::{StrikeClient, btc_decimal_to_msat};

// Deliberately no Debug/Serialize: operational balances are not log/event data.
pub struct StrikeBtcBalance {
    current_sats: u64,
}

impl StrikeBtcBalance {
    /// Includes pending deposits; not the lower immediately spendable balance.
    pub fn current_sats(&self) -> u64 {
        self.current_sats
    }
}

#[derive(Deserialize)]
struct BalanceRow {
    currency: String,
    current: Option<String>,
}

impl StrikeClient {
    /// GET only, requiring partner.balances.read. No deprecated total fallback,
    /// fiat conversion, payment-ledger inference or absent-BTC-as-zero behavior.
    pub async fn btc_balance(&self) -> Result<StrikeBtcBalance> {
        let rows: Vec<BalanceRow> = self
            .send_json(self.client.get(self.base_url.join("balances")?), "balance")
            .await
            // Avoid provider field values or malformed response text in logs.
            .map_err(|_| anyhow::anyhow!("authoritative Strike balance unavailable"))?;
        let mut btc = rows.into_iter().filter(|row| row.currency == "BTC");
        let row = btc
            .next()
            .ok_or_else(|| anyhow::anyhow!("authoritative BTC balance absent"))?;
        if btc.next().is_some() {
            bail!("authoritative BTC balance ambiguous");
        }
        let current = row
            .current
            .ok_or_else(|| anyhow::anyhow!("authoritative BTC current balance absent"))?;
        // Tighten the shared amount parser: no whitespace, dangling decimal,
        // exponent, sign, fractional satoshi, or unbounded provider string.
        if current.len() > 32 || current.trim() != current || current.ends_with('.') {
            bail!("authoritative BTC balance invalid");
        }
        let msat = btc_decimal_to_msat(&current)
            .map_err(|_| anyhow::anyhow!("authoritative BTC balance invalid"))?;
        if msat % 1000 != 0 {
            bail!("authoritative BTC balance is not satoshi-aligned");
        }
        Ok(StrikeBtcBalance {
            current_sats: msat / 1000,
        })
    }
}
