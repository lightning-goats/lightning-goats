//! Bounded read-only NIP-61 discovery. Signed events remain unacquired input.
use anyhow::{Result, ensure};
use futures_util::{SinkExt, StreamExt};
use lightning_goats_nostr_event::VerifiedEvent;
use serde::Serialize;
use std::{collections::BTreeSet, net::SocketAddr, time::Duration};
use tokio::{
    io::{AsyncRead, AsyncWrite},
    net::TcpStream,
    time::{Instant, timeout, timeout_at},
};
use tokio_tungstenite::{
    WebSocketStream,
    tungstenite::{Message, protocol::WebSocketConfig},
};
use url::{Host, Url};

pub struct Filter {
    pub recipient: String,
    pub mints: Vec<String>,
    pub since: u64,
    pub until: u64,
    pub limit: u16,
}
#[derive(Serialize)]
struct WireFilter<'a> {
    kinds: [u16; 1],
    #[serde(rename = "#p")]
    recipient: [&'a str; 1],
    #[serde(rename = "#u")]
    mints: &'a [String],
    since: u64,
    until: u64,
    limit: u16,
}
impl Filter {
    pub fn validate(&self) -> Result<()> {
        lightning_goats_nostr_event::validate_public_key(&self.recipient)?;
        ensure!(
            !self.mints.is_empty()
                && self.mints.len() <= 16
                && self.since <= self.until
                && self.until <= i64::MAX as u64
                && (1..=256).contains(&self.limit),
            "invalid bounded nutzap filter"
        );
        let mut unique = BTreeSet::new();
        for mint in &self.mints {
            ensure!(
                mint.len() <= 2048 && !mint.chars().any(char::is_control) && unique.insert(mint),
                "invalid nutzap mint filter"
            );
            let url = Url::parse(mint).map_err(|_| anyhow::anyhow!("invalid mint filter URL"))?;
            ensure!(
                url.scheme() == "https"
                    && url.host().is_some()
                    && url.username().is_empty()
                    && url.password().is_none()
                    && url.query().is_none()
                    && url.fragment().is_none(),
                "invalid mint filter URL"
            );
        }
        let encoded = serde_json::to_vec(&WireFilter {
            kinds: [9321],
            recipient: [&self.recipient],
            mints: &self.mints,
            since: self.since,
            until: self.until,
            limit: self.limit,
        })?;
        // Reserve room for REQ and its fixed-length subscription UUID before DNS/TLS.
        ensure!(
            encoded.len() <= 32_704,
            "nutzap query exceeds request bound"
        );
        Ok(())
    }
    fn matches(&self, event: &VerifiedEvent) -> bool {
        let signed = event.event();
        signed.kind == 9321
            && (self.since..=self.until).contains(&signed.created_at)
            && event.singleton_tag("p", true).ok().flatten() == Some(self.recipient.as_str())
            && event
                .singleton_tag("u", true)
                .ok()
                .flatten()
                .is_some_and(|mint| self.mints.iter().any(|m| m == mint))
    }
}
pub struct Batch {
    pub events: Vec<VerifiedEvent>,
    pub complete: bool,
    pub saturated: bool,
}

pub fn validate_relay(raw: &str) -> Result<Url> {
    ensure!(
        raw.len() <= 2048 && !raw.chars().any(char::is_control),
        "invalid receive relay"
    );
    let url = Url::parse(raw).map_err(|_| anyhow::anyhow!("invalid receive relay"))?;
    ensure!(
        url.scheme() == "wss"
            && url.host().is_some()
            && url.port_or_known_default() == Some(443)
            && url.username().is_empty()
            && url.password().is_none()
            && url.fragment().is_none(),
        "receive relay must use secure port 443 without credentials or fragments"
    );
    match url.host() {
        Some(Host::Ipv4(ip)) => {
            crate::mint_policy::validate_connection_addresses(&[SocketAddr::new(ip.into(), 443)])?;
        }
        Some(Host::Ipv6(ip)) => {
            crate::mint_policy::validate_connection_addresses(&[SocketAddr::new(ip.into(), 443)])?;
        }
        Some(Host::Domain(host)) => ensure!(
            host.contains('.')
                && !host.ends_with('.')
                && ![".local", ".localhost", ".internal", ".home", ".onion"]
                    .iter()
                    .any(|suffix| host.ends_with(suffix)),
            "receive relay requires a public host"
        ),
        None => anyhow::bail!("receive relay host missing"),
    }
    Ok(url)
}

