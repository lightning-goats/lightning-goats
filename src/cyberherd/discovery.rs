//! Refresh authenticated heads before recovery. The configured relay completion
//! quorum is explicit; saturation or missing quorum makes discovery unavailable,
//! never evidence that cached targets became stale. No daily note is fabricated.
use std::collections::BTreeSet;

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
            let key = super::ingress::hash(&("engagement-v2", cycle_start, coordinate, &id))?;
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
        crate::nostr_event::validate_public_key(pubkey)?;
        let mut query = filter(vec![0]);
        query.authors.push(pubkey.into());
        query.until = Some(now.try_into()?);
        let (events, complete) = self.read(&[query]).await;
        for event in events {
            if let Ok(profile) = AuthenticatedProfile::parse(event, pubkey, now) {
                store.remember_profile(owner, &profile).await?;
            }
        }
        store
            .record_reward_profile_read(owner, pubkey, complete, now)
            .await?;
        if !complete {
            return Ok(None);
        }
        store.profile(owner, pubkey, now).await
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
        let mut filters = Vec::new();
        for key in keys {
            crate::nostr_event::validate_public_key(key)?;
            let mut query = filter(vec![0]);
            query.authors.push(key.clone());
            query.until = Some(now as u64);
            filters.push(query);
        }
        let (events, complete) = self.read(&filters).await;
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
                .record_reward_profile_read(owner, key, complete, now)
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
