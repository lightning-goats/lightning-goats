use std::time::Duration;

use anyhow::{Context, Result, bail};
use axum::extract::ws::{Message, WebSocket};
use futures_util::{SinkExt, StreamExt};
use serde_json::Value;
use tokio::time::{MissedTickBehavior, interval};

use crate::ledger::{DurableEvent, LedgerStore};

const EVENT_BATCH_SIZE: u32 = 100;
const EVENT_POLL_INTERVAL: Duration = Duration::from_millis(250);

pub async fn serve_overlay_socket(
    socket: WebSocket,
    ledger: LedgerStore,
    threshold_sats: u64,
) -> Result<()> {
    let (mut sender, mut receiver) = socket.split();
    let (mut last_seq, snapshot) = ledger.overlay_snapshot_message(threshold_sats).await?;
    sender
        .send(Message::Text(snapshot.into()))
        .await
        .context("failed sending overlay snapshot")?;

    let mut ticker = interval(EVENT_POLL_INTERVAL);
    ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);

    loop {
        tokio::select! {
            _ = ticker.tick() => {
                loop {
                    let events = ledger.events_after(last_seq, EVENT_BATCH_SIZE).await?;
                    let event_count = events.len();
                    if event_count == 0 {
                        break;
                    }

                    for event in events {
                        let seq = event.seq;
                        let message = durable_event_message(event)?;
                        sender
                            .send(Message::Text(message.into()))
                            .await
                            .context("failed sending durable overlay event")?;
                        last_seq = seq;
                    }

                    if event_count < EVENT_BATCH_SIZE as usize {
                        break;
                    }
                }
            }
            incoming = receiver.next() => {
                match incoming {
                    Some(Ok(Message::Ping(payload))) => {
                        sender
                            .send(Message::Pong(payload))
                            .await
                            .context("failed replying to overlay websocket ping")?;
                    }
                    Some(Ok(Message::Close(_))) | None => return Ok(()),
                    Some(Ok(Message::Text(_)
                        | Message::Binary(_)
                        | Message::Pong(_))) => {
                        // The overlay websocket is intentionally server-to-client only.
                    }
                    Some(Err(error)) => return Err(error).context("overlay websocket receive failed"),
                }
            }
        }
    }
}

fn durable_event_message(event: DurableEvent) -> Result<String> {
    let mut payload: Value = serde_json::from_str(&event.payload_json)
        .context("durable overlay event contains invalid JSON")?;
    let object = payload
        .as_object_mut()
        .context("durable overlay event payload must be a JSON object")?;
    object.insert("type".to_owned(), Value::String(event.event_type));
    object.insert("seq".to_owned(), Value::from(event.seq));
    serde_json::to_string(&payload).context("failed serializing overlay event")
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn durable_event_adds_server_controlled_type_and_sequence() {
        let message = durable_event_message(DurableEvent {
            seq: 42,
            event_type: "payment_received".to_owned(),
            payload_json: json!({
                "amount_sats": 250,
                "type": "attacker-supplied",
                "seq": 999
            })
            .to_string(),
        })
        .unwrap();
        let value: Value = serde_json::from_str(&message).unwrap();

        assert_eq!(value["type"], "payment_received");
        assert_eq!(value["seq"], 42);
        assert_eq!(value["amount_sats"], 250);
    }

    #[test]
    fn durable_event_rejects_non_object_payloads() {
        assert!(durable_event_message(DurableEvent {
            seq: 1,
            event_type: "bad".to_owned(),
            payload_json: "[]".to_owned(),
        })
        .is_err());
    }
}