/// No proxy, redirect or second DNS lookup. Actual TCP uses only the checked address set.
pub async fn query(relay: &str, filter: &Filter) -> Result<Batch> {
    filter.validate()?;
    let url = validate_relay(relay)?;
    timeout(Duration::from_secs(20), async {
        let socket = connect_url(&url).await?;
        query_socket(socket, filter, Instant::now() + Duration::from_secs(10)).await
    })
    .await
    .map_err(|_| anyhow::anyhow!("nutzap relay query timed out"))?
    .map_err(|_| anyhow::anyhow!("nutzap relay query unavailable"))
}

/// Shared bounded public WebSocket transport for discovery and exact-event delivery.
pub(crate) async fn connect(
    relay: &str,
) -> Result<WebSocketStream<tokio_tungstenite::MaybeTlsStream<TcpStream>>> {
    let url = validate_relay(relay)?;
    connect_url(&url).await
}
async fn connect_url(
    url: &Url,
) -> Result<WebSocketStream<tokio_tungstenite::MaybeTlsStream<TcpStream>>> {
    timeout(Duration::from_secs(12), async {
        let addresses = match url.host() {
            Some(Host::Ipv4(ip)) => vec![SocketAddr::new(ip.into(), 443)],
            Some(Host::Ipv6(ip)) => vec![SocketAddr::new(ip.into(), 443)],
            Some(Host::Domain(host)) => {
                timeout(Duration::from_secs(4), tokio::net::lookup_host((host, 443)))
                    .await??
                    .take(33)
                    .collect()
            }
            None => anyhow::bail!("relay host missing"),
        };
        let addresses = crate::mint_policy::validate_connection_addresses(&addresses)?;
        let stream = timeout(
            Duration::from_secs(5),
            TcpStream::connect(addresses.as_slice()),
        )
        .await??;
        let limits = WebSocketConfig::default()
            .max_message_size(Some(131_072))
            .max_frame_size(Some(131_072));
        let (socket, _) = tokio_tungstenite::client_async_tls_with_config(
            url.as_str(),
            stream,
            Some(limits),
            None,
        )
        .await?;
        Ok(socket)
    })
    .await
    .map_err(|_| anyhow::anyhow!("relay connection timed out"))?
}

