//! Durable authenticated heads/aliases and replay-safe raw social inbox. Only
//! the CyberHerd database is opened; no core financial state is writable here.
use std::collections::BTreeSet;

use anyhow::{Context, Result, bail};
use sqlx::{Row, Sqlite, Transaction};

use super::{Interaction, Snapshot, Store};
use crate::{
    cyberherd::{
        calendar::DayWindow,
        targets::{
            AuthenticatedProfile, AuthenticatedStream, StreamAuthority, TargetPolicy, Targets,
            supersedes,
        },
    },
    nostr_event::{MAX_EVENT_BYTES, VerifiedEvent, validate_public_key},
};

const QUERY_BYTE_BUDGET: usize = 8 * 1024 * 1024;

async fn insert_event(
    tx: &mut Transaction<'_, Sqlite>,
    owner: &str,
    event: &VerifiedEvent,
) -> Result<()> {
    let signed = event.event();
    let json = serde_json::to_string(signed)?;
    if json.len() > MAX_EVENT_BYTES {
        bail!("stored Nostr event exceeds its wire bound");
    }
    sqlx::query("INSERT INTO observed_events(owner,event_id,kind,author,created_at,event_json) VALUES(?,?,?,?,?,?) ON CONFLICT(owner,event_id) DO NOTHING")
        .bind(owner).bind(&signed.id).bind(i64::try_from(signed.kind)?).bind(&signed.pubkey).bind(i64::try_from(signed.created_at)?).bind(json).execute(&mut **tx).await?;
    Ok(())
}

