use super::delivery_state::Unsigned;
use super::{Store, decode_snapshot, load_catalog};
use crate::{
    cyberherd::{
        delivery::ratio,
        delivery_worker::RelayAck,
        ingress::hash,
        lifecycle::{self, Observation},
        targets::{AuthenticatedStream, StreamAuthority, select_active_stream},
    },
    nostr::SignedNostrEvent,
    nostr_event::VerifiedEvent,
};
use anyhow::{Context as _, Result, bail};
use serde::{Deserialize, Serialize};
use sqlx::{Row as _, Sqlite, Transaction};
use std::collections::BTreeMap;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StreamEvidence {
    coordinate: String,
    event: SignedNostrEvent,
    authority: StreamAuthority,
}
impl From<&AuthenticatedStream> for StreamEvidence {
    fn from(s: &AuthenticatedStream) -> Self {
        Self {
            coordinate: s.coordinate().into(),
            event: s.event().event().clone(),
            authority: s.authority().clone(),
        }
    }
}
#[derive(Debug, Serialize, Deserialize)]
struct Context {
    state: crate::core_bridge::LifecycleContext,
    stream: Option<StreamEvidence>,
}
pub(crate) struct Job {
    pub owner: String,
    pub identity: String,
    pub destination: String,
    pub purpose: String,
    pub lease: Uuid,
    pub unsigned: Unsigned,
    pub signed: Option<String>,
    pub relay: String,
    pub remaining_relays: Vec<String>,
}

impl Store {
    async fn notice_stream(
        &self,
        owner: &str,
        evening: bool,
        now: i64,
    ) -> Result<Option<AuthenticatedStream>> {
        let rows = sqlx::query("SELECT e.event_json,v.authority_json FROM stream_heads h JOIN stream_versions v ON h.owner=v.owner AND h.event_id=v.event_id JOIN observed_events e ON v.owner=e.owner AND v.event_id=e.event_id WHERE h.owner=? AND e.created_at>=? ORDER BY e.created_at DESC,e.event_id DESC LIMIT 129")
            .bind(owner).bind(now.saturating_sub(3600)).fetch_all(&self.pool).await?;
        if rows.len() > 128 {
            bail!("notice stream projection exceeds bound");
        }
        let mut streams = vec![];
        for row in rows {
            streams.push(AuthenticatedStream::restore(
                VerifiedEvent::from_json(&row.try_get::<String, _>("event_json")?)?,
                owner,
                serde_json::from_str(&row.try_get::<String, _>("authority_json")?)?,
                now,
            )?);
        }
        if let Some(active) = select_active_stream(&streams, owner, now)? {
            return Ok(Some(active));
        }
        Ok(if evening {
            streams.into_iter().find(|s| s.recently_ended(now))
        } else {
            None
        })
    }