async fn query_socket<S: AsyncRead + AsyncWrite + Unpin>(
    mut socket: WebSocketStream<S>,
    filter: &Filter,
    deadline: Instant,
) -> Result<Batch> {
    filter.validate()?;
    let subscription = uuid::Uuid::new_v4().to_string();
    let wire = serde_json::to_string(&serde_json::json!([
        "REQ",
        subscription,
        WireFilter {
            kinds: [9321],
            recipient: [&filter.recipient],
            mints: &filter.mints,
            since: filter.since,
            until: filter.until,
            limit: filter.limit
        }
    ]))?;
    ensure!(wire.len() <= 32768, "nutzap query exceeds bound");
    timeout_at(deadline, socket.send(Message::Text(wire.into()))).await??;
    let mut batch = Batch {
        events: vec![],
        complete: false,
        saturated: false,
    };
    let mut unique = BTreeSet::new();
    let mut bytes = 0usize;
    for frame in 0..1024 {
        if frame == 1023 {
            batch.saturated = true;
        }
        let message = match timeout_at(deadline, socket.next()).await {
            Ok(Some(Ok(message))) => message,
            _ => break,
        };
        match message {
            Message::Text(text) => {
                bytes = bytes.saturating_add(text.len());
                if bytes > 4 * 1024 * 1024 {
                    batch.saturated = true;
                    break;
                }
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
                        batch.complete = true;
                        break;
                    }
                    Some("CLOSED") => break,
                    Some("EVENT") if parts.len() == 3 => {
                        let raw = serde_json::to_string(&parts[2])?;
                        let Ok(event) = VerifiedEvent::from_json(&raw) else {
                            continue;
                        };
                        if !filter.matches(&event) || !unique.insert(event.event().id.clone()) {
                            continue;
                        }
                        batch.events.push(event);
                        if batch.events.len() >= usize::from(filter.limit) {
                            batch.saturated = true;
                            break;
                        }
                    }
                    _ => {}
                }
            }
            Message::Ping(body) => {
                if !matches!(
                    timeout_at(deadline, socket.send(Message::Pong(body))).await,
                    Ok(Ok(()))
                ) {
                    break;
                }
            }
            Message::Binary(_) | Message::Frame(_) => {
                batch.saturated = true;
                break;
            }
            Message::Close(_) => break,
            _ => {}
        }
    }
    let _ = timeout(Duration::from_millis(500), socket.close(None)).await;
    batch.events.sort_by(|a, b| {
        a.event()
            .created_at
            .cmp(&b.event().created_at)
            .then_with(|| a.event().id.cmp(&b.event().id))
    });
    Ok(batch)
}

#[cfg(test)]
mod tests {
    use super::*;
    use cdk::secp256k1::{Keypair, Message as SecpMessage, Secp256k1, SecretKey};
    use lightning_goats_nostr_event::{SignedNostrEvent, canonical_bytes};
    use sha2::{Digest, Sha256};
    use tokio_tungstenite::tungstenite::protocol::Role;

