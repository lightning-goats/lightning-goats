//! Publishes only the retained encrypted wrapper. No signing or monetary authority.
use anyhow::{Context as _, Result, bail};
use futures_util::{SinkExt as _, StreamExt as _, future::select_ok};
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio_tungstenite::{WebSocketStream, tungstenite::Message};

use crate::{ledger::LedgerStore, nostr_event::VerifiedEvent};

async fn publish_one(relay: &str, event: &VerifiedEvent) -> Result<String> {
    tokio::time::timeout(Duration::from_secs(20), async {
        let socket = crate::public_network::connect_relay(relay).await?;
        publish_socket(socket, event).await?;
        Ok(relay.to_owned())
    })
    .await
    .context("private relay acknowledgment expired")?
}

async fn publish_socket<S: AsyncRead + AsyncWrite + Unpin>(
    mut socket: WebSocketStream<S>,
    event: &VerifiedEvent,
) -> Result<()> {
    let wire = serde_json::to_string(&serde_json::json!(["EVENT", event.event()]))?;
    if wire.len() > 32768 {
        bail!("private publication exceeds bound");
    }
    socket.send(Message::Text(wire.into())).await?;
    let mut bytes = 0usize;
    for _ in 0..64 {
        let Some(frame) = socket.next().await else {
            bail!("private relay closed before acknowledgment")
        };
        let frame = frame?;
        bytes = bytes.saturating_add(frame.len());
        if bytes > 131072 {
            bail!("private relay acknowledgment exceeds bound");
        }
        match frame {
            Message::Text(text) => {
                let value: serde_json::Value = serde_json::from_str(&text)?;
                if value[0] == "OK" && value[1] == event.event().id {
                    if value[2] == true {
                        return Ok(());
                    }
                    bail!("private relay refused the original envelope");
                }
            }
            Message::Ping(data) => socket.send(Message::Pong(data)).await?,
            Message::Close(_) => bail!("private relay closed before acknowledgment"),
            _ => {}
        }
    }
    bail!("private relay acknowledgment frame budget exhausted")
}

pub async fn delivery_step(ledger: &LedgerStore) -> Result<()> {
    let Some(work) = ledger.reserve_contact_delivery().await? else {
        return Ok(());
    };
    let event = crate::contact::validate_envelope(&work.envelope_json, &work.recipient)?;
    if work.relays.is_empty() || work.relays.len() > 8 {
        bail!("invalid retained private inbox policy");
    }
    for relay in &work.relays {
        crate::public_network::validate_destination(relay, "wss")?;
    }
    let result = select_ok(
        work.relays
            .iter()
            .map(|relay| Box::pin(publish_one(relay, &event))),
    )
    .await;
    let accepted = result.ok().map(|(relay, _)| relay);
    ledger
        .finish_contact_delivery(&work, accepted.as_deref())
        .await
}

pub async fn run(ledger: LedgerStore) -> Result<()> {
    loop {
        if delivery_step(&ledger).await.is_err() {
            tracing::warn!("private contact delivery retained for retry");
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contact::tests::fixture;

    #[tokio::test]
    async fn only_exact_positive_acknowledgment_confirms_original_ciphertext() {
        for accepted in [true, false] {
            let intent = fixture();
            let event = crate::contact::validate_envelope(&intent.envelope_json, &intent.recipient)
                .unwrap();
            let original = event.event().clone();
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let address = listener.local_addr().unwrap();
            let relay = tokio::spawn(async move {
                let (stream, _) = listener.accept().await.unwrap();
                let mut socket = tokio_tungstenite::accept_async(stream).await.unwrap();
                let text = socket.next().await.unwrap().unwrap().into_text().unwrap();
                let value: serde_json::Value = serde_json::from_str(&text).unwrap();
                assert_eq!(value[0], "EVENT");
                assert_eq!(value[1], serde_json::to_value(original).unwrap());
                socket
                    .send(Message::Text(
                        serde_json::json!(["OK", "unrelated-id", true, ""])
                            .to_string()
                            .into(),
                    ))
                    .await
                    .unwrap();
                socket
                    .send(Message::Text(
                        serde_json::json!(["OK", value[1]["id"], accepted, ""])
                            .to_string()
                            .into(),
                    ))
                    .await
                    .unwrap();
            });
            let (socket, _) = tokio_tungstenite::connect_async(format!("ws://{address}"))
                .await
                .unwrap();
            assert_eq!(publish_socket(socket, &event).await.is_ok(), accepted);
            relay.await.unwrap();
        }
    }
}
