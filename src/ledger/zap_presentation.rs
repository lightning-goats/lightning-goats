//! Read-only presentation correlation; accounting and durable source bytes stay
//! unchanged. Only an actual credit (or its actual consumed feeding credit) can
//! select an invoice-bound zap request.
use super::{DurableEvent, LedgerStore};
use crate::{core_bridge::EventBody, zap_presentation::ZapAttribution};
use anyhow::{Context, Result, bail};
use sqlx::Row as _;

impl LedgerStore {
    pub(crate) async fn verified_zap_attribution(
        &self,
        event: &DurableEvent,
    ) -> Result<Option<ZapAttribution>> {
        if !matches!(
            event.event_type.as_str(),
            "payment_received" | "feeder_confirmed"
        ) {
            return Ok(None);
        }
        let mut tx = self.pool.begin().await?;
        let body = super::core_bridge::export_body(
            &mut tx,
            event.seq,
            &event.event_type,
            Some(&event.payload_json),
        )
        .await?;
        let payment = match body {
            EventBody::Payment(payment) => Some(payment),
            EventBody::Feeding(_) => {
                let value: serde_json::Value = serde_json::from_str(&event.payload_json)?;
                // Old unattributed confirmations retain their neutral wording.
                if value.get("triggering_payment_amount_sats").is_none() {
                    return Ok(None);
                }
                let debit:i64=sqlx::query_scalar("SELECT l.id FROM feed_event_sources s JOIN ledger_entries l ON l.feed_attempt_id=s.feed_attempt_id AND l.entry_type='FEED_DEBIT' WHERE s.event_seq=?")
                    .bind(i64::try_from(event.seq)?).fetch_one(&mut *tx).await?;
                let balance: i64 = sqlx::query_scalar(
                    "SELECT COALESCE(SUM(delta_sats),0) FROM ledger_entries WHERE id<=?",
                )
                .bind(debit)
                .fetch_one(&mut *tx)
                .await?;
                let balance = u64::try_from(balance)?;
                if value["feed_credit_sats"].as_u64() != Some(balance) {
                    bail!("zap feeding balance contradicts its debit");
                }
                let Some((credit, amount)) =
                    super::feed::triggering_credit(&mut tx, debit, balance).await?
                else {
                    return Ok(None);
                };
                if value["triggering_credit_id"].as_i64() != Some(credit)
                    || value["triggering_payment_amount_sats"].as_u64() != Some(amount)
                {
                    bail!("zap attribution differs from the feeding's triggering credit");
                }
                let row=sqlx::query("SELECT e.seq,e.event_type,e.payload_json FROM credit_grants g JOIN event_log e ON e.seq=g.event_seq WHERE g.ledger_entry_id=?")
                    .bind(credit).fetch_optional(&mut *tx).await?;
                if let Some(row) = row {
                    let kind: String = row.try_get("event_type")?;
                    let raw: String = row.try_get("payload_json")?;
                    match super::core_bridge::export_body(
                        &mut tx,
                        u64::try_from(row.try_get::<i64, _>("seq")?)?,
                        &kind,
                        Some(&raw),
                    )
                    .await?
                    {
                        EventBody::Payment(payment) => Some(payment),
                        _ => None,
                    }
                } else {
                    None
                }
            }
            _ => None,
        };
        let attribution = payment
            .as_deref()
            .map(ZapAttribution::from_payment)
            .transpose()?
            .flatten();
        if let Some(attribution) = &attribution {
            let value: serde_json::Value = serde_json::from_str(&event.payload_json)?;
            let expected = if event.event_type == "payment_received" {
                value["amount_sats"].as_u64()
            } else {
                value["triggering_payment_amount_sats"].as_u64()
            };
            if expected.context("zap presentation lacks its actual amount")?
                != attribution.amount_sats()
            {
                bail!("zap presentation amount differs from actual credited payment");
            }
        }
        tx.commit().await?;
        Ok(attribution)
    }
}