    /// State transitions and their frozen per-destination notices commit
    /// together. Unknown readings never update the baseline or cooldown.
    pub(crate) async fn observe_lifecycle(
        &self,
        owner: &str,
        observation: Observation<'_>,
    ) -> Result<()> {
        crate::nostr_event::validate_public_key(owner)?;
        let Observation {
            state,
            discovery_available,
            timezone,
            relays,
            now,
        } = observation;
        let relays = crate::payout::invoice::receipt_relays(relays)?;
        let Some(paused) = state.confirmed_override(now) else {
            return Ok(());
        };
        if !state.nostr_enabled {
            return Ok(());
        }
        let observed = state
            .observed_at
            .context("missing confirmed override time")?;
        let evening = paused && lifecycle::evening(timezone, state.solar.as_ref(), now)?;
        let stream = if discovery_available {
            self.notice_stream(owner, evening, now).await?
        } else {
            None
        };
        let context = Context {
            state: state.clone(),
            stream: stream.as_ref().map(StreamEvidence::from),
        };
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let previous = sqlx::query("SELECT * FROM lifecycle_observer WHERE owner=?")
            .bind(owner)
            .fetch_optional(&mut *tx)
            .await?;
        let old = previous
            .as_ref()
            .map(|r| r.try_get::<bool, _>("override_enabled"))
            .transpose()?;
        let old_at = previous
            .as_ref()
            .map(|r| r.try_get::<i64, _>("observed_at"))
            .transpose()?;
        let mut seq = previous
            .as_ref()
            .map(|r| r.try_get::<i64, _>("transition_seq"))
            .transpose()?
            .unwrap_or(0);
        if old_at.is_some_and(|at| observed < at) {
            return Ok(());
        }
        if old_at == Some(observed) && old.is_some_and(|p| p != paused) {
            bail!("conflicting lifecycle observation timestamp");
        }
        let snapshot = decode_snapshot(
            &sqlx::query("SELECT * FROM owners WHERE owner=?")
                .bind(owner)
                .fetch_one(&mut *tx)
                .await?,
        )?;
        let count = snapshot.roster.active_count();
        let mut values = BTreeMap::from([
            (
                "minimum_sats".into(),
                snapshot.settings.minimum_sats.to_string(),
            ),
            (
                "member_allocation".into(),
                snapshot.settings.member_percent.to_string(),
            ),
            (
                "pool_percent".into(),
                snapshot.settings.member_percent.to_string(),
            ),
            ("active_members".into(), count.to_string()),
            (
                "max_members".into(),
                snapshot.settings.max_members.to_string(),
            ),
            (
                "spots_remaining".into(),
                usize::from(snapshot.settings.max_members)
                    .saturating_sub(count)
                    .to_string(),
            ),
        ]);
        let total = snapshot
            .roster
            .members
            .iter()
            .filter(|(_, m)| m.active)
            .try_fold(0u128, |sum, (key, m)| -> Result<u128> {
                sum.checked_add(crate::cyberherd::shares::point_units(
                    m.amount_sats,
                    snapshot.roster.awards.get(key).copied().unwrap_or_default(),
                    snapshot.settings.minimum_sats.max(1),
                )?)
                .context("herd points overflow")
            })?;
        values.insert(
            "total_points".into(),
            ratio(total, u128::from(snapshot.settings.minimum_sats.max(1)))?,
        );
        let catalog = load_catalog(&mut tx, owner).await?;
        if old != Some(paused) && (old.is_some() || paused) {
            seq = seq
                .checked_add(1)
                .context("lifecycle transition overflow")?;
            let identity = hash(&(owner, "availability", seq))?;
            let category = if evening {
                "feeder_requests_paused_night"
            } else if paused {
                "feeder_requests_paused"
            } else {
                "feeder_requests_enabled"
            };
            if let Some(text) = catalog.render(category, &identity, &values)? {
                queue(
                    &mut tx,
                    owner,
                    &identity,
                    if paused { "pause" } else { "resume" },
                    &text,
                    &context,
                    &relays,
                    now,
                )
                .await?;
            }
        }
        sqlx::query("INSERT INTO lifecycle_observer(owner,override_enabled,observed_at,transition_seq) VALUES(?,?,?,?) ON CONFLICT(owner) DO UPDATE SET override_enabled=excluded.override_enabled,observed_at=excluded.observed_at,transition_seq=excluded.transition_seq")
            .bind(owner).bind(paused).bind(observed).bind(seq).execute(&mut *tx).await?;
        if paused {
            // Do not let an unsubmitted invitation block the pause on its chat
            // destination or appear after the operator has stopped feeding.
            sqlx::query("UPDATE notice_destinations SET canceled=1 WHERE owner=? AND canceled=0 AND EXISTS(SELECT 1 FROM notice_events e WHERE e.owner=notice_destinations.owner AND e.identity=notice_destinations.identity AND e.purpose='invitation')")
                .bind(owner).execute(&mut *tx).await?;
        }
        if !paused && let Some(stream) = &stream {
            let last: Option<i64> = sqlx::query_scalar(
                "SELECT queued_at FROM lifecycle_invitations WHERE owner=? AND coordinate=?",
            )
            .bind(owner)
            .bind(stream.coordinate())
            .fetch_optional(&mut *tx)
            .await?;
            let ack:Option<i64> = sqlx::query_scalar("SELECT MAX(r.acknowledged_at) FROM notice_receipts r JOIN notice_events e ON r.owner=e.owner AND r.identity=e.identity WHERE e.owner=? AND e.purpose='invitation' AND json_extract(e.context_json,'$.stream.coordinate')=?")
                    .bind(owner).bind(stream.coordinate()).fetch_one(&mut *tx).await?;
            if last
                .into_iter()
                .chain(ack)
                .all(|at| now.saturating_sub(at) >= 7200)
            {
                let identity = hash(&(owner, "invitation", stream.coordinate(), now))?;
                if let Some(mut text) =
                    catalog.render("cyber_herd_invitation", &identity, &values)?
                {
                    let capacity = if count >= usize::from(snapshot.settings.max_members) {
                        "cyber_herd_space_full"
                    } else {
                        "cyber_herd_space_open"
                    };
                    if let Some(footer) = catalog.render(capacity, &identity, &values)? {
                        text.push_str("\n\n");
                        text.push_str(&footer);
                    }
                    queue(
                        &mut tx,
                        owner,
                        &identity,
                        "invitation",
                        &text,
                        &context,
                        &relays,
                        now,
                    )
                    .await?;
                    sqlx::query("INSERT INTO lifecycle_invitations(owner,coordinate,queued_at) VALUES(?,?,?) ON CONFLICT(owner,coordinate) DO UPDATE SET queued_at=excluded.queued_at")
                            .bind(owner).bind(stream.coordinate()).bind(now).execute(&mut *tx).await?;
                }
            }
        }
        tx.commit().await?;
        Ok(())
    }

