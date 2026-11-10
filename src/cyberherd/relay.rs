//! Bounded read-only NIP-01 queries. This adapter holds no signer, money or
//! gateway capability and never trusts a relay's event without native verification.
use std::{
    collections::{BTreeMap, BTreeSet},
    time::Duration,
};

use anyhow::{Context, Result, bail};
use futures_util::{SinkExt, StreamExt, future::join_all};
use serde::Serialize;
use tokio::{
    io::{AsyncRead, AsyncWrite},
    time::{Instant, timeout, timeout_at},
};
use tokio_tungstenite::{WebSocketStream, tungstenite::Message};

use super::targets::ConversationTarget;
use crate::{
    nostr_event::{Coordinate, VerifiedEvent, validate_hex, validate_public_key},
    public_network,
};

#[derive(Debug, Clone, Serialize)]
pub struct Filter {
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub ids: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub authors: Vec<String>,
    pub kinds: Vec<u16>,
    #[serde(rename = "#e", skip_serializing_if = "Vec::is_empty")]
    pub events: Vec<String>,
    #[serde(rename = "#a", skip_serializing_if = "Vec::is_empty")]
    pub coordinates: Vec<String>,
    #[serde(rename = "#p", skip_serializing_if = "Vec::is_empty")]
    pub people: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub since: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub until: Option<u64>,
    pub limit: u16,
}

impl Filter {
    pub fn validate(&self) -> Result<()> {
        if self.ids.len() > 128
            || self.authors.len() > 8
            || self.events.len() > 128
            || self.coordinates.len() > 128
            || self.people.len() > 8
            || self.kinds.is_empty()
            || self.kinds.len() > 8
            || !(1..=512).contains(&self.limit)
            || self.since.is_some_and(|v| v > i64::MAX as u64)
            || self.until.is_some_and(|v| v > i64::MAX as u64)
            || self
                .since
                .zip(self.until)
                .is_some_and(|(start, end)| start > end)
            || (self.ids.is_empty()
                && self.authors.is_empty()
                && self.events.is_empty()
                && self.coordinates.is_empty()
                && self.people.is_empty())
        {
            bail!("unbounded or invalid Nostr query filter");
        }
        for id in self.ids.iter().chain(&self.events) {
            validate_hex(id, 32)?;
        }
        for author in self.authors.iter().chain(&self.people) {
            validate_public_key(author)?;
        }
        for coordinate in &self.coordinates {
            let _: Coordinate = coordinate.parse()?;
        }
        Ok(())
    }
    pub fn matches(&self, event: &VerifiedEvent) -> bool {
        let signed = event.event();
        (self.ids.is_empty() || self.ids.contains(&signed.id))
            && (self.authors.is_empty() || self.authors.contains(&signed.pubkey))
            && self
                .kinds
                .iter()
                .any(|kind| u64::from(*kind) == signed.kind)
            && self.since.is_none_or(|since| signed.created_at >= since)
            && self.until.is_none_or(|until| signed.created_at <= until)
            && (self.events.is_empty()
                || event
                    .tag_values("e")
                    .any(|id| self.events.iter().any(|value| value == id)))
            && (self.coordinates.is_empty()
                || event
                    .tag_values("a")
                    .any(|id| self.coordinates.iter().any(|value| value == id)))
            && (self.people.is_empty()
                || event
                    .tag_values("p")
                    .any(|key| self.people.iter().any(|value| value == key)))
    }
}