    fn filter(limit: u16) -> Filter {
        Filter {
            recipient: cdk::nuts::SecretKey::from_slice(&[4; 32])
                .unwrap()
                .public_key()
                .x_only_public_key()
                .to_string(),
            mints: vec!["https://mint.example".into()],
            since: 10,
            until: 20,
            limit,
        }
    }
    fn signed(filter: &Filter, mint: &str, created_at: u64) -> SignedNostrEvent {
        let secp = Secp256k1::new();
        let key = Keypair::from_secret_key(&secp, &SecretKey::from_slice(&[3; 32]).unwrap());
        let mut event = SignedNostrEvent {
            id: String::new(),
            pubkey: key.x_only_public_key().0.to_string(),
            created_at,
            kind: 9321,
            tags: vec![
                vec!["p".into(), filter.recipient.clone()],
                vec!["u".into(), mint.into()],
            ],
            content: "synthetic discovery only".into(),
            sig: String::new(),
        };
        let hash: [u8; 32] = Sha256::digest(canonical_bytes(&event).unwrap()).into();
        event.id = hex::encode(hash);
        event.sig = secp
            .sign_schnorr_no_aux_rand(&SecpMessage::from_digest(hash), &key)
            .to_string();
        event
    }
    #[test]
    fn relay_and_filter_bounds_reject_private_or_unscoped_destinations() -> Result<()> {
        for relay in [
            "ws://relay.example",
            "wss://127.0.0.1",
            "wss://[::1]",
            "wss://[::ffff:127.0.0.1]",
            "wss://router.local",
            "wss://localhost",
            "wss://relay.example:444",
            "wss://user:secret@relay.example",
            "wss://relay.example/#private",
        ] {
            assert!(validate_relay(relay).is_err(), "accepted {relay}");
        }
        validate_relay("wss://relay.example/")?;
        let mut query = filter(32);
        query.validate()?;
        query.limit = 0;
        assert!(query.validate().is_err());
        query.limit = 257;
        assert!(query.validate().is_err());
        query.limit = 32;
        query.mints.push(query.mints[0].clone());
        assert!(query.validate().is_err());
        query.mints.clear();
        assert!(query.validate().is_err());
        query.mints = (0..16)
            .map(|index| {
                let prefix = format!("https://mint.example/{index}/");
                format!("{prefix}{}", "a".repeat(2048 - prefix.len()))
            })
            .collect();
        assert!(query.validate().is_err());
        Ok(())
    }
    #[tokio::test]
    async fn signed_scoped_events_deduplicate_and_eose_does_not_assert_acquisition() -> Result<()> {
        let query = filter(8);
        let (client, server) = tokio::io::duplex(262144);
        let client = WebSocketStream::from_raw_socket(client, Role::Client, None).await;
        let mut server = WebSocketStream::from_raw_socket(server, Role::Server, None).await;
        let first = signed(&query, "https://mint.example", 15);
        let earlier = signed(&query, "https://mint.example", 14);
        let wrong = signed(&query, "https://other.example", 15);
        let mut forged = first.clone();
        forged.sig = "ff".repeat(64);
        let (batch, served) = tokio::time::timeout(Duration::from_secs(3), async {
            tokio::join!(
                query_socket(client, &query, Instant::now() + Duration::from_secs(2)),
                async {
                    let request = server.next().await.unwrap()?.into_text()?;
                    let value: serde_json::Value = serde_json::from_str(&request)?;
                    assert_eq!(value[2]["kinds"], serde_json::json!([9321]));
                    assert_eq!(value[2]["#p"], serde_json::json!([query.recipient]));
                    assert_eq!(value[2]["#u"], serde_json::json!(query.mints));
                    let subscription = value[1].as_str().unwrap();
                    server
                        .send(Message::Text(
                            serde_json::json!(["EVENT", "wrong-subscription", first])
                                .to_string()
                                .into(),
                        ))
                        .await?;
                    for event in [&forged, &wrong, &first, &first, &earlier] {
                        server
                            .send(Message::Text(
                                serde_json::json!(["EVENT", subscription, event])
                                    .to_string()
                                    .into(),
                            ))
                            .await?;
                    }
                    server
                        .send(Message::Text(
                            serde_json::json!(["EOSE", subscription]).to_string().into(),
                        ))
                        .await?;
                    let _ = server.next().await;
                    Ok::<_, anyhow::Error>(())
                }
            )
        })
        .await?;
        served?;
        let batch = batch?;
        assert!(batch.complete && !batch.saturated);
        assert_eq!(batch.events.len(), 2);
        assert_eq!(batch.events[0].event().created_at, 14);
        assert_eq!(batch.events[1].event().id, first.id);
        Ok(())
    }
    #[tokio::test]
    async fn saturated_binary_and_closed_queries_never_claim_complete_coverage() -> Result<()> {
        for mode in ["limit", "binary", "closed"] {
            let query = filter(1);
            let (client, server) = tokio::io::duplex(262144);
            let client = WebSocketStream::from_raw_socket(client, Role::Client, None).await;
            let mut server = WebSocketStream::from_raw_socket(server, Role::Server, None).await;
            let (batch, served) = tokio::time::timeout(Duration::from_secs(3), async {
                tokio::join!(
                    query_socket(client, &query, Instant::now() + Duration::from_secs(2)),
                    async {
                        let request = server.next().await.unwrap()?.into_text()?;
                        let value: serde_json::Value = serde_json::from_str(&request)?;
                        let message = match mode {
                            "limit" => Message::Text(
                                serde_json::json!([
                                    "EVENT",
                                    value[1],
                                    signed(&query, "https://mint.example", 15)
                                ])
                                .to_string()
                                .into(),
                            ),
                            "binary" => Message::Binary(vec![0; 16].into()),
                            _ => Message::Text(
                                serde_json::json!(["CLOSED", value[1], "fixture refusal"])
                                    .to_string()
                                    .into(),
                            ),
                        };
                        server.send(message).await?;
                        let _ = server.next().await;
                        Ok::<_, anyhow::Error>(())
                    }
                )
            })
            .await?;
            served?;
            let batch = batch?;
            assert!(!batch.complete);
            assert_eq!(batch.saturated, mode != "closed");
        }
        Ok(())
    }
}
