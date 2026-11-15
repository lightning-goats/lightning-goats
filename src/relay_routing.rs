//! Signed relay-list discovery for addressed public events. This performs only
//! metadata queries, never downstream visibility measurements or signing.
use crate::{
    cyberherd::relay::{Filter, Query, Reader},
    nostr_event::{VerifiedEvent, validate_public_key},
};
use anyhow::{Result, bail};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};
use tokio::sync::Mutex;

const MAX_PEOPLE: usize = 8;
const MAX_USER_RELAYS: usize = 4;
const MAX_CACHE: usize = 256;
const REFRESH_SECONDS: i64 = 900;

/// Send It is write-only. It is never a source of relay lists or a DM inbox.
pub fn is_broadcaster(raw: &str) -> bool {
    reqwest::Url::parse(raw)
        .ok()
        .is_some_and(|u| u.host_str() == Some("sendit.nosflare.com"))
}

/// Bound addressed identities, including NIP-19 npub/nprofile mentions. Relay
/// hints inside an nprofile are discovery hints, not authority to deliver DMs.
pub fn people(content: &str, tags: &[Vec<String>]) -> Vec<String> {
    let mut keys = BTreeSet::new();
    for tag in tags {
        if tag.first().map(String::as_str) == Some("p")
            && let Some(key) = tag.get(1)
            && validate_public_key(key).is_ok()
        {
            keys.insert(key.clone());
        }
    }
    for part in content.split("nostr:").skip(1).take(32) {
        let encoded: String = part
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric())
            .take(2048)
            .collect();
        let Ok((hrp, bytes)) = bitcoin::bech32::decode(&encoded) else {
            continue;
        };
        let key = if hrp.as_str().eq_ignore_ascii_case("npub") && bytes.len() == 32 {
            Some(hex::encode(&bytes))
        } else if hrp.as_str().eq_ignore_ascii_case("nprofile") {
            let mut offset = 0;
            let mut key = None;
            while offset + 2 <= bytes.len() {
                let kind = bytes[offset];
                let size = usize::from(bytes[offset + 1]);
                offset += 2;
                if offset + size > bytes.len() {
                    key = None;
                    break;
                }
                if kind == 0 && size == 32 {
                    key = Some(hex::encode(&bytes[offset..offset + size]));
                }
                offset += size;
            }
            key
        } else {
            None
        };
        if let Some(key) = key
            && validate_public_key(&key).is_ok()
        {
            keys.insert(key);
        }
    }
    keys.into_iter().take(MAX_PEOPLE).collect()
}

pub fn public_relays(raw: &[String]) -> Result<Vec<String>> {
    if raw.is_empty() || raw.len() > 48 {
        bail!("public relay plan exceeds bound");
    }
    raw.iter()
        .map(|r| Ok(crate::public_network::validate_destination(r, "wss")?.to_string()))
        .collect::<Result<BTreeSet<_>>>()
        .map(|s| s.into_iter().collect())
}

#[derive(Debug, Clone)]
pub struct RelayList {
    event: VerifiedEvent,
    pub read: Vec<String>,
    pub write: Vec<String>,
}
impl RelayList {
    pub fn parse(event: &VerifiedEvent, author: &str, now: i64) -> Result<Self> {
        validate_public_key(author)?;
        if now < 0
            || event.event().pubkey != author
            || event.event().kind != 10002
            || event.event().created_at > now as u64
        {
            bail!("relay list author, kind or time differs");
        }
        if event
            .tag_values("expiration")
            .any(|v| v.parse::<i64>().map_or(true, |t| t <= now))
        {
            bail!("relay list is expired or malformed");
        }
        let mut read = BTreeSet::new();
        let mut write = BTreeSet::new();
        for tag in &event.event().tags {
            if tag.first().map(String::as_str) != Some("r") || !(2..=3).contains(&tag.len()) {
                continue;
            }
            let Ok(url) = crate::public_network::validate_destination(&tag[1], "wss") else {
                continue;
            };
            if is_broadcaster(url.as_str()) {
                continue;
            }
            match tag.get(2).map(String::as_str) {
                None => {
                    read.insert(url.to_string());
                    write.insert(url.to_string());
                }
                Some("read") => {
                    read.insert(url.to_string());
                }
                Some("write") => {
                    write.insert(url.to_string());
                }
                _ => {}
            }
        }
        Ok(Self {
            event: event.clone(),
            read: read.into_iter().take(MAX_USER_RELAYS).collect(),
            write: write.into_iter().take(MAX_USER_RELAYS).collect(),
        })
    }
    pub fn event(&self) -> &VerifiedEvent {
        &self.event
    }
}