impl Store {
    /// Retain a referenced signed object without granting project/host authority.
    /// In particular a raw 30311 can never enter stream_heads or aliases here.
    pub async fn remember_reference(
        &self,
        owner: &str,
        event: &VerifiedEvent,
        now: i64,
    ) -> Result<()> {
        validate_public_key(owner)?;
        event.require_not_future(now)?;
        if !matches!(event.event().kind, 1 | 9041 | 30311) {
            bail!("unsupported referenced target");
        }
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        insert_event(&mut tx, owner, event).await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn stream_alias_ids(&self, owner: &str, coordinate: &str) -> Result<Vec<String>> {
        validate_public_key(owner)?;
        let _: crate::nostr_event::Coordinate = coordinate.parse()?;
        // A bounded overlap for relay queries. Direct references from settled
        // zaps/inbox work still resolve any individually requested old version.
        Ok(sqlx::query_scalar("SELECT v.event_id FROM stream_versions v JOIN observed_events e ON v.owner=e.owner AND v.event_id=e.event_id WHERE v.owner=? AND v.coordinate=? ORDER BY e.created_at DESC,e.event_id LIMIT 125")
            .bind(owner).bind(coordinate).fetch_all(&self.pool).await?)
    }

    pub async fn remember_project_event(
        &self,
        policy: &TargetPolicy,
        event: &VerifiedEvent,
        now: i64,
    ) -> Result<()> {
        policy.validate()?;
        let owner = &policy.project_pubkey;
        event.require_not_future(now)?;
        if event.event().pubkey != *owner || !matches!(event.event().kind, 1 | 9041) {
            bail!("project target must be signed by its configured owner");
        }
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        insert_event(&mut tx, owner, event).await?;
        if policy.is_tagged_project_note(event) {
            let goal = super::super::targets::goal_reference(event).ok().flatten();
            sqlx::query("INSERT INTO announcement_candidates(owner,policy_key,event_id,goal_id) VALUES(?,?,?,?) ON CONFLICT(owner,policy_key,event_id) DO NOTHING")
                .bind(owner).bind(policy.policy_key()?).bind(&event.event().id).bind(goal).execute(&mut *tx).await?;
        }
        tx.commit().await?;
        Ok(())
    }

    pub async fn remember_stream(&self, owner: &str, stream: &AuthenticatedStream) -> Result<bool> {
        validate_public_key(owner)?;
        if stream.owner() != owner {
            bail!("stream proof belongs to another owner");
        }
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        insert_event(&mut tx, owner, stream.event()).await?;
        sqlx::query("INSERT INTO stream_versions(owner,event_id,coordinate,authority_json) VALUES(?,?,?,?) ON CONFLICT(owner,event_id) DO UPDATE SET authority_json=excluded.authority_json")
            .bind(owner).bind(&stream.event().event().id).bind(stream.coordinate()).bind(serde_json::to_string(stream.authority())?).execute(&mut *tx).await?;
        let previous:Option<String>=sqlx::query_scalar("SELECT e.event_json FROM stream_heads h JOIN observed_events e ON h.owner=e.owner AND h.event_id=e.event_id WHERE h.owner=? AND h.coordinate=?")
            .bind(owner).bind(stream.coordinate()).fetch_optional(&mut *tx).await?;
        let replace = match previous {
            Some(json) => supersedes(
                stream.event().event(),
                VerifiedEvent::from_json(&json)?.event(),
            ),
            None => true,
        };
        if replace {
            sqlx::query("INSERT INTO stream_heads(owner,coordinate,event_id) VALUES(?,?,?) ON CONFLICT(owner,coordinate) DO UPDATE SET event_id=excluded.event_id")
                .bind(owner).bind(stream.coordinate()).bind(&stream.event().event().id).execute(&mut *tx).await?;
        }
        tx.commit().await?;
        Ok(replace)
    }

    pub async fn remember_profile(
        &self,
        owner: &str,
        profile: &AuthenticatedProfile,
    ) -> Result<bool> {
        validate_public_key(owner)?;
        let event = profile.event();
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        insert_event(&mut tx, owner, event).await?;
        let previous:Option<String>=sqlx::query_scalar("SELECT e.event_json FROM profile_heads h JOIN observed_events e ON h.owner=e.owner AND h.event_id=e.event_id WHERE h.owner=? AND h.pubkey=?")
            .bind(owner).bind(&event.event().pubkey).fetch_optional(&mut *tx).await?;
        let replace = match previous {
            Some(json) => supersedes(event.event(), VerifiedEvent::from_json(&json)?.event()),
            None => true,
        };
        if replace {
            sqlx::query("INSERT INTO profile_heads(owner,pubkey,event_id) VALUES(?,?,?) ON CONFLICT(owner,pubkey) DO UPDATE SET event_id=excluded.event_id")
                .bind(owner).bind(&event.event().pubkey).bind(&event.event().id).execute(&mut *tx).await?;
        }
        tx.commit().await?;
        Ok(replace)
    }

    pub async fn profile(
        &self,
        owner: &str,
        pubkey: &str,
        now: i64,
    ) -> Result<Option<AuthenticatedProfile>> {
        validate_public_key(owner)?;
        validate_public_key(pubkey)?;
        let json:Option<String>=sqlx::query_scalar("SELECT e.event_json FROM profile_heads h JOIN observed_events e ON h.owner=e.owner AND h.event_id=e.event_id WHERE h.owner=? AND h.pubkey=?")
            .bind(owner).bind(pubkey).fetch_optional(&self.pool).await?;
        json.map(|json| AuthenticatedProfile::parse(VerifiedEvent::from_json(&json)?, pubkey, now))
            .transpose()
    }

    pub async fn observed_event(&self, owner: &str, id: &str) -> Result<Option<VerifiedEvent>> {
        validate_public_key(owner)?;
        crate::nostr_event::validate_hex(id, 32)?;
        let json: Option<String> = sqlx::query_scalar(
            "SELECT event_json FROM observed_events WHERE owner=? AND event_id=?",
        )
        .bind(owner)
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        json.map(|json| VerifiedEvent::from_json(&json)).transpose()
    }

    pub async fn stream_alias(
        &self,
        owner: &str,
        id: &str,
        now: i64,
    ) -> Result<Option<AuthenticatedStream>> {
        validate_public_key(owner)?;
        crate::nostr_event::validate_hex(id, 32)?;
        let row=sqlx::query("SELECT e.event_json,v.authority_json FROM stream_versions v JOIN observed_events e ON v.owner=e.owner AND v.event_id=e.event_id WHERE v.owner=? AND v.event_id=?")
            .bind(owner).bind(id).fetch_optional(&self.pool).await?;
        row.map(|row| decode_stream(&row, owner, now)).transpose()
    }

    pub async fn target_projection(
        &self,
        policy: &TargetPolicy,
        day: &DayWindow,
        requested_ids: &[String],
        uncertain: BTreeSet<String>,
        now: i64,
    ) -> Result<Targets> {
        policy.validate()?;
        day.validate()?;
        if requested_ids.len() > 32 {
            bail!("too many requested target references");
        }
        let owner = &policy.project_pubkey;
        let mut tx = self.pool.begin().await?;
        let mut bytes = 0usize;
        let mut events = Vec::new();
        let heads=sqlx::query("SELECT e.event_json,v.authority_json FROM stream_heads h JOIN stream_versions v ON h.owner=v.owner AND h.event_id=v.event_id JOIN observed_events e ON v.owner=e.owner AND v.event_id=e.event_id WHERE h.owner=? AND e.created_at>=? ORDER BY e.created_at DESC LIMIT 129")
            .bind(owner).bind(now.saturating_sub(3600)).fetch_all(&mut *tx).await?;
        if heads.len() > 128 {
            bail!("stream-head projection exceeds its bound");
        }
        let mut streams = Vec::new();
        for row in heads {
            let json: String = row.try_get("event_json")?;
            bytes = add_bytes(bytes, json.len())?;
            streams.push(decode_stream(&row, owner, now)?);
        }
        // Select the newest tagged note and, separately, the newest note linked
        // to the active stream goal. Unrelated project traffic cannot crowd out
        // the daily announcement or force an unbounded scan into memory.
        let policy_key = policy.policy_key()?;
        if let Some(json)=sqlx::query_scalar::<_,String>("SELECT e.event_json FROM announcement_candidates c JOIN observed_events e ON c.owner=e.owner AND c.event_id=e.event_id WHERE c.owner=? AND c.policy_key=? AND e.created_at>=? AND e.created_at<? ORDER BY e.created_at DESC,e.event_id DESC LIMIT 1")
            .bind(owner).bind(&policy_key).bind(day.start).bind(day.end).fetch_optional(&mut *tx).await? {
            bytes=add_bytes(bytes,json.len())?;events.push(VerifiedEvent::from_json(&json)?);
        }
        let active = super::super::targets::select_active_stream(&streams, owner, now)?;
        if let Some(reference) = active.as_ref().and_then(|stream| {
            super::super::targets::goal_reference(stream.event())
                .ok()
                .flatten()
        }) {
            if let Some(json)=sqlx::query_scalar::<_,String>("SELECT e.event_json FROM announcement_candidates c JOIN observed_events e ON c.owner=e.owner AND c.event_id=e.event_id WHERE c.owner=? AND c.policy_key=? AND c.goal_id=? AND e.created_at>=? AND e.created_at<? ORDER BY e.created_at DESC,e.event_id DESC LIMIT 1")
                .bind(owner).bind(&policy_key).bind(reference).bind(day.start).bind(day.end).fetch_optional(&mut *tx).await? {
                bytes=add_bytes(bytes,json.len())?;events.push(VerifiedEvent::from_json(&json)?);
            }
        }
        for id in requested_ids {
            crate::nostr_event::validate_hex(id, 32)?;
            if let Some(row)=sqlx::query("SELECT e.event_json,v.authority_json FROM observed_events e LEFT JOIN stream_versions v ON e.owner=v.owner AND e.event_id=v.event_id WHERE e.owner=? AND e.event_id=?")
                .bind(owner).bind(id).fetch_optional(&mut *tx).await? {
                let json:String=row.try_get("event_json")?;bytes=add_bytes(bytes,json.len())?;
                let event=VerifiedEvent::from_json(&json)?;
                if row.try_get::<Option<String>,_>("authority_json")?.is_some() { streams.push(decode_stream(&row,owner,now)?); }
                else { events.push(event); }
            }
        }
        let mut references = BTreeSet::new();
        for event in &events {
            if let Some(id) = super::super::targets::goal_reference(event).ok().flatten() {
                references.insert(id);
            }
        }
        for stream in &streams {
            if let Some(id) = super::super::targets::goal_reference(stream.event())
                .ok()
                .flatten()
            {
                references.insert(id);
            }
        }
        if references.len() > 256 {
            bail!("target goal-reference bound exceeded");
        }
        for id in references {
            if let Some(json) = sqlx::query_scalar::<_, String>(
                "SELECT event_json FROM observed_events WHERE owner=? AND event_id=? AND kind=9041",
            )
            .bind(owner)
            .bind(id)
            .fetch_optional(&mut *tx)
            .await?
            {
                bytes = add_bytes(bytes, json.len())?;
                events.push(VerifiedEvent::from_json(&json)?);
            }
        }
        tx.commit().await?;
        Targets::project(policy.clone(), day, events, streams, uncertain, now)
    }

    /// Stage raw signed social input before profile/network lookup. A crash or
    /// an unavailable profile therefore cannot claim the interaction as applied.
    pub async fn enqueue_social(
        &self,
        owner: &str,
        event: &VerifiedEvent,
        now: i64,
    ) -> Result<bool> {
        validate_public_key(owner)?;
        event.require_not_future(now)?;
        if super::super::social::SocialEvent::parse(event.clone(), owner, now)?.is_none() {
            bail!("unsupported or self-authored CyberHerd social event");
        }
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let already_seen: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM social_inbox WHERE owner=? AND event_id=?)",
        )
        .bind(owner)
        .bind(&event.event().id)
        .fetch_one(&mut *tx)
        .await?;
        if already_seen {
            tx.commit().await?;
            return Ok(false);
        }
        let pending: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM social_inbox WHERE owner=? AND state='pending'",
        )
        .bind(owner)
        .fetch_one(&mut *tx)
        .await?;
        if pending >= 4096 {
            return Err(crate::cyberherd::ingress::IngressFull.into());
        }
        insert_event(&mut tx, owner, event).await?;
        let inserted=sqlx::query("INSERT INTO social_inbox(owner,event_id) VALUES(?,?) ON CONFLICT(owner,event_id) DO NOTHING").bind(owner).bind(&event.event().id).execute(&mut *tx).await?.rows_affected()==1;
        // The same transaction establishes order before any profile/target
        // lookup. A later, backdated social event cannot precede a feeding.
        super::ingress_state::enqueue(
            &mut tx,
            owner,
            &format!("nostr:{}", event.event().id),
            "social",
            &serde_json::to_string(&event.event().id)?,
        )
        .await?;
        tx.commit().await?;
        Ok(inserted)
    }

    pub async fn social_batch(
        &self,
        owner: &str,
        now: i64,
        limit: u16,
    ) -> Result<Vec<VerifiedEvent>> {
        validate_public_key(owner)?;
        if now < 0 || !(1..=128).contains(&limit) {
            bail!("invalid social inbox batch");
        }
        let rows=sqlx::query_scalar::<_,String>("SELECT e.event_json FROM social_inbox s JOIN observed_events e ON s.owner=e.owner AND s.event_id=e.event_id WHERE s.owner=? AND s.state='pending' AND s.next_attempt<=? ORDER BY e.created_at,e.event_id LIMIT ?")
            .bind(owner).bind(now).bind(i64::from(limit)).fetch_all(&self.pool).await?;
        let mut bytes = 0usize;
        let mut result = Vec::new();
        for json in rows {
            bytes = add_bytes(bytes, json.len())?;
            result.push(VerifiedEvent::from_json(&json)?);
        }
        Ok(result)
    }

    pub async fn finish_social(&self, owner: &str, id: &str, done: bool, now: i64) -> Result<()> {
        validate_public_key(owner)?;
        crate::nostr_event::validate_hex(id, 32)?;
        if now < 0 {
            bail!("invalid social retry timestamp");
        }
        sqlx::query("UPDATE social_inbox SET state=CASE WHEN ? THEN 'done' ELSE state END,attempts=attempts+1,next_attempt=?+min(300,5*(1<<min(attempts,6))) WHERE owner=? AND event_id=?")
            .bind(done).bind(now).bind(owner).bind(id).execute(&self.pool).await?;
        Ok(())
    }

    pub async fn frozen_interaction(
        &self,
        owner: &str,
        source_id: &str,
    ) -> Result<Option<Interaction>> {
        validate_public_key(owner)?;
        if source_id.len() > 512 {
            bail!("invalid interaction lookup");
        }
        let json: Option<String> =
            sqlx::query_scalar("SELECT input_json FROM interactions WHERE owner=? AND source_id=?")
                .bind(owner)
                .bind(source_id)
                .fetch_optional(&self.pool)
                .await?;
        json.map(|json| Ok(serde_json::from_str(&json)?))
            .transpose()
    }

    pub async fn catch_up_calendar(
        &self,
        owner: &str,
        timezone: &str,
        enabled: bool,
        now: i64,
    ) -> Result<Snapshot> {
        let day = DayWindow::at(timezone, now)?;
        let current = self.snapshot(owner).await?;
        if !enabled || current.cycle_start >= day.start {
            return Ok(current);
        }
        let command = format!("midnight-{}", day.start);
        match self.reset(owner, &command, day.start).await {
            Ok(snapshot) => Ok(snapshot),
            Err(error) => {
                let reread = self.snapshot(owner).await?;
                if reread.cycle_start >= day.start {
                    Ok(reread)
                } else {
                    Err(error)
                }
            }
        }
    }
}

fn add_bytes(total: usize, additional: usize) -> Result<usize> {
    let result = total
        .checked_add(additional)
        .context("target projection byte count overflow")?;
    if result > QUERY_BYTE_BUDGET {
        bail!("target projection byte budget exceeded");
    }
    Ok(result)
}

fn decode_stream(
    row: &sqlx::sqlite::SqliteRow,
    owner: &str,
    now: i64,
) -> Result<AuthenticatedStream> {
    let event = VerifiedEvent::from_json(&row.try_get::<String, _>("event_json")?)?;
    let authority: StreamAuthority =
        serde_json::from_str(&row.try_get::<String, _>("authority_json")?)?;
    AuthenticatedStream::restore(event, owner, authority, now)
}
