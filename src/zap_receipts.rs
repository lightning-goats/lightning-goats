//! Independent protocol-receipt delivery. Its outages cannot undo settlement,
//! block the feeder or manufacture membership; retries use exact stored bytes.
use std::time::Duration;

use anyhow::{Context, Result, bail};
use futures_util::{SinkExt, StreamExt};
use tokio::time::timeout;
use tokio_tungstenite::tungstenite::Message;

use crate::{ledger::LedgerStore, nostr::NakClient, nostr_event::VerifiedEvent};

pub async fn publish_protocol_event(event: &VerifiedEvent, relay: &str) -> Result<()> {
    if !matches!(event.event().kind, 1 | 1311 | 9735) {
        bail!("this event kind cannot be published by the protocol outbox");
    }
    let mut socket = crate::public_network::connect_relay(relay).await?;
    let wire = serde_json::json!(["EVENT", event.event()]).to_string();
    timeout(Duration::from_secs(8), async {
        socket.send(Message::Text(wire.into())).await?;
        for _ in 0..32 {
            let message = socket
                .next()
                .await
                .context("relay closed before acknowledging the stored event")??;
            match message {
                Message::Text(text) => {
                    let value: serde_json::Value =
                        serde_json::from_str(&text).context("invalid relay protocol response")?;
                    if value[0] == "OK" && value[1] == event.event().id {
                        if value[2] == true {
                            return Ok(());
                        }
                        bail!("relay rejected the stored signed event");
                    }
                }
                Message::Ping(bytes) => socket.send(Message::Pong(bytes)).await?,
                Message::Close(_) => bail!("relay closed before acknowledgement"),
                _ => {}
            }
        }
        bail!("relay exceeded the acknowledgement message bound")
    })
    .await
    .context("relay acknowledgement deadline exceeded")??;
    let _ = timeout(Duration::from_millis(500), socket.close(None)).await;
    Ok(())
}

pub async fn receipt_step(ledger: &LedgerStore, nak: &NakClient) -> Result<bool> {
    let Some(job) = ledger.reserve_zap_receipt().await? else {
        return Ok(false);
    };
    let signed = if let Some(json) = &job.signed_json {
        match VerifiedEvent::from_json(json) {
            Ok(event) => event,
            Err(_) => {
                ledger.finish_zap_receipt_step(&job, false, true).await?;
                bail!("persisted zap receipt failed verification; it will not be replaced");
            }
        }
    } else {
        match nak.sign_zap_receipt(&job).await {
            Ok(event) => {
                let stored = ledger.save_signed_zap_receipt(&job, &event).await?;
                VerifiedEvent::from_json(&stored)?
            }
            Err(_) => {
                ledger.finish_zap_receipt_step(&job, false, true).await?;
                bail!("zap receipt signing failed; immutable intent retained for retry");
            }
        }
    };
    if signed.event().kind != 9735
        || signed.event().pubkey != job.request.receipt_signer()
        || signed.event().created_at != job.paid_at
        || !signed.event().content.is_empty()
        || signed.event().tags != job.request.receipt_tags(&job.invoice)?
    {
        ledger.finish_zap_receipt_step(&job, false, true).await?;
        bail!("persisted zap receipt differs from its authoritative settlement intent");
    }
    let published = publish_protocol_event(&signed, &job.relay).await.is_ok();
    ledger
        .finish_zap_receipt_step(&job, published, false)
        .await?;
    if !published {
        bail!("zap receipt relay delivery failed; exact signed bytes retained");
    }
    Ok(true)
}

pub async fn run_receipt_worker(ledger: LedgerStore, nak: NakClient) -> Result<()> {
    loop {
        if let Err(error) = receipt_step(&ledger, &nak).await {
            tracing::warn!(%error,"zap receipt delivery deferred; settlement is unaffected");
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nostr_event::test_event;

    #[tokio::test]
    async fn zap_requests_are_rejected_before_any_relay_connection() {
        let request = VerifiedEvent::verify(test_event(1, 9734, 100, "", vec![])).unwrap();
        let error = publish_protocol_event(&request, "wss://10.8.0.6/")
            .await
            .unwrap_err();
        assert!(error.to_string().contains("cannot be published"));
    }
}
