//! Separate presentation producer over the already-classified feeding barrier.
//! Slow discovery, templates or publication cannot delay food/accounting/rewards.
use super::{Store, delivery_state::Unsigned, load_catalog};
use crate::cyberherd::{
    conversation::Context,
    delivery::{Rendered, ratio},
    feeding_delivery::{Broadcast, Overlay},
    ingress::CoreBinding,
    shares::point_units,
    targets::Targets,
};
use anyhow::{Context as _, Result, bail};
use sqlx::{Row as _, Sqlite, Transaction};
use std::collections::BTreeMap;

impl Store {
    /// Called only with a fresh successful authenticated target projection.
    /// A discovery outage retains the barrier rather than guessing a destination.
    pub(crate) async fn queue_next_feeding_announcement(
        &self,
        owner: &str,
        targets: &Targets,
        now: i64,
    ) -> Result<bool> {
        if targets.owner() != owner || now < 0 {
            bail!("feeding target scope/time mismatch");
        }
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let row=sqlx::query("SELECT b.request_id FROM feeding_barriers b WHERE b.owner=? AND NOT EXISTS(SELECT 1 FROM feeding_announcements a WHERE a.owner=b.owner AND a.request_id=b.request_id) ORDER BY b.ingress_seq LIMIT 1")
            .bind(owner).fetch_optional(&mut *tx).await?;
        let Some(row) = row else {
            return Ok(false);
        };
        let id: String = row.try_get("request_id")?;
        // The barrier is immutable and has already classified prior payments.
        let barrier_row=sqlx::query("SELECT ingress_seq,feeding_json,snapshot_json,conversations_json FROM feeding_barriers WHERE owner=? AND request_id=?")
            .bind(owner).bind(&id).fetch_one(&mut *tx).await?;
        let barrier = super::ingress_state::decode_barrier(&barrier_row)?;
        let Some(proof) = &barrier.feeding.presentation else {
            record(&mut tx, owner, &id, None, None).await?;
            tx.commit().await?;
            return Ok(true);
        };
        proof.validate()?;
        if proof.owner != owner {
            bail!("core delegated the feeding notice to another owner");
        }
        let previous:Option<Option<i64>>=sqlx::query_scalar("SELECT event_seq FROM feeding_announcements WHERE owner=? AND identity=? ORDER BY request_id LIMIT 1")
            .bind(owner).bind(&proof.identity).fetch_optional(&mut *tx).await?;
        if let Some(event) = previous {
            record(&mut tx, owner, &id, Some(&proof.identity), event).await?;
            tx.commit().await?;
            return Ok(true);
        }
        let catalog = load_catalog(&mut tx, owner).await?;
        let source = crate::ledger::DurableEvent {
            seq: proof.source_seq,
            event_type: "feeder_confirmed".into(),
            payload_json: "{}".into(),
        };
        let Some(base) = crate::presentation::MessageRenderer::editable_feeding(
            &catalog,
            &source,
            proof,
            barrier.feeding.threshold_sats,
        )?
        else {
            record(&mut tx, owner, &id, Some(&proof.identity), None).await?;
            tx.commit().await?;
            return Ok(true);
        };
        let snapshot = &barrier.snapshot;
        let units = snapshot
            .roster
            .members
            .iter()
            .filter(|(_, m)| m.active)
            .try_fold(0u128, |sum, (key, m)| -> Result<u128> {
                sum.checked_add(point_units(
                    m.amount_sats,
                    snapshot.roster.awards.get(key).copied().unwrap_or_default(),
                    snapshot.settings.minimum_sats.max(1),
                )?)
                .context("feeding herd points overflow")
            })?;
        let values = BTreeMap::from([
            (
                "active_members".into(),
                snapshot.roster.active_count().to_string(),
            ),
            (
                "total_points".into(),
                ratio(units, u128::from(snapshot.settings.minimum_sats.max(1)))?,
            ),
            (
                "member_allocation".into(),
                snapshot.settings.member_percent.to_string(),
            ),
        ]);
        let summary = catalog.render("cyber_herd_feeder_score", &proof.identity, &values)?;
        let compose = |body: Option<String>| -> Result<String> {
            let mut body = body.context("feeding template lost a render target")?;
            if let Some(summary) = &summary {
                body.push_str("\n\n");
                body.push_str(summary);
            }
            if body.trim().is_empty() || body.len() > 32768 {
                bail!("feeding announcement exceeds bound");
            }
            Ok(body)
        };
        let context = Context {
            note: None,
            stream: targets.current().into_iter().find(|t| t.kind == 30311),
        };
        context.validate(owner)?;
        let message = Rendered {
            identity: proof.identity.clone(),
            cycle: snapshot.cycle,
            member: owner.into(),
            conversation: None,
            context,
            category: "feeder_trigger".into(),
            nostr: compose(base.nostr_content)?,
            overlay: compose(base.overlay_message)?,
            accepted: false,
            requires_joining: false,
            additional_members: vec![],
            broadcast: Some(Broadcast {
                request_id: barrier.feeding.request_id,
                proof: proof.clone(),
                goats: base.overlay_goats,
            }),
        };
        message.validate_broadcast(owner)?;
        let event=sqlx::query("INSERT INTO domain_events(owner,cycle,source_id,event_type,snapshot_json) VALUES(?,?,?,'feeding_notice',?)")
            .bind(owner).bind(snapshot.cycle).bind(format!("feeding-announcement:{id}")).bind(serde_json::to_string(&message)?).execute(&mut *tx).await?.last_insert_rowid();
        record(&mut tx, owner, &id, Some(&proof.identity), Some(event)).await?;
        tx.commit().await?;
        Ok(true)
    }

