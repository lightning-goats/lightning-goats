use std::time::Duration;

use anyhow::{Context, Result, bail};
use tokio::time::sleep;

use crate::{
    config::RuntimeMode,
    ledger::LedgerStore,
    nostr::{NakClient, SignedNostrEvent},
    presentation::MessageRenderer,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageProcessorStep {
    Idle,
    PublicationDisabled { seq: u64 },
    NonPublicSkipped { seq: u64 },
    Enqueued { seq: u64 },
}

pub async fn process_next_message(
    ledger: &LedgerStore,
    nak: Option<&NakClient>,
    renderer: &MessageRenderer,
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

    if !mode.nostr_enabled() {
        ledger.advance_message_cursor(event.seq).await?;
        return Ok(MessageProcessorStep::PublicationDisabled { seq: event.seq });
    }

    let rendered = renderer.render(&event, threshold_sats)?;
    let Some(content) = rendered.nostr_content else {
        ledger.advance_message_cursor(event.seq).await?;
        return Ok(MessageProcessorStep::NonPublicSkipped { seq: event.seq });
    };

    let nak = nak.context("active messaging requires a configured NIP-46 nak client")?;
    let signed = nak
        .sign_kind1(
            &content,
            vec![vec!["t".to_owned(), "LightningGoats".to_owned()]],
        )
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
    renderer: MessageRenderer,
    threshold_sats: u64,
    mode: RuntimeMode,
) -> Result<()> {
    loop {
        match process_next_message(&ledger, nak.as_ref(), &renderer, threshold_sats, mode).await {
            Ok(MessageProcessorStep::Idle) => sleep(Duration::from_millis(250)).await,
            Ok(MessageProcessorStep::PublicationDisabled { seq }) => {
                tracing::debug!(
                    seq,
                    mode = mode.as_str(),
                    "Nostr publication disabled for runtime mode"
                );
            }
            Ok(MessageProcessorStep::NonPublicSkipped { seq }) => {
                tracing::debug!(seq, "durable event is overlay-only or has no Phase 1 public message");
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

fn publish_retry_delay(previous_attempts: u64) -> Duration {
    let exponent = previous_attempts.min(5) as u32;
    Duration::from_secs((1u64 << exponent).min(30))
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use tempfile::TempDir;

    use super::*;

    async fn store() -> (TempDir, LedgerStore) {
        let directory = TempDir::new().unwrap();
        let path = directory.path().join("lightning-goats.db");
        let store = LedgerStore::connect(&format!("sqlite://{}", path.display()))
            .await
            .unwrap();
        (directory, store)
    }

    #[tokio::test]
    async fn shadow_mode_advances_cursor_without_signing_or_outbox() {
        let (_directory, store) = store().await;
        store
            .append_event(
                "payment_received",
                &json!({
                    "amount_sats": 100,
                    "feed_credit_sats": 100,
                    "address_user": "herd"
                }),
            )
            .await
            .unwrap();
        let renderer = MessageRenderer::embedded().unwrap();

        assert_eq!(
            process_next_message(&store, None, &renderer, 1_000, RuntimeMode::Shadow)
                .await
                .unwrap(),
            MessageProcessorStep::PublicationDisabled { seq: 1 }
        );
        assert_eq!(store.message_cursor().await.unwrap(), 1);
        assert!(store.next_outbox_entry().await.unwrap().is_none());
    }

    #[tokio::test]
    async fn canary_mode_advances_cursor_without_signing_or_outbox() {
        let (_directory, store) = store().await;
        store
            .append_event(
                "payment_received",
                &json!({
                    "amount_sats": 100,
                    "feed_credit_sats": 100,
                    "address_user": "herd"
                }),
            )
            .await
            .unwrap();
        let renderer = MessageRenderer::embedded().unwrap();

        assert_eq!(
            process_next_message(&store, None, &renderer, 1_000, RuntimeMode::Canary)
                .await
                .unwrap(),
            MessageProcessorStep::PublicationDisabled { seq: 1 }
        );
        assert_eq!(store.message_cursor().await.unwrap(), 1);
        assert!(store.next_outbox_entry().await.unwrap().is_none());
    }

    #[tokio::test]
    async fn active_overlay_only_event_advances_without_nostr_client() {
        let (_directory, store) = store().await;
        store
            .append_event("interface_info", &json!({}))
            .await
            .unwrap();
        let renderer = MessageRenderer::embedded().unwrap();

        assert_eq!(
            process_next_message(&store, None, &renderer, 1_000, RuntimeMode::Active)
                .await
                .unwrap(),
            MessageProcessorStep::NonPublicSkipped { seq: 1 }
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