/// e and a are OR alternatives, not one NIP-01 filter that requires both.
pub fn engagement_filters(
    targets: &[ConversationTarget],
    aliases: &[String],
    since: u64,
    until: u64,
) -> Result<Vec<Filter>> {
    if targets.len() > 3 || aliases.len() > 125 {
        bail!("engagement target/alias bound exceeded");
    }
    let ids: Vec<_> = targets
        .iter()
        .map(|target| target.event_id.clone())
        .chain(aliases.iter().cloned())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let coordinates: Vec<_> = targets
        .iter()
        .filter_map(|target| target.coordinate.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    engagement_scope_filters(ids, coordinates, since, until)
}

pub(crate) fn engagement_scope_filters(
    ids: Vec<String>,
    coordinates: Vec<String>,
    since: u64,
    until: u64,
) -> Result<Vec<Filter>> {
    if ids.len() > 128 || coordinates.len() > 128 {
        bail!("engagement scope bound exceeded");
    }
    let base = Filter {
        ids: vec![],
        authors: vec![],
        kinds: vec![6, 7, 16],
        events: vec![],
        coordinates: vec![],
        people: vec![],
        since: Some(since),
        until: Some(until),
        limit: 256,
    };
    let mut filters = Vec::new();
    if !ids.is_empty() {
        let mut filter = base.clone();
        filter.events = ids;
        filter.validate()?;
        filters.push(filter);
    }
    if !coordinates.is_empty() {
        let mut filter = base;
        filter.coordinates = coordinates;
        filter.validate()?;
        filters.push(filter);
    }
    Ok(filters)
}

#[derive(Debug)]
pub struct ReadBatch {
    pub events: Vec<VerifiedEvent>,
    pub completed_relays: usize,
    pub failed_relays: usize,
    /// Caller must paginate/backfill rather than advance its cursor past a cap.
    pub saturated: bool,
}

#[derive(Debug, Clone)]
pub struct Reader {
    relays: Vec<String>,
}

pub trait Query {
    fn query<'a>(
        &'a self,
        filters: &'a [Filter],
    ) -> impl std::future::Future<Output = Result<ReadBatch>> + Send + 'a;
}

impl Query for Reader {
    async fn query(&self, filters: &[Filter]) -> Result<ReadBatch> {
        Reader::query(self, filters).await
    }
}

impl Reader {
    pub fn new(relays: Vec<String>) -> Result<Self> {
        if relays.is_empty() || relays.len() > 8 {
            bail!("Nostr reading requires one to eight configured relays");
        }
        let mut normalized = BTreeSet::new();
        for relay in relays {
            normalized.insert(public_network::validate_destination(&relay, "wss")?.to_string());
        }
        Ok(Self {
            relays: normalized.into_iter().collect(),
        })
    }
    pub fn relay_count(&self) -> usize {
        self.relays.len()
    }
    pub async fn query(&self, filters: &[Filter]) -> Result<ReadBatch> {
        if filters.is_empty() || filters.len() > 8 {
            bail!("Nostr query requires one to eight bounded filters");
        }
        for filter in filters {
            filter.validate()?;
        }
        let requests = join_all(self.relays.iter().map(|relay| query_one(relay, filters))).await;
        let mut unique = BTreeMap::new();
        let mut result = ReadBatch {
            events: vec![],
            completed_relays: 0,
            failed_relays: 0,
            saturated: false,
        };
        let mut bytes = 0usize;
        for request in requests {
            match request {
                Ok(batch) => {
                    result.completed_relays += usize::from(batch.complete);
                    result.failed_relays += usize::from(!batch.complete);
                    result.saturated |= batch.saturated;
                    for event in batch.events {
                        if unique.contains_key(&event.event().id) {
                            continue;
                        }
                        let size = serde_json::to_vec(event.event())?.len();
                        if unique.len() >= 1024 || bytes.saturating_add(size) > 8 * 1024 * 1024 {
                            result.saturated = true;
                            continue;
                        }
                        bytes += size;
                        unique.insert(event.event().id.clone(), event);
                    }
                }
                Err(_) => result.failed_relays += 1,
            }
        }
        result.events = unique.into_values().collect();
        result.events.sort_by(|a, b| {
            a.event()
                .created_at
                .cmp(&b.event().created_at)
                .then_with(|| a.event().id.cmp(&b.event().id))
        });
        Ok(result)
    }
}

struct RelayBatch {
    events: Vec<VerifiedEvent>,
    complete: bool,
    saturated: bool,
}

async fn query_one(relay: &str, filters: &[Filter]) -> Result<RelayBatch> {
    let deadline = Instant::now() + Duration::from_secs(20);
    let socket = timeout_at(deadline, public_network::connect_relay(relay))
        .await
        .context("Nostr query connection expired")??;
    query_socket(socket, filters, deadline).await
}

