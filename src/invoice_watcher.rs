use std::time::Duration;

use anyhow::{Context, Result};
use tokio::time::sleep;

use crate::{
    cln::ClnRestClient,
    ledger::{LedgerStore, SettlementOutcome},
};

const WAIT_TIMEOUT_SECONDS: u64 = 30;

pub async fn poll_once(
    client: &ClnRestClient,
    ledger: &LedgerStore,
    herd_user: &str,
) -> Result<Option<SettlementOutcome>> {
    let cursor = ledger
        .last_pay_index()
        .await?
        .context("CLN cursor is uninitialized; run lightning-goatsctl init-cursor first")?;
    let Some(invoice) = client
        .wait_any_invoice(cursor, WAIT_TIMEOUT_SECONDS)
        .await?
    else {
        return Ok(None);
    };

    let outcome = ledger.record_settlement(&invoice, herd_user).await?;
    Ok(Some(outcome))
}

pub async fn run_invoice_watcher(
    client: ClnRestClient,
    ledger: LedgerStore,
    herd_user: String,
) -> Result<()> {
    let mut consecutive_transport_errors = 0u32;

    loop {
        match poll_once(&client, &ledger, &herd_user).await {
            Ok(Some(SettlementOutcome::Credited { sats, user })) => {
                consecutive_transport_errors = 0;
                let credit = ledger.feed_credit_sats().await?;
                tracing::info!(sats, %user, feed_credit_sats = credit, "credited qualifying Lightning Goats payment");
            }
            Ok(Some(SettlementOutcome::Ignored)) => {
                consecutive_transport_errors = 0;
                tracing::debug!(
                    "observed non-herd paid invoice; cursor advanced without feed credit"
                );
            }
            Ok(Some(SettlementOutcome::Duplicate)) => {
                consecutive_transport_errors = 0;
                tracing::debug!("observed duplicate paid invoice event; no credit added");
            }
            Ok(None) => {
                consecutive_transport_errors = 0;
            }
            Err(error) if is_retryable_cln_error(&error) => {
                consecutive_transport_errors = consecutive_transport_errors.saturating_add(1);
                let delay = retry_delay(consecutive_transport_errors);
                tracing::warn!(%error, ?delay, "CLNRest polling failed; preserving cursor and retrying");
                sleep(delay).await;
            }
            Err(error) => {
                return Err(error).context("paid-invoice watcher stopped on ledger/invariant error");
            }
        }
    }
}

fn is_retryable_cln_error(error: &anyhow::Error) -> bool {
    error.chain().any(|cause| cause.is::<reqwest::Error>())
        || error
            .to_string()
            .starts_with("CLNRest waitanyinvoice failed with HTTP")
}

fn retry_delay(consecutive_errors: u32) -> Duration {
    let exponent = consecutive_errors.saturating_sub(1).min(5);
    Duration::from_secs((1u64 << exponent).min(30))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retry_backoff_is_bounded() {
        assert_eq!(retry_delay(1), Duration::from_secs(1));
        assert_eq!(retry_delay(2), Duration::from_secs(2));
        assert_eq!(retry_delay(6), Duration::from_secs(30));
        assert_eq!(retry_delay(100), Duration::from_secs(30));
    }
}