    pub(crate) async fn pending_feeding_overlay(&self, owner: &str) -> Result<Option<Overlay>> {
        let mut tx = self.pool.begin().await?;
        let row=sqlx::query("SELECT m.document_json FROM delivery_overlay o JOIN delivery_messages m ON o.owner=m.owner AND o.identity=m.identity WHERE o.owner=? AND o.event_seq IS NULL AND json_type(m.document_json,'$.broadcast')='object' ORDER BY m.source_seq LIMIT 1")
            .bind(owner).fetch_optional(&mut *tx).await?;
        let Some(row) = row else {
            return Ok(None);
        };
        let message: Rendered = serde_json::from_str(&row.try_get::<String, _>("document_json")?)?;
        message.validate_broadcast(owner)?;
        let binding: String =
            sqlx::query_scalar("SELECT binding_json FROM core_streams WHERE owner=?")
                .bind(owner)
                .fetch_one(&mut *tx)
                .await?;
        let binding: CoreBinding = serde_json::from_str(&binding)?;
        let overlay = Overlay {
            epoch: binding.epoch,
            broadcast: message
                .broadcast
                .context("feeding message lacks confirmed proof")?,
            message: message.overlay,
        };
        overlay.validate()?;
        tx.commit().await?;
        Ok(Some(overlay))
    }
    pub(crate) async fn acknowledge_feeding_overlay(
        &self,
        message: &Overlay,
        seq: u64,
    ) -> Result<()> {
        message.validate()?;
        if seq == 0 || seq > i64::MAX as u64 {
            bail!("invalid feeding overlay receipt");
        }
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let owner = &message.broadcast.proof.owner;
        let identity = &message.broadcast.proof.identity;
        let row=sqlx::query("SELECT m.document_json,o.event_seq FROM delivery_messages m JOIN delivery_overlay o ON m.owner=o.owner AND m.identity=o.identity WHERE m.owner=? AND m.identity=?")
            .bind(owner).bind(identity).fetch_one(&mut *tx).await?;
        let original: Rendered = serde_json::from_str(&row.try_get::<String, _>("document_json")?)?;
        original.validate_broadcast(owner)?;
        if original.broadcast.as_ref() != Some(&message.broadcast)
            || original.overlay != message.message
            || row
                .try_get::<Option<i64>, _>("event_seq")?
                .is_some_and(|n| n != seq as i64)
        {
            bail!("feeding overlay acknowledgment changed original message");
        }
        sqlx::query("UPDATE delivery_overlay SET event_seq=? WHERE owner=? AND identity=?")
            .bind(seq as i64)
            .bind(owner)
            .bind(identity)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(())
    }
}
async fn record(
    tx: &mut Transaction<'_, Sqlite>,
    owner: &str,
    id: &str,
    identity: Option<&str>,
    event: Option<i64>,
) -> Result<()> {
    sqlx::query(
        "INSERT INTO feeding_announcements(owner,request_id,identity,event_seq) VALUES(?,?,?,?)",
    )
    .bind(owner)
    .bind(id)
    .bind(identity)
    .bind(event)
    .execute(&mut **tx)
    .await?;
    Ok(())
}
pub(super) async fn insert_broadcast(
    tx: &mut Transaction<'_, Sqlite>,
    owner: &str,
    message: &Rendered,
) -> Result<()> {
    message.validate_broadcast(owner)?;
    let mut destinations = vec![("broadcast:note".to_owned(), "null".to_owned())];
    if let Some(stream) = &message.context.stream {
        destinations.push((
            format!(
                "stream:{}",
                stream
                    .coordinate
                    .as_ref()
                    .context("feeding stream lacks stable address")?
            ),
            serde_json::to_string(stream)?,
        ));
    }
    for (destination, target) in destinations {
        sqlx::query("INSERT INTO delivery_destinations(owner,identity,destination,branch,target_json) VALUES(?,?,?,NULL,?) ON CONFLICT(owner,identity,destination) DO NOTHING")
            .bind(owner).bind(&message.identity).bind(&destination).bind(target).execute(&mut **tx).await?;
        sqlx::query("INSERT INTO delivery_work(owner,identity,destination) VALUES(?,?,?) ON CONFLICT(owner,identity,destination) DO NOTHING")
            .bind(owner).bind(&message.identity).bind(destination).execute(&mut **tx).await?;
    }
    Ok(())
}
pub(super) fn unsigned_broadcast(
    owner: &str,
    message: &Rendered,
    destination: &str,
    now: i64,
) -> Result<Unsigned> {
    message.validate_broadcast(owner)?;
    let mut tags = vec![vec!["t".into(), "LightningGoats".into()]];
    let kind = if destination == "broadcast:note" {
        1
    } else {
        let stream = message
            .context
            .stream
            .as_ref()
            .context("feeding stream was not frozen")?;
        let coordinate = stream
            .coordinate
            .as_ref()
            .context("feeding stream lacks stable address")?;
        if destination != format!("stream:{coordinate}") {
            bail!("feeding destination changed");
        }
        tags.push(vec![
            "a".into(),
            coordinate.clone(),
            String::new(),
            "root".into(),
        ]);
        1311
    };
    Ok(Unsigned {
        owner: owner.into(),
        kind,
        created_at: u64::try_from(now)?,
        content: message.nostr.clone(),
        tags,
    })
}