#[derive(Debug, Clone)]
struct Cached {
    checked: i64,
    list: Option<RelayList>,
}
#[derive(Debug, Clone)]
pub struct Router<Q = Reader> {
    source: Q,
    cache: Arc<Mutex<BTreeMap<String, Cached>>>,
}
impl Router<Reader> {
    pub fn new(relays: &[String]) -> Result<Self> {
        Self::with_source(Reader::new(
            relays
                .iter()
                .filter(|r| !is_broadcaster(r))
                .cloned()
                .collect(),
        )?)
    }
}
impl<Q: Query> Router<Q> {
    pub fn with_source(source: Q) -> Result<Self> {
        Ok(Self {
            source,
            cache: Arc::new(Mutex::new(BTreeMap::new())),
        })
    }
    pub async fn lists(&self, people: &[String], now: i64) -> Result<BTreeMap<String, RelayList>> {
        if now < 0 {
            bail!("invalid relay-list clock");
        }
        let people: BTreeSet<_> = people.iter().cloned().collect();
        if people.len() > MAX_PEOPLE {
            bail!("too many addressed users");
        }
        for person in &people {
            validate_public_key(person)?;
        }
        let missing: Vec<_> = {
            let cache = self.cache.lock().await;
            people
                .iter()
                .filter(|p| {
                    cache.get(*p).is_none_or(|v| {
                        now < v.checked
                            || now - v.checked
                                >= if v.list.is_some() {
                                    REFRESH_SECONDS
                                } else {
                                    60
                                }
                    })
                })
                .cloned()
                .collect()
        };
        if !missing.is_empty() {
            let filters: Vec<_> = missing
                .iter()
                .map(|p| Filter {
                    ids: vec![],
                    authors: vec![p.clone()],
                    kinds: vec![10002],
                    events: vec![],
                    coordinates: vec![],
                    people: vec![],
                    since: None,
                    until: Some(now as u64),
                    limit: 8,
                })
                .collect();
            if let Ok(batch) = self.source.query(&filters).await
                && batch.completed_relays > 0
                && !batch.saturated
            {
                let mut cache = self.cache.lock().await;
                for person in missing {
                    let best = batch
                        .events
                        .iter()
                        .filter_map(|e| RelayList::parse(e, &person, now).ok())
                        .max_by(|a, b| {
                            a.event
                                .event()
                                .created_at
                                .cmp(&b.event.event().created_at)
                                .then_with(|| b.event.event().id.cmp(&a.event.event().id))
                        });
                    let previous = cache.get(&person).and_then(|v| v.list.clone());
                    let list = match (previous, best) {
                        (Some(old), Some(new))
                            if old.event.event().created_at > new.event.event().created_at
                                || (old.event.event().created_at
                                    == new.event.event().created_at
                                    && old.event.event().id < new.event.event().id) =>
                        {
                            Some(old)
                        }
                        (_, Some(new)) => Some(new),
                        (old, None) => old,
                    };
                    if cache.len() >= MAX_CACHE
                        && !cache.contains_key(&person)
                        && let Some(key) = cache
                            .iter()
                            .min_by_key(|(_, v)| v.checked)
                            .map(|(k, _)| k.clone())
                    {
                        cache.remove(&key);
                    }
                    cache.insert(person, Cached { checked: now, list });
                }
            }
        }
        let cache = self.cache.lock().await;
        Ok(people
            .into_iter()
            .filter_map(|p| {
                cache
                    .get(&p)
                    .and_then(|v| v.list.clone())
                    .and_then(|l| RelayList::parse(l.event(), &p, now).ok())
                    .map(|l| (p, l))
            })
            .collect())
    }
    pub async fn public_relays(
        &self,
        people: &[String],
        base: &[String],
        now: i64,
    ) -> Result<Vec<String>> {
        let mut result = BTreeSet::new();
        for relay in base {
            result.insert(crate::public_network::validate_destination(relay, "wss")?.to_string());
        }
        for list in self.lists(people, now).await?.into_values() {
            result.extend(list.read);
        }
        if result.is_empty() || result.len() > 48 {
            bail!("public relay plan exceeds bound");
        }
        Ok(result.into_iter().collect())
    }
}

