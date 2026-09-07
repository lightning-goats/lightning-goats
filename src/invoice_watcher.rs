use std::time::Duration;

use anyhow::{Context, Result};
use serde_json::json;
use tokio::time::sleep;

use crate::{
    cln::ClnRestClient,
    domain::{invoice::ClnAddressInvoiceLabel, payment::SettledPayment},
    ledger::{LedgerStore, SettlementOutcome},
};

const WAIT_TIMEOUT_SECONDS: u64 = 30;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LegacyClnPollOutcome {
    Credited { sats: u64, address_user: String },
    Ignored,
    Duplicate,
}

pub async fn poll_once(
    client: &ClnRestClient,
    ledger: &LedgerStore,
    herd_user: &str,
) -> Result<Option<LegacyClnPollOutcome>> {
    let cursor = ledger
        .last_legacy_cln_pay_index()
        .await?
        .context("legacy CLN cursor is uninitialized; run lightning-goatsctl init-cursor first")?;
    let Some(invoice) = client
        .wait_any_invoice(cursor, WAIT_TIMEOUT_SECONDS)
        .await?
    else {
        return Ok(None);
    };

    let outcome = match ClnAddressInvoiceLabel::parse(&invoice.label) {
        Ok(label) if label.is_for_user(herd_user) => {
            let payment = SettledPayment {
                source: "cln".to_owned(),
                source_id: format!("pay-index-{}", invoice.pay_index),
                payment_hash: Some(invoice.payment_hash.clone()),
                address_user: label.user().to_owned(),
                credit_pool: "herd".to_owned(),
                amount_msat: invoice.amount_msat,
                settled_at: Some(invoice.settled_at),
                context_json: Some(
                    json!({
                        "legacy_cln_pay_index": invoice.pay_index,
                        "legacy_cln_label": invoice.label,
                    })
                    .to_string(),
                ),
            };
            match ledger.record_payment(&payment).await? {
                SettlementOutcome::Credited {
                    sats,
                    address_user,
                    ..
                } => LegacyClnPollOutcome::Credited { sats, address_user },
                SettlementOutcome::Duplicate => LegacyClnPollOutcome::Duplicate,
            }
        }
        _ => LegacyClnPollOutcome::Ignored,
    };

    // Advance only after any qualifying settlement has been durably recorded.
    // If we crash between record_payment and this cursor update, replay is safe
    // because provider-neutral settlement identity is idempotent.
    ledger.advance_legacy_cln_cursor(invoice.pay_index).await?;
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
            Ok(Some(LegacyClnPollOutcome::Credited { sats, address_user })) => {
                consecutive_transport_errors = 0;
                let credit = ledger.feed_credit_sats().await?;
                tracing::info!(sats, %address_user, feed_credit_sats = credit, "credited qualifying legacy CLN payment through backend-neutral ledger");
            }
            Ok(Some(LegacyClnPollOutcome::Ignored)) => {
                consecutive_transport_errors = 0;
                tracing::debug!(
                    "observed non-herd legacy CLN paid invoice; compatibility cursor advanced without feed credit"
                );
            }
            Ok(Some(LegacyClnPollOutcome::Duplicate)) => {
                consecutive_transport_errors = 0;
                tracing::debug!("observed replayed legacy CLN payment; no duplicate credit added");
            }
            Ok(None) => {
                consecutive_transport_errors = 0;
            }
            Err(error) if is_retryable_cln_error(&error) => {
                consecutive_transport_errors = consecutive_transport_errors.saturating_add(1);
                let delay = retry_delay(consecutive_transport_errors);
                tracing::warn!(%error, ?delay, "CLNRest polling failed; preserving legacy cursor and retrying");
                sleep(delay).await;
            }
            Err(error) => {
                return Err(error)
                    .context("legacy CLN paid-invoice watcher stopped on ledger/invariant error");
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