    pub(crate) async fn notice_invitation_live(&self, job: &Job, now: i64) -> Result<bool> {
        let row = sqlx::query("SELECT e.context_json,e.created_at,d.canceled FROM notice_events e JOIN notice_destinations d ON e.owner=d.owner AND e.identity=d.identity WHERE e.owner=? AND e.identity=? AND d.destination=?")
            .bind(&job.owner).bind(&job.identity).bind(&job.destination).fetch_one(&self.pool).await?;
        if row.try_get::<bool, _>("canceled")?
            || !(0..7200).contains(&now.saturating_sub(row.try_get("created_at")?))
        {
            return Ok(false);
        }
        let context: Context = serde_json::from_str(&row.try_get::<String, _>("context_json")?)?;
        let Some(original) = context.stream else {
            bail!("invitation lost authenticated stream");
        };
        let active = self.notice_stream(&job.owner, false, now).await?;
        if active.is_none_or(|s| s.coordinate() != original.coordinate) {
            return Ok(false);
        }
        let ack:Option<i64> = sqlx::query_scalar("SELECT MAX(r.acknowledged_at) FROM notice_receipts r JOIN notice_events e ON r.owner=e.owner AND r.identity=e.identity WHERE e.owner=? AND e.identity<>? AND e.purpose='invitation' AND json_extract(e.context_json,'$.stream.coordinate')=?")
            .bind(&job.owner).bind(&job.identity).bind(&original.coordinate).fetch_one(&self.pool).await?;
        Ok(ack.is_none_or(|at| now.saturating_sub(at) >= 7200))
    }

