//! Refresh authenticated heads before recovery. The configured relay completion
//! quorum is explicit; saturation or missing quorum makes discovery unavailable,
//! never evidence that cached targets became stale. No daily note is fabricated.
use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Result, bail};

use super::{
    calendar::DayWindow,
    relay::{Filter, Query, engagement_filters, engagement_scope_filters},
    store::Store,
    targets::{
        AuthenticatedProfile, ProviderCache, StreamAuthentication, TargetPolicy, Targets,
        goal_reference,
    },
};
use crate::nostr_event::VerifiedEvent;

pub struct Discovery<R> {
    source: R,
    providers: ProviderCache,
    required_relays: usize,
}

pub struct Refresh {
    pub targets: Targets,
    pub available: bool,
}

fn newest(a: &VerifiedEvent, b: &VerifiedEvent) -> std::cmp::Ordering {
    a.event()
        .created_at
        .cmp(&b.event().created_at)
        .then_with(|| b.event().id.cmp(&a.event().id))
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NutzapDestination {
    pub advertisement: String,
    pub member: String,
    pub mint: String,
    pub receiving_key: String,
    pub token_relays: Vec<String>,
    pub read_relays: Vec<String>,
}

impl NutzapDestination {
    fn from_event(
        event: &VerifiedEvent,
        member: &str,
        project_mint: &str,
        read_relays: Vec<String>,
        now: i64,
    ) -> Result<Self> {
        event.require_not_future(now)?;
        let signed = event.event();
        anyhow::ensure!(
            signed.kind == 10019 && signed.pubkey == member,
            "wrong wallet advertisement"
        );
        let key = event
            .singleton_tag("pubkey", true)?
            .ok_or_else(|| anyhow::anyhow!("missing wallet lock key"))?;
        let x_only = if key.len() == 66 {
            key.strip_prefix("02")
                .ok_or_else(|| anyhow::anyhow!("incompatible NIP-61 key"))?
        } else {
            key
        };
        crate::nostr_event::validate_public_key(x_only)?;
        let mint = signed
            .tags
            .iter()
            .find_map(|tag| match tag.as_slice() {
                [name, url, units @ ..]
                    if name == "mint"
                        && url.trim_end_matches('/') == project_mint.trim_end_matches('/')
                        && (units.is_empty() || units.iter().any(|u| u == "sat")) =>
                {
                    Some(url.clone())
                }
                _ => None,
            })
            .ok_or_else(|| anyhow::anyhow!("Goats mint sats are not advertised"))?;
        let mut token_relays = BTreeSet::new();
        for raw in event.tag_values("relay") {
            token_relays
                .insert(crate::public_network::validate_destination(raw, "wss")?.to_string());
        }
        anyhow::ensure!(
            (1..=8).contains(&token_relays.len()),
            "invalid token relay count"
        );
        Ok(Self {
            advertisement: signed.id.clone(),
            member: member.into(),
            mint,
            receiving_key: format!("02{x_only}"),
            token_relays: token_relays.into_iter().collect(),
            read_relays,
        })
    }
}

impl<R: Query> Discovery<R> {
    pub fn new(source: R, required_relays: usize) -> Result<Self> {
        if !(1..=8).contains(&required_relays) {
            bail!("invalid discovery relay quorum");
        }
        Ok(Self {
            source,
            providers: ProviderCache::default(),
            required_relays,
        })
    }

    pub fn source(&self) -> &R {
        &self.source
    }

    /// Recent intake cannot wait behind historical recovery. Each event/stream
    /// coordinate retains its own cursor, so a new stream version or alias never
    /// resets an already scanned announcement. Old cursors/receipts are retained.
    pub async fn backfill(
        &self,
        store: &Store,
        owner: &str,
        targets: &Targets,
        cycle_start: i64,
        now: i64,
    ) -> Result<bool> {
        if now < cycle_start || cycle_start < 0 || targets.owner() != owner {
            bail!("invalid social backfill scope");
        }
        let current = targets.current();
        let mut aliases = Vec::new();
        for target in &current {
            if let Some(root) = &target.coordinate {
                aliases.extend(store.stream_alias_ids(owner, root).await?);
            }
        }
        let recent_since = now.saturating_sub(300).max(cycle_start);
        let recent = engagement_filters(&current, &aliases, recent_since as u64, now as u64)?;
        if recent.is_empty() {
            return Ok(false);
        }
        let mut scopes = BTreeSet::new();
        for target in &current {
            scopes.insert((false, target.event_id.clone()));
            if let Some(root) = &target.coordinate {
                scopes.insert((true, root.clone()));
            }
        }
        scopes.extend(aliases.into_iter().map(|id| (false, id)));
        let mut cursors = Vec::new();
        for (coordinate, id) in scopes {
            // Revisit the current cycle once to admit previously skipped emoji likes.
            let key = super::ingress::hash(&("engagement-v3", cycle_start, coordinate, &id))?;
            let cursor = store.relay_window(owner, &key, cycle_start).await?;
            cursors.push((coordinate, id, key, cursor));
        }
        let mut advanced = false;
        match self.scan_social(store, owner, &recent, now).await? {
            None => return Ok(false),
            Some(false) => {
                // Only a cursor already covered by this complete recent window
                // may advance. A recent scan never skips an older recovery gap.
                for (_, _, key, cursor) in &mut cursors {
                    if *cursor >= recent_since && *cursor < now {
                        store.advance_relay_window(owner, key, now).await?;
                        *cursor = now;
                        advanced = true;
                    }
                }
            }
            Some(true) => {}
        }
        let Some(cursor) = cursors.iter().map(|row| row.3).filter(|v| *v < now).min() else {
            return Ok(advanced);
        };
        let selected: Vec<_> = cursors.iter().filter(|row| row.3 == cursor).collect();
        let since = cursor.saturating_sub(30).max(cycle_start);
        let mut until = now.min(cursor.saturating_add(86400));
        // An empty/quiet day catches up in one bounded query; a saturated day
        // bisects down to a second. Missing quorum/full inbox never advances it.
        for _ in 0..18 {
            let ids = selected
                .iter()
                .filter(|row| !row.0)
                .map(|row| row.1.clone())
                .collect();
            let coordinates = selected
                .iter()
                .filter(|row| row.0)
                .map(|row| row.1.clone())
                .collect();
            let filters = engagement_scope_filters(ids, coordinates, since as u64, until as u64)?;
            match self.scan_social(store, owner, &filters, now).await? {
                None => return Ok(advanced),
                Some(false) => {
                    for row in selected {
                        store.advance_relay_window(owner, &row.2, until).await?;
                    }
                    return Ok(true);
                }
                Some(true) => {}
            }
            if until <= since + 1 {
                return Ok(advanced);
            }
            until = since + (until - since) / 2;
            if until <= cursor {
                return Ok(advanced);
            }
        }
        Ok(advanced)
    }

    async fn scan_social(
        &self,
        store: &Store,
        owner: &str,
        filters: &[Filter],
        now: i64,
    ) -> Result<Option<bool>> {
        let batch = match self.source.query(filters).await {
            Ok(batch) => batch,
            Err(_) => return Ok(None),
        };
        // Native verification and durable staging precede profile resolution.
        // Missing quorum still retains validated events, without skipping gaps.
        for event in batch.events {
            if event.require_not_future(now).is_ok()
                && super::social::SocialEvent::parse(event.clone(), owner, now)
                    .is_ok_and(|v| v.is_some())
            {
                super::social::stage(store, owner, event, now).await?;
            }
        }
        Ok((batch.completed_relays >= self.required_relays).then_some(batch.saturated))
    }

    pub async fn refresh(
        &mut self,
        store: &Store,
        policy: &TargetPolicy,
        day: &DayWindow,
        requested_ids: &[String],
        now: i64,
    ) -> Result<Refresh> {
        policy.validate()?;
        day.validate()?;
        if !day.contains(now) || requested_ids.len() > 32 {
            bail!("invalid discovery window/reference bound");
        }
        let owner = &policy.project_pubkey;
        let mut project = filter(vec![1]);
        project.authors.push(owner.clone());
        project.since = Some(day.start as u64);
        project.until = Some(now as u64);
        let mut stream = filter(vec![30311]);
        stream.authors.push(owner.clone());
        stream.since = Some(now.saturating_sub(3600) as u64);
        stream.until = Some(now as u64);
        let mut hosted = stream.clone();
        hosted.authors.clear();
        hosted.people.push(owner.clone());
        let (events, mut available) = self.read(&[project, stream, hosted]).await;
        let mut references: BTreeSet<String> = requested_ids.iter().cloned().collect();
        let mut uncertain = BTreeSet::new();
        for event in events {
            if let Ok(Some(id)) = goal_reference(&event) {
                references.insert(id);
            }
            self.remember(store, policy, event, now, &mut uncertain)
                .await?;
        }
        if references.len() > 128 {
            bail!("discovery references exceed bounded batch");
        }
        if !references.is_empty() {
            let mut ids = filter(vec![1, 9041, 30311]);
            ids.ids = references.into_iter().collect();
            let (events, complete) = self.read(&[ids]).await;
            available &= complete;
            for event in events {
                self.remember(store, policy, event, now, &mut uncertain)
                    .await?;
            }
        }
        if !uncertain.is_empty() {
            available = false;
        }
        let targets = store
            .target_projection(policy, day, requested_ids, uncertain, now)
            .await?;
        Ok(Refresh { targets, available })
    }

    async fn remember(
        &mut self,
        store: &Store,
        policy: &TargetPolicy,
        event: VerifiedEvent,
        now: i64,
        uncertain: &mut BTreeSet<String>,
    ) -> Result<()> {
        if event.require_not_future(now).is_err() {
            return Ok(());
        }
        match event.event().kind {
            1 | 9041 => {
                if event.event().pubkey == policy.project_pubkey {
                    store.remember_project_event(policy, &event, now).await?;
                } else {
                    store
                        .remember_reference(&policy.project_pubkey, &event, now)
                        .await?;
                }
            }
            30311 => {
                store
                    .remember_reference(&policy.project_pubkey, &event, now)
                    .await?;
                // Malformed/forged host declarations are not authority and do
                // not overwrite a legitimate cached head.
                match self
                    .providers
                    .authenticate(event.clone(), &policy.project_pubkey, now)
                    .await
                {
                    Ok(StreamAuthentication::Accepted(stream)) => {
                        store
                            .remember_stream(&policy.project_pubkey, &stream)
                            .await?;
                    }
                    Ok(StreamAuthentication::Unavailable) => {
                        uncertain.insert(event.event().id.clone());
                    }
                    Ok(StreamAuthentication::Rejected) | Err(_) => {}
                }
            }
            _ => {}
        }
        Ok(())
    }

    /// A successful signed negative profile is remembered. A failed/missing
    /// query is not an address-removal update or a reason to forget a profile.
    pub async fn refresh_profile(
        &self,
        store: &Store,
        owner: &str,
        pubkey: &str,
        now: i64,
    ) -> Result<Option<AuthenticatedProfile>> {
        let keys = vec![pubkey.to_owned()];
        let (events, availability) = self.read_profiles(&keys, now).await?;
        for event in events {
            if let Ok(profile) = AuthenticatedProfile::parse(event, pubkey, now) {
                store.remember_profile(owner, &profile).await?;
            }
        }
        let complete = availability[pubkey];
        store
            .record_reward_profile_read(owner, pubkey, complete, now)
            .await?;
        if !complete {
            return Ok(None);
        }
        store.profile(owner, pubkey, now).await
    }

    /// Resolve an explicitly requested public reward destination. Outbox reads
    /// include the latest advertisement even when the configured relays have
    /// an older copy. A newer incompatible advertisement supersedes the old one.
    pub async fn nutzap_destination(
        &self,
        member: &str,
        project_mint: &str,
        now: i64,
    ) -> Result<NutzapDestination> {
        crate::nostr_event::validate_public_key(member)?;
        let mut query = filter(vec![10002, 10019]);
        query.authors.push(member.to_owned());
        query.until = Some(u64::try_from(now)?);
        let (mut events, complete) = self.read(std::slice::from_ref(&query)).await;
        anyhow::ensure!(complete, "member destination discovery incomplete");
        let list = events
            .iter()
            .filter_map(|e| crate::relay_routing::RelayList::parse(e, member, now).ok())
            .max_by(|a, b| newest(a.event(), b.event()));
        if let Some(list) = &list
            && !list.write.is_empty()
        {
            query.kinds = vec![10019];
            let batch = self.source.query_hints(&list.write, &[query]).await?;
            anyhow::ensure!(
                !batch.saturated && !batch.completed_urls.is_empty(),
                "member outbox discovery incomplete"
            );
            events.extend(batch.events);
        }
        let advertisement = events
            .iter()
            .filter(|e| {
                e.event().kind == 10019
                    && e.event().pubkey == member
                    && e.require_not_future(now).is_ok()
            })
            .max_by(|a, b| newest(a, b))
            .ok_or_else(|| anyhow::anyhow!("member has no signed wallet advertisement"))?;
        NutzapDestination::from_event(
            advertisement,
            member,
            project_mint,
            list.map(|l| l.read).unwrap_or_default(),
            now,
        )
    }

    async fn read(&self, filters: &[Filter]) -> (Vec<VerifiedEvent>, bool) {
        match self.source.query(filters).await {
            Ok(batch) => (
                batch.events,
                batch.completed_relays >= self.required_relays && !batch.saturated,
            ),
            Err(_) => (vec![], false),
        }
    }

    /// A complete configured-relay query may contain a signed relay list but
    /// no profile. Follow the author's public NIP-65 write relays, rather than
    /// retrying that same empty query forever. Hints never authenticate profiles
    /// or replace the configured completion quorum.
    async fn read_profiles(
        &self,
        keys: &[String],
        now: i64,
    ) -> Result<(Vec<VerifiedEvent>, BTreeMap<String, bool>)> {
        if keys.is_empty() || keys.len() > 8 || now < 0 {
            bail!("profile query batch exceeds bound");
        }
        let mut filters = Vec::new();
        for key in keys {
            crate::nostr_event::validate_public_key(key)?;
            let mut query = filter(vec![0, 10002]);
            query.authors.push(key.clone());
            query.until = Some(now as u64);
            filters.push(query);
        }
        let (mut events, complete) = self.read(&filters).await;
        let mut availability: BTreeMap<_, _> =
            keys.iter().map(|key| (key.clone(), complete)).collect();
        let mut missing = BTreeMap::new();
        for key in keys {
            if events
                .iter()
                .any(|event| AuthenticatedProfile::parse(event.clone(), key, now).is_ok())
            {
                continue;
            }
            let list = events
                .iter()
                .filter_map(|event| crate::relay_routing::RelayList::parse(event, key, now).ok())
                .max_by(|a, b| {
                    a.event()
                        .event()
                        .created_at
                        .cmp(&b.event().event().created_at)
                        .then_with(|| b.event().event().id.cmp(&a.event().event().id))
                });
            if let Some(list) = list.filter(|list| !list.write.is_empty()) {
                missing.insert(key.clone(), list.write);
            }
        }
        if !missing.is_empty() {
            // Round robin gives every actor in a reward batch an outbox before
            // filling the shared eight-relay bound with additional destinations.
            let mut relays = Vec::new();
            for index in 0..8 {
                for outboxes in missing.values() {
                    if let Some(relay) = outboxes.get(index)
                        && relays.len() < 8
                        && !relays.contains(relay)
                    {
                        relays.push(relay.clone());
                    }
                }
            }
            let filters: Vec<_> = missing
                .keys()
                .map(|key| {
                    let mut query = filter(vec![0]);
                    query.authors.push(key.clone());
                    query.until = Some(now as u64);
                    query
                })
                .collect();
            let completed = match self.source.query_hints(&relays, &filters).await {
                Ok(batch) => {
                    events.extend(batch.events);
                    if batch.saturated {
                        BTreeSet::new()
                    } else {
                        batch.completed_urls
                    }
                }
                Err(_) => BTreeSet::new(),
            };
            for (key, outboxes) in &missing {
                availability.insert(
                    key.clone(),
                    complete && outboxes.iter().any(|relay| completed.contains(relay)),
                );
            }
        }
        Ok((events, availability))
    }

    /// One bounded query, rather than serially opening relays for each member.
    /// A saturated/incomplete batch is not proof of a missing payout address.
    pub async fn refresh_reward_profiles(
        &self,
        store: &Store,
        owner: &str,
        keys: &[String],
        now: i64,
    ) -> Result<()> {
        if keys.is_empty() || keys.len() > 8 || now < 0 {
            bail!("reward profile batch exceeds bound");
        }
        let (events, availability) = self.read_profiles(keys, now).await?;
        for event in events {
            if keys.contains(&event.event().pubkey) {
                if let Ok(profile) =
                    AuthenticatedProfile::parse(event.clone(), &event.event().pubkey, now)
                {
                    store.remember_profile(owner, &profile).await?;
                }
            }
        }
        for key in keys {
            store
                .record_reward_profile_read(owner, key, availability[key], now)
                .await?;
        }
        Ok(())
    }
}

fn filter(kinds: Vec<u16>) -> Filter {
    Filter {
        ids: vec![],
        authors: vec![],
        kinds,
        events: vec![],
        coordinates: vec![],
        people: vec![],
        since: None,
        until: None,
        limit: 512,
    }
}