/// NIP-17 inbox lists are independent from NIP-65 public read/write lists.
pub fn private_inbox(event: &VerifiedEvent, recipient: &str, now: i64) -> Result<Vec<String>> {
    validate_public_key(recipient)?;
    if now < 0
        || event.event().kind != 10050
        || event.event().pubkey != recipient
        || event.event().created_at > now as u64
    {
        bail!("invalid private inbox list");
    }
    if event
        .tag_values("expiration")
        .any(|v| v.parse::<i64>().map_or(true, |t| t <= now))
    {
        bail!("private inbox list expired");
    }
    let mut relays = BTreeSet::new();
    for relay in event.tag_values("relay") {
        if let Ok(url) = crate::public_network::validate_destination(relay, "wss")
            && !is_broadcaster(url.as_str())
        {
            relays.insert(url.to_string());
        }
    }
    if relays.is_empty() || relays.len() > 8 {
        bail!("private inbox unavailable or exceeds bound");
    }
    Ok(relays.into_iter().collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn event(kind: u64, at: u64, tags: Vec<Vec<String>>) -> VerifiedEvent {
        VerifiedEvent::verify(crate::nostr_event::test_event(1, kind, at, "", tags)).unwrap()
    }
    #[test]
    fn signed_nip65_separates_read_write_and_rejects_unsafe_routes() {
        let e = event(
            10002,
            100,
            vec![
                vec!["r".into(), "wss://both.example/".into()],
                vec!["r".into(), "wss://inbox.example/".into(), "read".into()],
                vec!["r".into(), "wss://outbox.example/".into(), "write".into()],
                vec!["r".into(), "wss://10.8.0.6/".into()],
                vec!["r".into(), "wss://sendit.nosflare.com/".into()],
            ],
        );
        let l = RelayList::parse(&e, &e.event().pubkey, 100).unwrap();
        assert_eq!(l.read, vec!["wss://both.example/", "wss://inbox.example/"]);
        assert_eq!(
            l.write,
            vec!["wss://both.example/", "wss://outbox.example/"]
        );
        assert!(RelayList::parse(&e, &"22".repeat(32), 100).is_err());
        assert!(RelayList::parse(&e, &e.event().pubkey, 99).is_err());
    }
    #[test]
    fn giftwrap_inboxes_do_not_fall_back_to_public_broadcasters() {
        let e = event(
            10050,
            100,
            vec![
                vec!["relay".into(), "wss://private.example/".into()],
                vec!["relay".into(), "wss://sendit.nosflare.com/".into()],
            ],
        );
        assert_eq!(
            private_inbox(&e, &e.event().pubkey, 100).unwrap(),
            vec!["wss://private.example/"]
        );
        assert!(RelayList::parse(&e, &e.event().pubkey, 100).is_err());
        let public = event(
            10002,
            100,
            vec![vec!["r".into(), "wss://public.example/".into()]],
        );
        assert!(private_inbox(&public, &public.event().pubkey, 100).is_err());
    }
}

/// Preserve the author's existing read policy while advertising configured
/// storage relays for writes. A write-only broadcaster is not advertised as a
/// readable outbox. This creates no signature or network operation.
pub fn advertised_write_tags(
    existing: Option<&RelayList>,
    direct: &[String],
) -> Result<Option<Vec<Vec<String>>>> {
    let mut tags = existing
        .map(|l| l.event().event().tags.clone())
        .unwrap_or_default();
    if tags.len() > 64 {
        bail!("existing relay list exceeds advertisement bound");
    }
    let mut changed = existing.is_none();
    for relay in public_relays(direct)?
        .into_iter()
        .filter(|r| !is_broadcaster(r))
    {
        let mut present = false;
        for tag in &mut tags {
            if tag.first().map(String::as_str) != Some("r") {
                continue;
            }
            if !(2..=3).contains(&tag.len()) {
                bail!("malformed existing relay advertisement");
            }
            let url = crate::public_network::validate_destination(&tag[1], "wss")?;
            if url.as_str() == relay {
                if tag.get(2).is_some_and(|v| v == "read") {
                    tag.truncate(2);
                    changed = true;
                } else if tag.get(2).is_some_and(|v| v != "write") {
                    bail!("unknown relay marker");
                }
                present = true;
            }
        }
        if !present {
            tags.push(vec!["r".into(), relay, "write".into()]);
            changed = true;
        }
    }
    if tags.len() > 64 {
        bail!("relay advertisement exceeds bound");
    }
    Ok(changed.then_some(tags))
}

#[cfg(test)]
mod routing_tests {
    use super::*;
    use crate::cyberherd::relay::ReadBatch;
    #[derive(Clone)]
    struct Source {
        events: Arc<Mutex<Vec<VerifiedEvent>>>,
        fail: Arc<Mutex<bool>>,
    }
    impl Query for Source {
        async fn query(&self, filters: &[Filter]) -> Result<ReadBatch> {
            for filter in filters {
                filter.validate()?;
                assert_eq!(filter.kinds, vec![10002]);
                assert!(filter.ids.is_empty());
            }
            if *self.fail.lock().await {
                bail!("synthetic metadata outage");
            }
            Ok(ReadBatch {
                events: self
                    .events
                    .lock()
                    .await
                    .iter()
                    .filter(|e| filters.iter().any(|f| f.matches(e)))
                    .cloned()
                    .collect(),
                completed_relays: 1,
                failed_relays: 0,
                saturated: false,
            })
        }
    }
    fn list(at: u64, tags: Vec<Vec<String>>) -> VerifiedEvent {
        VerifiedEvent::verify(crate::nostr_event::test_event(2, 10002, at, "", tags)).unwrap()
    }
    #[tokio::test]
    async fn newer_empty_lists_clear_inboxes_and_cached_heads_never_roll_back() {
        let old = list(
            100,
            vec![vec!["r".into(), "wss://old.example/".into(), "read".into()]],
        );
        let empty = list(
            200,
            vec![vec![
                "r".into(),
                "wss://write-only.example/".into(),
                "write".into(),
            ]],
        );
        let person = old.event().pubkey.clone();
        let source = Source {
            events: Arc::new(Mutex::new(vec![old.clone()])),
            fail: Arc::new(Mutex::new(false)),
        };
        let router = Router::with_source(source.clone()).unwrap();
        let base = vec![
            "wss://primary.example/".into(),
            "wss://sendit.nosflare.com/".into(),
        ];
        assert!(
            router
                .public_relays(&[person.clone()], &base, 100)
                .await
                .unwrap()
                .contains(&"wss://old.example/".into())
        );
        *source.events.lock().await = vec![old.clone(), empty];
        let routes = router
            .public_relays(&[person.clone()], &base, 1100)
            .await
            .unwrap();
        assert_eq!(routes, base);
        *source.events.lock().await = vec![old];
        assert_eq!(
            router
                .public_relays(&[person.clone()], &base, 2100)
                .await
                .unwrap(),
            base
        );
        *source.fail.lock().await = true;
        assert_eq!(
            router.public_relays(&[person], &base, 3100).await.unwrap(),
            base
        );
    }
    #[tokio::test]
    async fn same_timestamp_uses_lowest_id_and_ignores_future_metadata() {
        let a = list(100, vec![vec!["r".into(), "wss://a.example/".into()]]);
        let b = list(100, vec![vec!["r".into(), "wss://b.example/".into()]]);
        let future = list(1000, vec![vec!["r".into(), "wss://future.example/".into()]]);
        let expected = if a.event().id < b.event().id {
            a.clone()
        } else {
            b.clone()
        };
        let person = a.event().pubkey.clone();
        let source = Source {
            events: Arc::new(Mutex::new(vec![a, b, future])),
            fail: Arc::new(Mutex::new(false)),
        };
        let router = Router::with_source(source).unwrap();
        let lists = router.lists(&[person.clone()], 200).await.unwrap();
        assert_eq!(lists[&person].event().event().id, expected.event().id);
    }
    #[test]
    fn advertisements_preserve_reads_and_exclude_broadcasters() {
        let original = list(
            100,
            vec![vec![
                "r".into(),
                "wss://inbox.example/".into(),
                "read".into(),
            ]],
        );
        let parsed = RelayList::parse(&original, &original.event().pubkey, 100).unwrap();
        let tags = advertised_write_tags(
            Some(&parsed),
            &[
                "wss://storage.example/".into(),
                "wss://sendit.nosflare.com/".into(),
            ],
        )
        .unwrap()
        .unwrap();
        assert_eq!(
            tags,
            vec![
                vec!["r", "wss://inbox.example/", "read"],
                vec!["r", "wss://storage.example/", "write"]
            ]
        );
    }
    #[test]
    fn nip19_mentions_resolve_identity_without_trusting_their_relay_hints() {
        let person = crate::nostr_event::test_event(2, 0, 100, "", vec![]).pubkey;
        let mut data = vec![0, 32];
        data.extend(hex::decode(&person).unwrap());
        let hint = b"wss://10.8.0.6/";
        data.extend([1, hint.len() as u8]);
        data.extend(hint);
        let reference = bitcoin::bech32::encode::<bitcoin::bech32::Bech32>(
            bitcoin::bech32::Hrp::parse("nprofile").unwrap(),
            &data,
        )
        .unwrap();
        assert_eq!(
            people(&format!("Thanks nostr:{reference}!"), &[]),
            vec![person]
        );
    }
}
