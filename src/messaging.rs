use std::time::Duration;

use anyhow::{Context, Result, bail};
use serde_json::Value;
use tokio::time::sleep;

use crate::{
    config::RuntimeMode,
    ledger::{DurableEvent, LedgerStore},
    nostr::{NakClient, SignedNostrEvent},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageProcessorStep {
    Idle,
    ShadowSkipped { seq: u64 },
    NonPublicSkipped { seq: u64 },
    Enqueued { seq: u64 },
}

pub async fn process_next_message(
    ledger: &LedgerStore,
    nak: Option<&NakClient>,
    threshold_sats: u64,
    mode: RuntimeMode,
) -> Result<MessageProcessorStep> {
    if threshold_sats == 0 {
        bail!("feeder threshold must be greater than zero");
    }
    let cursor = ledger.message_cursor().await?;
    let Some(event) = ledger.events_after(cursor, 1).await?.into_iter().next() else {
        return Ok(MessageProcessorStep::Idle);
    };

    if mode == RuntimeMode::Shadow {
        ledger.advance_message_cursor(event.seq).await?;
        return Ok(MessageProcessorStep::ShadowSkipped { seq: event.seq });
    }

    let Some(message) = build_public_message(&event, threshold_sats)? else {
        ledger.advance_message_cursor(event.seq).await?;
        return Ok(MessageProcessorStep::NonPublicSkipped { seq: event.seq });
    };

    let nak = nak.context("active messaging requires a configured NIP-46 nak client")?;
    let signed = nak
        .sign_kind1(&message.content, message.tags)
        .await
        .with_context(|| {
            format!(
                "failed signing Nostr message for durable event {}",
                event.seq
            )
        })?;
    let signed_json =
        serde_json::to_string(&signed).context("failed serializing signed outbox event")?;
    ledger
        .enqueue_signed_message(event.seq, &signed.id, &signed_json)
        .await?;
    Ok(MessageProcessorStep::Enqueued { seq: event.seq })
}

pub async fn run_message_processor(
    ledger: LedgerStore,
    nak: Option<NakClient>,
    threshold_sats: u64,
    mode: RuntimeMode,
) -> Result<()> {
    loop {
        match process_next_message(&ledger, nak.as_ref(), threshold_sats, mode).await {
            Ok(MessageProcessorStep::Idle) => sleep(Duration::from_millis(250)).await,
            Ok(MessageProcessorStep::ShadowSkipped { seq }) => {
                tracing::debug!(seq, "shadow mode: skipped Nostr side effect");
            }
            Ok(MessageProcessorStep::NonPublicSkipped { seq }) => {
                tracing::debug!(seq, "durable event has no public Phase 1 Nostr message");
            }
            Ok(MessageProcessorStep::Enqueued { seq }) => {
                tracing::info!(seq, "signed Nostr event committed to durable outbox");
            }
            Err(error) => {
                tracing::error!(%error, "Nostr message processing failed; source cursor preserved");
                sleep(Duration::from_secs(5)).await;
            }
        }
    }
}

pub async fn run_outbox_publisher(ledger: LedgerStore, nak: NakClient) -> Result<()> {
    loop {
        let Some(entry) = ledger.next_outbox_entry().await? else {
            sleep(Duration::from_millis(500)).await;
            continue;
        };

        let event: SignedNostrEvent = match serde_json::from_str(&entry.signed_event_json) {
            Ok(event) => event,
            Err(error) => {
                ledger
                    .mark_outbox_failed(
                        &entry.event_id,
                        &format!("invalid signed event JSON: {error}"),
                    )
                    .await?;
                tracing::error!(
                    event_id = %entry.event_id,
                    "persisted Nostr outbox row is invalid; publication blocked"
                );
                sleep(publish_retry_delay(entry.attempts)).await;
                continue;
            }
        };

        match nak.publish_signed(&event).await {
            Ok(()) => {
                ledger.mark_outbox_published(&entry.event_id).await?;
                tracing::info!(event_id = %entry.event_id, "published persisted Nostr event");
            }
            Err(error) => {
                ledger
                    .mark_outbox_failed(&entry.event_id, &error.to_string())
                    .await?;
                tracing::warn!(
                    %error,
                    event_id = %entry.event_id,
                    "Nostr publication failed; exact signed event will be retried"
                );
                sleep(publish_retry_delay(entry.attempts)).await;
            }
        }
    }
}

struct PublicMessage {
    content: String,
    tags: Vec<Vec<String>>,
}

fn build_public_message(
    event: &DurableEvent,
    threshold_sats: u64,
) -> Result<Option<PublicMessage>> {
    let payload: Value = serde_json::from_str(&event.payload_json)
        .context("durable event contains invalid JSON for message rendering")?;

    let content = match event.event_type.as_str() {
        "payment_received" => {
            let amount = required_u64(&payload, "amount_sats")?;
            let credit = required_u64(&payload, "feed_credit_sats")?;
            if credit < threshold_sats {
                let remaining = threshold_sats - credit;
                format!(
                    "⚡ {amount} sats received for the Lightning Goats. {remaining} sats until the next feeding."
                )
            } else {
                let feeds_due = credit / threshold_sats;
                let remainder = credit % threshold_sats;
                format!(
                    "⚡ {amount} sats received for the Lightning Goats. {feeds_due} feeding(s) earned; {remainder} sats remain toward the next feeding."
                )
            }
        }
        "feeder_confirmed" => {
            let remaining = required_u64(&payload, "feed_credit_sats")?;
            if remaining >= threshold_sats {
                format!(
                    "⚡ The Lightning Goats have been fed! {remaining} sats remain credited and another feeding is due."
                )
            } else {
                format!(
                    "⚡ The Lightning Goats have been fed! {remaining} sats remain toward the next feeding."
                )
            }
        }
        _ => return Ok(None),
    };

    Ok(Some(PublicMessage {
        content,
        tags: vec![vec!["t".to_owned(), "LightningGoats".to_owned()]],
    }))
}

fn required_u64(payload: &Value, field: &str) -> Result<u64> {
    payload
        .get(field)
        .and_then(Value::as_u64)
        .with_context(|| format!("durable event is missing unsigned integer field {field}"))
}

fn publish_retry_delay(previous_attempts: u64) -> Duration {
    let exponent = previous_attempts.min(5) as u32;
    Duration::from_secs((1u64 << exponent).min(30))
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use tempfile::TempDir;

    use super::*;

    fn event(seq: u64, event_type: &str, payload: Value) -> DurableEvent {
        DurableEvent {
            seq,
            event_type: event_type.to_owned(),
            payload_json: payload.to_string(),
        }
    }

    async fn store() -> (TempDir, LedgerStore) {
        let directory = TempDir::new().unwrap();
        let path = directory.path().join("lightning-goats.db");
        let store = LedgerStore::connect(&format!("sqlite://{}", path.display()))
            .await
            .unwrap();
        (directory, store)
    }

    #[test]
    fn renders_payment_below_threshold() {
        let message = build_public_message(
            &event(
                1,
                "payment_received",
                json!({"amount_sats": 250, "feed_credit_sats": 750}),
            ),
            1_000,
        )
        .unwrap()
        .unwrap();
        assert!(message.content.contains("250 sats received"));
        assert!(message.content.contains("250 sats until"));
    }

    #[test]
    fn renders_multiple_feeds_due() {
        let message = build_public_message(
            &event(
                1,
                "payment_received",
                json!({"amount_sats": 2340, "feed_credit_sats": 2340}),
            ),
            1_000,
        )
        .unwrap()
        .unwrap();
        assert!(message.content.contains("2 feeding(s) earned"));
        assert!(message.content.contains("340 sats remain"));
    }

    #[test]
    fn processing_errors_are_not_public_nostr_messages() {
        assert!(
            build_public_message(
                &event(1, "processing_error", json!({"message": "internal"})),
                1_000
            )
            .unwrap()
            .is_none()
        );
    }

    #[tokio::test]
    async fn shadow_mode_advances_cursor_without_signing_or_outbox() {
        let (_directory, store) = store().await;
        store
            .append_event(
                "payment_received",
                &json!({"amount_sats": 100, "feed_credit_sats": 100}),
            )
            .await
            .unwrap();

        assert_eq!(
            process_next_message(&store, None, 1_000, RuntimeMode::Shadow)
                .await
                .unwrap(),
            MessageProcessorStep::ShadowSkipped { seq: 1 }
        );
        assert_eq!(store.message_cursor().await.unwrap(), 1);
        assert!(store.next_outbox_entry().await.unwrap().is_none());
    }

    #[test]
    fn retry_delay_is_bounded() {
        assert_eq!(publish_retry_delay(0), Duration::from_secs(1));
        assert_eq!(publish_retry_delay(1), Duration::from_secs(2));
        assert_eq!(publish_retry_delay(5), Duration::from_secs(30));
        assert_eq!(publish_retry_delay(100), Duration::from_secs(30));
    }
}