async fn query_socket<S>(
    mut socket: WebSocketStream<S>,
    filters: &[Filter],
    deadline: Instant,
) -> Result<RelayBatch>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let subscription = uuid::Uuid::new_v4().to_string();
    let mut request = vec![serde_json::json!("REQ"), serde_json::json!(subscription)];
    for filter in filters {
        filter.validate()?;
        request.push(serde_json::to_value(filter)?);
    }
    let wire = serde_json::to_string(&request)?;
    if wire.len() > 32768 {
        bail!("Nostr query exceeds request bound");
    }
    timeout_at(deadline, socket.send(Message::Text(wire.into())))
        .await
        .context("Nostr query send expired")??;
    let mut result = RelayBatch {
        events: vec![],
        complete: false,
        saturated: false,
    };
    let mut unique = BTreeSet::new();
    let mut bytes = 0usize;
    let mut counts = vec![0u16; filters.len()];
    for frame in 0..2048 {
        if frame == 2047 {
            result.saturated = true;
        }
        let message = match timeout_at(deadline, socket.next()).await {
            Ok(Some(Ok(message))) => message,
            _ => break,
        };
        match message {
            Message::Text(text) => {
                bytes = bytes.saturating_add(text.len());
                if bytes > 4 * 1024 * 1024 {
                    result.saturated = true;
                    break;
                }
                let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) else {
                    continue;
                };
                let Some(array) = value.as_array() else {
                    continue;
                };
                if array.get(1).and_then(|v| v.as_str()) != Some(&subscription) {
                    continue;
                }
                match array.first().and_then(|v| v.as_str()) {
                    Some("EOSE") if array.len() == 2 => {
                        result.complete = true;
                        break;
                    }
                    Some("CLOSED") => break,
                    Some("EVENT") if array.len() == 3 => {
                        let event_wire = serde_json::to_string(&array[2])?;
                        let Ok(event) = VerifiedEvent::from_json(&event_wire) else {
                            continue;
                        };
                        if !filters.iter().any(|filter| filter.matches(&event))
                            || !unique.insert(event.event().id.clone())
                        {
                            continue;
                        }
                        for (filter, count) in filters.iter().zip(&mut counts) {
                            if filter.matches(&event) {
                                *count = count.saturating_add(1);
                                if *count >= filter.limit {
                                    result.saturated = true;
                                }
                            }
                        }
                        result.events.push(event);
                        if result.events.len() >= 512 {
                            result.saturated = true;
                            break;
                        }
                    }
                    _ => {}
                }
            }
            Message::Ping(bytes) => {
                if !matches!(
                    timeout_at(deadline, socket.send(Message::Pong(bytes))).await,
                    Ok(Ok(()))
                ) {
                    break;
                }
            }
            Message::Close(_) => break,
            Message::Binary(_) | Message::Frame(_) => {
                result.saturated = true;
                break;
            }
            _ => {}
        }
    }
    let _ = timeout(Duration::from_millis(500), async {
        socket
            .send(Message::Text(
                serde_json::json!(["CLOSE", subscription])
                    .to_string()
                    .into(),
            ))
            .await?;
        socket.close(None).await
    })
    .await;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use bitcoin::secp256k1::{Keypair, Message as SecpMessage, Secp256k1, SecretKey};
    use sha2::{Digest, Sha256};
    use tokio_tungstenite::tungstenite::protocol::Role;

    fn event(target: &str) -> VerifiedEvent {
        let key =
            Keypair::from_secret_key(&Secp256k1::new(), &SecretKey::from_slice(&[2; 32]).unwrap());
        let mut event = crate::nostr::SignedNostrEvent {
            id: String::new(),
            pubkey: key.x_only_public_key().0.to_string(),
            created_at: 1000,
            kind: 7,
            tags: vec![vec!["e".into(), target.into()]],
            content: "+".into(),
            sig: String::new(),
        };
        let digest: [u8; 32] =
            Sha256::digest(crate::nostr_event::canonical_bytes(&event).unwrap()).into();
        event.id = hex::encode(digest);
        event.sig = Secp256k1::new()
            .sign_schnorr_no_aux_rand(&SecpMessage::from_digest(digest), &key)
            .to_string();
        VerifiedEvent::verify(event).unwrap()
    }

    fn filter() -> Filter {
        Filter {
            ids: vec![],
            authors: vec![],
            kinds: vec![7],
            events: vec!["12".repeat(32)],
            coordinates: vec![],
            people: vec![],
            since: Some(900),
            until: Some(1100),
            limit: 10,
        }
    }

    #[tokio::test]
    async fn read_transport_checks_subscription_signature_and_filter_before_accepting() {
        let (client, server) = tokio::io::duplex(262144);
        let good = event(&"12".repeat(32));
        let expected = good.event().id.clone();
        let foreign = event(&"13".repeat(32));
        let server = tokio::spawn(async move {
            let mut socket = WebSocketStream::from_raw_socket(server, Role::Server, None).await;
            let request = socket.next().await.unwrap().unwrap();
            let request: serde_json::Value =
                serde_json::from_str(request.to_text().unwrap()).unwrap();
            assert_eq!(request[0], "REQ");
            let sub = request[1].as_str().unwrap();
            let mut bad = serde_json::to_value(good.event()).unwrap();
            bad["sig"] = serde_json::json!("00".repeat(64));
            for payload in [
                serde_json::json!(["EOSE", "wrong-subscription"]),
                serde_json::json!(["EVENT", sub, bad]),
                serde_json::json!(["EVENT", sub, foreign.event()]),
                serde_json::json!(["EVENT", sub, good.event()]),
                serde_json::json!(["EVENT", sub, good.event()]),
                serde_json::json!(["EOSE", sub]),
            ] {
                socket
                    .send(Message::Text(payload.to_string().into()))
                    .await
                    .unwrap();
            }
            let close = socket.next().await.unwrap().unwrap();
            let close: serde_json::Value = serde_json::from_str(close.to_text().unwrap()).unwrap();
            assert_eq!(close[0], "CLOSE");
        });
        let socket = WebSocketStream::from_raw_socket(client, Role::Client, None).await;
        let batch = query_socket(socket, &[filter()], Instant::now() + Duration::from_secs(2))
            .await
            .unwrap();
        server.await.unwrap();
        assert!(batch.complete);
        assert!(!batch.saturated);
        assert_eq!(batch.events.len(), 1);
        assert_eq!(batch.events[0].event().id, expected);
    }

    #[tokio::test]
    async fn capped_or_closed_read_does_not_claim_complete_backfill() {
        let (client, server) = tokio::io::duplex(262144);
        let server = tokio::spawn(async move {
            let mut socket = WebSocketStream::from_raw_socket(server, Role::Server, None).await;
            socket.next().await.unwrap().unwrap();
            socket
                .send(Message::Binary(vec![0; 32].into()))
                .await
                .unwrap();
            let _ = socket.next().await;
        });
        let socket = WebSocketStream::from_raw_socket(client, Role::Client, None).await;
        let batch = query_socket(socket, &[filter()], Instant::now() + Duration::from_secs(2))
            .await
            .unwrap();
        server.await.unwrap();
        assert!(!batch.complete);
        assert!(batch.saturated);
        assert!(batch.events.is_empty());
    }

    #[test]
    fn engagement_query_uses_separate_or_filters_and_refuses_broad_queries() {
        let key = event(&"12".repeat(32)).event().pubkey.clone();
        let coordinate = format!("30311:{key}:show");
        let target = ConversationTarget {
            event_id: "14".repeat(32),
            kind: 30311,
            coordinate: Some(coordinate.clone()),
            payee_pubkey: key,
        };
        let filters = engagement_filters(&[target], &["15".repeat(32)], 900, 1100).unwrap();
        assert_eq!(filters.len(), 2);
        assert!(filters[0].coordinates.is_empty());
        assert_eq!(filters[0].events.len(), 2);
        assert!(filters[1].events.is_empty());
        assert_eq!(filters[1].coordinates, vec![coordinate]);
        let mut broad = filter();
        broad.events.clear();
        assert!(broad.validate().is_err());
        assert!(Reader::new(vec!["wss://10.0.0.1/".into()]).is_err());
        assert!(Reader::new(vec!["wss://user:secret@relay.example/".into()]).is_err());
        assert!(Reader::new(vec![]).is_err());
    }
}
