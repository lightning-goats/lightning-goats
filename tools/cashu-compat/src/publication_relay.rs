//! Bounded metadata discovery and exact signed-event relay acknowledgment.
use anyhow::{Result, ensure};
use futures_util::{SinkExt, StreamExt};
use lightning_goats_nostr_event::{VerifiedEvent, validate_public_key};
use std::{collections::BTreeMap, time::Duration};
use tokio::{
    io::{AsyncRead, AsyncWrite},
    time::{Instant, timeout, timeout_at},
};
use tokio_tungstenite::{WebSocketStream, tungstenite::Message};

/// Shared with core NIP-65 routing; callers never treat write-only relays as read routes.
pub fn sender_reads(event: &VerifiedEvent, sender: &str, now: i64) -> Result<Vec<String>> {
    Ok(
        lightning_goats_nostr_event::relay_list::directions(event, sender, now, |raw| {
            let url = crate::relay::validate_relay(raw).ok()?;
            (url.host_str() != Some("sendit.nosflare.com") && url.query().is_none())
                .then(|| url.to_string())
        })?
        .read,
    )
}
pub struct Discovery {
    pub read_relays: Vec<String>,
    pub signed_list: Option<String>,
}
/// Query only configured discovery relays, never sender-supplied URL hints.
pub async fn discover(relays: &[String], sender: &str, now: i64) -> Result<Discovery> {
    validate_public_key(sender)?;
    ensure!(
        now >= 0 && !relays.is_empty() && relays.len() <= 8,
        "invalid relay discovery scope"
    );
    for relay in relays {
        crate::relay::validate_relay(relay)?;
    }
    let batches = futures_util::future::join_all(relays.iter().map(|relay| async {
        timeout(Duration::from_secs(20), async {
            let socket = crate::relay::connect(relay).await?;
            list_socket(
                socket,
                sender,
                now,
                Instant::now() + Duration::from_secs(10),
            )
            .await
        })
        .await
        .map_err(|_| anyhow::anyhow!("relay list deadline exceeded"))?
    }))
    .await;
    let mut completed = 0;
    let mut events = BTreeMap::new();
    for batch in batches.into_iter().flatten() {
        completed += 1;
        for event in batch {
            events.insert(event.event().id.clone(), event);
        }
    }
    ensure!(completed > 0, "no complete relay-list discovery");
    let best = events
        .into_values()
        .filter(|event| sender_reads(event, sender, now).is_ok())
        .max_by(|a, b| {
            a.event()
                .created_at
                .cmp(&b.event().created_at)
                .then_with(|| b.event().id.cmp(&a.event().id))
        });
    match best {
        Some(event) => Ok(Discovery {
            read_relays: sender_reads(&event, sender, now)?,
            signed_list: Some(serde_json::to_string(event.event())?),
        }),
        None => Ok(Discovery {
            read_relays: vec![],
            signed_list: None,
        }),
    }
}
async fn list_socket<S: AsyncRead + AsyncWrite + Unpin>(
    mut socket: WebSocketStream<S>,
    sender: &str,
    now: i64,
    deadline: Instant,
) -> Result<Vec<VerifiedEvent>> {
    let subscription = uuid::Uuid::new_v4().to_string();
    let request = serde_json::json!(["REQ",subscription,{"authors":[sender],"kinds":[10002],"until":now,"limit":8}]);
    timeout_at(
        deadline,
        socket.send(Message::Text(request.to_string().into())),
    )
    .await??;
    let mut events = BTreeMap::new();
    let mut bytes = 0;
    for _ in 0..128 {
        let message = timeout_at(deadline, socket.next())
            .await?
            .ok_or_else(|| anyhow::anyhow!("relay list ended before EOSE"))??;
        if let Message::Text(text) = message {
            bytes += text.len();
            ensure!(
                bytes <= 1_048_576 && text.len() <= 131_072,
                "relay list response exceeds bound"
            );
            let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) else {
                continue;
            };
            let Some(parts) = value.as_array() else {
                continue;
            };
            if parts.get(1).and_then(|v| v.as_str()) != Some(&subscription) {
                continue;
            }
            match parts.first().and_then(|v| v.as_str()) {
                Some("EOSE") if parts.len() == 2 => {
                    ensure!(events.len() < 8, "relay list result saturated");
                    return Ok(events.into_values().collect());
                }
                Some("EVENT") if parts.len() == 3 => {
                    if let Ok(event) = VerifiedEvent::from_json(&parts[2].to_string())
                        && event.event().kind == 10002
                        && event.event().pubkey == sender
                        && event.event().created_at <= now as u64
                    {
                        events.insert(event.event().id.clone(), event);
                        ensure!(events.len() <= 8, "relay list result exceeds bound");
                    }
                }
                Some("CLOSED") => anyhow::bail!("relay list query refused"),
                _ => {}
            }
        }
    }
    anyhow::bail!("relay list frame limit exceeded")
}
/// The caller retains this exact raw event and separately journals a positive OK.
pub async fn publish(relay: &str, raw: &str) -> Result<()> {
    let event = VerifiedEvent::from_json(raw)?;
    ensure!(
        matches!(event.event().kind, 10019 | 7376),
        "unsupported Cashu publication kind"
    );
    timeout(Duration::from_secs(20), async {
        let socket = crate::relay::connect(relay).await?;
        publish_socket(
            socket,
            raw,
            &event.event().id,
            Instant::now() + Duration::from_secs(10),
        )
        .await
    })
    .await
    .map_err(|_| anyhow::anyhow!("Cashu relay publication timed out"))?
}
async fn publish_socket<S: AsyncRead + AsyncWrite + Unpin>(
    mut socket: WebSocketStream<S>,
    raw: &str,
    id: &str,
    deadline: Instant,
) -> Result<()> {
    let frame = format!("[\"EVENT\",{raw}]");
    timeout_at(deadline, socket.send(Message::Text(frame.into()))).await??;
    let mut bytes = 0;
    for _ in 0..64 {
        let message = timeout_at(deadline, socket.next())
            .await?
            .ok_or_else(|| anyhow::anyhow!("relay closed without acknowledgment"))??;
        if let Message::Text(text) = message {
            bytes += text.len();
            ensure!(
                bytes <= 262_144 && text.len() <= 131_072,
                "relay acknowledgment exceeds bound"
            );
            let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) else {
                continue;
            };
            let Some(parts) = value.as_array() else {
                continue;
            };
            if parts.len() == 4 && parts[0].as_str() == Some("OK") && parts[1].as_str() == Some(id)
            {
                ensure!(
                    parts[2].as_bool() == Some(true) && parts[3].is_string(),
                    "relay rejected Cashu event"
                );
                return Ok(());
            }
        }
    }
    anyhow::bail!("relay acknowledgment frame limit exceeded")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::publication::{
        Unsigned,
        tests::{announcement, owner, sign},
    };
    use tokio_tungstenite::tungstenite::protocol::Role;
    fn list(at: u64, tags: Vec<Vec<String>>) -> VerifiedEvent {
        VerifiedEvent::from_json(
            &sign(Unsigned {
                pubkey: owner(),
                created_at: at,
                kind: 10002,
                tags,
                content: String::new(),
            })
            .unwrap(),
        )
        .unwrap()
    }
    #[test]
    fn shared_direction_policy_rejects_expired_wrong_owner_and_write_only_routes() -> Result<()> {
        let event = list(
            100,
            vec![
                vec!["r".into(), "wss://both.example".into()],
                vec!["r".into(), "wss://read.example".into(), "read".into()],
                vec!["r".into(), "wss://write.example".into(), "write".into()],
                vec!["r".into(), "wss://10.8.0.6".into()],
                vec!["r".into(), "wss://sendit.nosflare.com".into()],
                vec!["r".into(), "wss://token.example/?private=x".into()],
            ],
        );
        assert_eq!(
            sender_reads(&event, &owner(), 100)?,
            vec!["wss://both.example/", "wss://read.example/"]
        );
        assert!(sender_reads(&event, &owner(), 99).is_err());
        let other = cdk::nuts::SecretKey::from_slice(&[2; 32])?
            .public_key()
            .x_only_public_key()
            .to_string();
        assert!(sender_reads(&event, &other, 100).is_err());
        for expiry in ["100", "malformed"] {
            assert!(
                sender_reads(
                    &list(100, vec![vec!["expiration".into(), expiry.into()]]),
                    &owner(),
                    100
                )
                .is_err()
            );
        }
        Ok(())
    }
    #[tokio::test]
    async fn discovery_requires_matching_eose_and_ignores_forged_metadata() -> Result<()> {
        for saturated in [false, true] {
            let (client, server) = tokio::io::duplex(262144);
            let client = WebSocketStream::from_raw_socket(client, Role::Client, None).await;
            let mut server = WebSocketStream::from_raw_socket(server, Role::Server, None).await;
            let owner = owner();
            let (result, served) = tokio::join!(
                list_socket(client, &owner, 100, Instant::now() + Duration::from_secs(2)),
                async {
                    let request = server.next().await.unwrap()?.into_text()?;
                    let request: serde_json::Value = serde_json::from_str(&request)?;
                    let sub = &request[1];
                    server
                        .send(Message::Text(
                            serde_json::json!(["EOSE", "wrong-subscription"])
                                .to_string()
                                .into(),
                        ))
                        .await?;
                    let mut forged = list(1, vec![]).event().clone();
                    forged.sig = "ff".repeat(64);
                    server
                        .send(Message::Text(
                            serde_json::json!(["EVENT", sub, forged]).to_string().into(),
                        ))
                        .await?;
                    for at in 1..=if saturated { 8 } else { 1 } {
                        let event = list(
                            at,
                            vec![vec!["r".into(), "wss://read.example".into(), "read".into()]],
                        );
                        server
                            .send(Message::Text(
                                serde_json::json!(["EVENT", sub, event.event()])
                                    .to_string()
                                    .into(),
                            ))
                            .await?;
                    }
                    server
                        .send(Message::Text(
                            serde_json::json!(["EOSE", sub]).to_string().into(),
                        ))
                        .await?;
                    Ok::<_, anyhow::Error>(())
                }
            );
            served?;
            if saturated {
                assert!(result.is_err());
            } else {
                assert_eq!(result?.len(), 1);
            }
        }
        let (client, _server) = tokio::io::duplex(262144);
        let socket = WebSocketStream::from_raw_socket(client, Role::Client, None).await;
        assert!(
            list_socket(
                socket,
                &owner(),
                100,
                Instant::now() + Duration::from_millis(20)
            )
            .await
            .is_err()
        );
        Ok(())
    }
    #[tokio::test]
    async fn publication_preserves_wire_bytes_and_requires_positive_exact_id_ack() -> Result<()> {
        let raw = format!(" \n{}\n", sign(announcement().unsigned(None)?)?);
        let id = VerifiedEvent::from_json(&raw)?.event().id.clone();
        for accepted in [true, false] {
            let (client, server) = tokio::io::duplex(262144);
            let client = WebSocketStream::from_raw_socket(client, Role::Client, None).await;
            let mut server = WebSocketStream::from_raw_socket(server, Role::Server, None).await;
            let (result, served) = tokio::join!(
                publish_socket(client, &raw, &id, Instant::now() + Duration::from_secs(2)),
                async {
                    let wire = server.next().await.unwrap()?.into_text()?;
                    assert_eq!(wire.as_str(), format!("[\"EVENT\",{raw}]"));
                    server
                        .send(Message::Text(
                            serde_json::json!(["OK", "ab".repeat(32), true, ""])
                                .to_string()
                                .into(),
                        ))
                        .await?;
                    server
                        .send(Message::Text(
                            serde_json::json!(["OK", id, accepted, "bounded fixture"])
                                .to_string()
                                .into(),
                        ))
                        .await?;
                    Ok::<_, anyhow::Error>(())
                }
            );
            served?;
            assert_eq!(result.is_ok(), accepted);
        }
        let (client, _server) = tokio::io::duplex(262144);
        let client = WebSocketStream::from_raw_socket(client, Role::Client, None).await;
        assert!(
            publish_socket(
                client,
                &raw,
                &id,
                Instant::now() + Duration::from_millis(20)
            )
            .await
            .is_err()
        );
        Ok(())
    }
}