    pub(crate) async fn reserve_notice(&self, owner: &str, now: i64) -> Result<Option<Job>> {
        if now < 0 {
            bail!("invalid notice time");
        }
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        // A stale invitation cannot block later availability notices. Signed
        // bytes remain retained for audit; cancellation never creates a new ID.
        sqlx::query("UPDATE notice_destinations SET canceled=1 WHERE owner=? AND canceled=0 AND EXISTS(SELECT 1 FROM notice_events e WHERE e.owner=notice_destinations.owner AND e.identity=notice_destinations.identity AND e.purpose='invitation' AND e.created_at<=?)")
            .bind(owner).bind(now.saturating_sub(7200)).execute(&mut *tx).await?;
        let rows=sqlx::query("SELECT e.identity,e.purpose,e.relays_json,e.seq,d.* FROM notice_events e JOIN notice_destinations d ON e.owner=d.owner AND e.identity=d.identity WHERE e.owner=? AND d.canceled=0 AND d.lease_until<=? AND (SELECT COUNT(*) FROM notice_receipts r WHERE r.owner=d.owner AND r.identity=d.identity AND r.destination=d.destination)<json_array_length(e.relays_json) ORDER BY d.attempted_at,e.seq,d.destination LIMIT 32")
            .bind(owner).bind(now).fetch_all(&mut *tx).await?;
        for row in rows {
            let identity: String = row.try_get("identity")?;
            let destination: String = row.try_get("destination")?;
            if row.try_get::<String, _>("purpose")? == "invitation" {
                let current = self.notice_stream(owner, false, now).await?;
                if current.is_none_or(|s| destination != format!("stream:{}", s.coordinate())) {
                    sqlx::query("UPDATE notice_destinations SET canceled=1 WHERE owner=? AND identity=? AND destination=?")
                        .bind(owner).bind(&identity).bind(&destination).execute(&mut *tx).await?;
                    continue;
                }
            }
            let relays: Vec<String> =
                serde_json::from_str(&row.try_get::<String, _>("relays_json")?)?;
            let mut remaining_relays = Vec::new();
            for relay in relays {
                let done:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM notice_receipts WHERE owner=? AND identity=? AND destination=? AND relay=?)")
                    .bind(owner).bind(&identity).bind(&destination).bind(&relay).fetch_one(&mut *tx).await?;
                if done {
                    continue;
                }
                // Ordering is per destination AND relay: one unavailable relay
                // cannot prevent the other relay from seeing transitions in order.
                let blocked:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM notice_events e JOIN notice_destinations d ON e.owner=d.owner AND e.identity=d.identity WHERE e.owner=? AND e.seq<? AND d.destination=? AND d.canceled=0 AND EXISTS(SELECT 1 FROM json_each(e.relays_json) WHERE value=?) AND NOT EXISTS(SELECT 1 FROM notice_receipts r WHERE r.owner=d.owner AND r.identity=d.identity AND r.destination=d.destination AND r.relay=?))")
                    .bind(owner).bind(row.try_get::<i64,_>("seq")?).bind(&destination).bind(&relay).bind(&relay).fetch_one(&mut *tx).await?;
                if !blocked {
                    remaining_relays.push(relay);
                }
            }
            if let Some(relay) = remaining_relays.first().cloned() {
                let lease = Uuid::new_v4();
                sqlx::query("UPDATE notice_destinations SET lease_id=?,lease_until=?,attempted_at=? WHERE owner=? AND identity=? AND destination=?")
                    .bind(lease.to_string()).bind(now.saturating_add(120)).bind(now).bind(owner).bind(&identity).bind(&destination).execute(&mut *tx).await?;
                let job = Job {
                    owner: owner.into(),
                    identity,
                    destination,
                    purpose: row.try_get("purpose")?,
                    lease,
                    unsigned: serde_json::from_str(&row.try_get::<String, _>("unsigned_json")?)?,
                    signed: row.try_get("signed_json")?,
                    relay,
                    remaining_relays,
                };
                tx.commit().await?;
                return Ok(Some(job));
            }
            sqlx::query("UPDATE notice_destinations SET attempted_at=? WHERE owner=? AND identity=? AND destination=?").bind(now).bind(owner).bind(&identity).bind(&destination).execute(&mut *tx).await?;
        }
        tx.commit().await?;
        Ok(None)
    }
    pub(crate) async fn save_notice_signature(
        &self,
        job: &Job,
        event: &VerifiedEvent,
    ) -> Result<String> {
        job.unsigned.validate_signed(event)?;
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        require_job(&mut tx, job).await?;
        sqlx::query("UPDATE notice_destinations SET signed_json=? WHERE owner=? AND identity=? AND destination=? AND signed_json IS NULL")
            .bind(serde_json::to_string(event.event())?).bind(&job.owner).bind(&job.identity).bind(&job.destination).execute(&mut *tx).await?;
        let raw:String=sqlx::query_scalar("SELECT signed_json FROM notice_destinations WHERE owner=? AND identity=? AND destination=?").bind(&job.owner).bind(&job.identity).bind(&job.destination).fetch_one(&mut *tx).await?;
        job.unsigned
            .validate_signed(&VerifiedEvent::from_json(&raw)?)?;
        tx.commit().await?;
        Ok(raw)
    }
    pub(crate) async fn finish_notice(
        &self,
        job: &Job,
        ack: Option<&RelayAck>,
        now: i64,
    ) -> Result<()> {
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        require_job(&mut tx, job).await?;
        if let Some(ack) = ack {
            let raw:String=sqlx::query_scalar("SELECT signed_json FROM notice_destinations WHERE owner=? AND identity=? AND destination=?").bind(&job.owner).bind(&job.identity).bind(&job.destination).fetch_one(&mut *tx).await?;
            let event = VerifiedEvent::from_json(&raw)?;
            job.unsigned.validate_signed(&event)?;
            ack.check(&event, &job.relay)?;
            sqlx::query("INSERT INTO notice_receipts(owner,identity,destination,relay,acknowledged_at) VALUES(?,?,?,?,?) ON CONFLICT(owner,identity,destination,relay) DO NOTHING")
                .bind(&job.owner).bind(&job.identity).bind(&job.destination).bind(&job.relay).bind(ack.at()).execute(&mut *tx).await?;
        }
        sqlx::query("UPDATE notice_destinations SET lease_id=NULL,lease_until=? WHERE owner=? AND identity=? AND destination=?")
            .bind(now.saturating_add(if ack.is_some(){0}else{5})).bind(&job.owner).bind(&job.identity).bind(&job.destination).execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(())
    }
}

