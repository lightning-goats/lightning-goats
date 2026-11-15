use std::time::Duration;

use anyhow::{Context, Result, bail};
use tokio::time::sleep;

use crate::{
    config::RuntimeMode, ledger::LedgerStore, nostr::NakClient, presentation::MessageRenderer,
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

    if !ledger.feeding_announcement_allowed(event.seq).await? {
        ledger.advance_message_cursor(event.seq).await?;
        return Ok(MessageProcessorStep::NonPublicSkipped { seq: event.seq });
    }
    let zap = ledger.verified_zap_attribution(&event).await?;
    let rendered = renderer.render_with_zap(&event, threshold_sats, zap.as_ref())?;
    let Some(content) = rendered.nostr_content else {
        ledger.advance_message_cursor(event.seq).await?;
        return Ok(MessageProcessorStep::NonPublicSkipped { seq: event.seq });
    };

    let nak = nak.context("active messaging requires a configured NIP-46 nak client")?;
    let mut tags = vec![vec!["t".to_owned(), "LightningGoats".to_owned()]];
    if let Some(zap) = &zap {
        if let Some(sender) = zap.sender() {
            tags.push(vec!["p".into(), sender.into()]);
        }
        tags.push(vec!["lightning-goats".into(), zap.identity().into()]);
    }
    let signed = nak.sign_kind1(&content, tags).await.with_context(|| {
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
                tracing::debug!(
                    seq,
                    "durable event is overlay-only or has no Phase 1 public message"
                );
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

pub trait PublicationTransport {
    fn publish(
        &self,
        event: &crate::nostr_event::VerifiedEvent,
        relay: &str,
    ) -> impl std::future::Future<Output = Result<()>> + Send;
}
pub struct ProtocolTransport;
impl PublicationTransport for ProtocolTransport {
    async fn publish(&self, event: &crate::nostr_event::VerifiedEvent, relay: &str) -> Result<()> {
        crate::zap_receipts::publish_protocol_event(event, relay).await
    }
}
pub async fn run_outbox_publisher(ledger: LedgerStore, nak: NakClient) -> Result<()> {
    let base = nak.publication_relays().to_vec();
    let router = crate::relay_routing::Router::new(&base)?;
    run_publication_worker(ledger, nak, router, base, ProtocolTransport).await
}
/// Dependency-injected transport permits isolated protocol qualification. The
/// production entry point always uses pinned public networking above.
pub async fn run_publication_worker<
    Q: crate::cyberherd::relay::Query + Send + Sync,
    T: PublicationTransport + Send + Sync,
>(
    ledger: LedgerStore,
    nak: NakClient,
    router: crate::relay_routing::Router<Q>,
    base: Vec<String>,
    transport: T,
) -> Result<()> {
    let mut metadata_checked = 0i64;
    let mut author_metadata = None;
    loop {
        let now = crate::cyberherd::calendar::now()?;
        if now.saturating_sub(metadata_checked) >= 900 {
            match advertise_relay_list(&ledger, &nak, &router, now).await {
                Ok(event) => author_metadata = Some(event),
                Err(_) => tracing::warn!(
                    "relay-list advertisement retained for retry; payment publication continues"
                ),
            }
            metadata_checked = now;
        }
        if let Some(entry) = ledger.unplanned_publication().await? {
            match crate::nostr_event::VerifiedEvent::from_json(&entry.signed_event_json) {
                Ok(event) if matches!(event.event().kind, 1 | 10002) => {
                    let people =
                        crate::relay_routing::people(&event.event().content, &event.event().tags);
                    let relays = router
                        .public_relays(&people, &base, crate::cyberherd::calendar::now()?)
                        .await?;
                    if event.event().kind != 10002
                        && let Some(metadata) = &author_metadata
                    {
                        ledger.route_relay_metadata(metadata, &relays).await?;
                    }
                    ledger.plan_publication(&entry.event_id, &relays).await?;
                }
                _ => {
                    ledger
                        .mark_outbox_failed(&entry.event_id, "invalid signed public event")
                        .await?;
                    sleep(publish_retry_delay(entry.attempts)).await;
                }
            }
        }
        let Some(job) = ledger
            .reserve_publication(crate::cyberherd::calendar::now()?)
            .await?
        else {
            sleep(Duration::from_millis(500)).await;
            continue;
        };
        let accepted = match crate::nostr_event::VerifiedEvent::from_json(&job.signed_json) {
            Ok(event)
                if event.event().id == job.event_id
                    && event.event().pubkey == nak.project_pubkey() =>
            {
                transport.publish(&event, &job.relay).await.is_ok()
            }
            _ => false,
        };
        ledger
            .finish_publication(&job, accepted, crate::cyberherd::calendar::now()?)
            .await?;
        sleep(Duration::from_millis(100)).await;
    }
}

async fn advertise_relay_list<Q: crate::cyberherd::relay::Query>(
    ledger: &LedgerStore,
    nak: &NakClient,
    router: &crate::relay_routing::Router<Q>,
    now: i64,
) -> Result<crate::nostr_event::VerifiedEvent> {
    let lists = router.lists(&[nak.project_pubkey().into()], now).await?;
    // Missing metadata never authorizes erasing the author's existing read policy.
    let current = lists
        .get(nak.project_pubkey())
        .context("project relay list unavailable")?;
    if current.event().event().created_at >= now as u64 {
        return Ok(current.event().clone());
    }
    let Some(tags) =
        crate::relay_routing::advertised_write_tags(Some(current), nak.publication_relays())?
    else {
        return Ok(current.event().clone());
    };
    let intent = ledger
        .relay_metadata_intent(nak.project_pubkey(), &current.event().event().id, tags, now)
        .await?;
    if ledger.relay_metadata_signed(&intent).await?.is_none() {
        let signed = nak
            .sign_protocol_event(10002, intent.created_at, "", intent.tags.clone())
            .await?;
        ledger.retain_relay_metadata(&intent, &signed).await?;
    }
    ledger
        .relay_metadata_signed(&intent)
        .await?
        .context("relay metadata signature absent")
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