#[allow(clippy::too_many_arguments)]
async fn queue(
    tx: &mut Transaction<'_, Sqlite>,
    owner: &str,
    identity: &str,
    purpose: &str,
    text: &str,
    context: &Context,
    relays: &[String],
    now: i64,
) -> Result<()> {
    if text.is_empty() || text.len() > 32768 {
        bail!("invalid broadcast message length");
    }
    let pending:i64=sqlx::query_scalar("SELECT COUNT(*) FROM notice_destinations d JOIN notice_events e ON e.owner=d.owner AND e.identity=d.identity WHERE d.owner=? AND d.canceled=0 AND (SELECT COUNT(*) FROM notice_receipts r WHERE r.owner=d.owner AND r.identity=d.identity AND r.destination=d.destination)<json_array_length(e.relays_json)")
        .bind(owner).fetch_one(&mut **tx).await?;
    let added = if purpose == "invitation" {
        1
    } else {
        1 + i64::from(context.stream.is_some())
    };
    if pending + added > 2048 {
        bail!("broadcast backlog is full");
    }
    sqlx::query("INSERT INTO notice_events(owner,identity,purpose,context_json,relays_json,created_at) VALUES(?,?,?,?,?,?)")
        .bind(owner).bind(identity).bind(purpose).bind(serde_json::to_string(context)?).bind(serde_json::to_string(relays)?).bind(now).execute(&mut **tx).await?;
    let mut destinations = vec![];
    if purpose != "invitation" {
        destinations.push((
            "note".to_owned(),
            1,
            vec![vec!["t".into(), "LightningGoats".into()]],
        ));
    }
    if let Some(stream) = &context.stream {
        destinations.push((
            format!("stream:{}", stream.coordinate),
            1311,
            vec![
                vec![
                    "a".into(),
                    stream.coordinate.clone(),
                    String::new(),
                    "root".into(),
                ],
                vec!["t".into(), "CyberHerd".into()],
            ],
        ));
    }
    for (destination, kind, tags) in destinations {
        let unsigned = Unsigned {
            owner: owner.into(),
            kind,
            created_at: u64::try_from(now)?,
            content: text.into(),
            tags,
        };
        sqlx::query("INSERT INTO notice_destinations(owner,identity,destination,unsigned_json) VALUES(?,?,?,?)")
            .bind(owner).bind(identity).bind(destination).bind(serde_json::to_string(&unsigned)?).execute(&mut **tx).await?;
    }
    Ok(())
}
async fn require_job(tx: &mut Transaction<'_, Sqlite>, job: &Job) -> Result<()> {
    let row=sqlx::query("SELECT lease_id,unsigned_json FROM notice_destinations WHERE owner=? AND identity=? AND destination=?").bind(&job.owner).bind(&job.identity).bind(&job.destination).fetch_one(&mut **tx).await?;
    if row.try_get::<Option<String>, _>("lease_id")?.as_deref() != Some(&job.lease.to_string())
        || serde_json::from_str::<Unsigned>(&row.try_get::<String, _>("unsigned_json")?)?
            != job.unsigned
    {
        bail!("notice lease/frozen fields changed");
    }
    Ok(())
}
