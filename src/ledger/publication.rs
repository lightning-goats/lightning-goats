//! Durable publication plans/attempts/ACKs use the existing append-only event
//! contract. They carry no payment, feeder, membership or signing authority.
use super::{LedgerStore, OutboxEntry, append_event_in_transaction};
use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use sqlx::Row as _;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Plan {
    event_id: String,
    relays: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Attempt {
    event_id: String,
    relay: String,
    lease: Uuid,
    attempt: u64,
    next_attempt: i64,
}
#[derive(Debug, Clone)]
pub struct PublicationWork {
    pub event_id: String,
    pub signed_json: String,
    pub relay: String,
    lease: Uuid,
    attempt: u64,
}

impl LedgerStore {
    pub async fn unplanned_publication(&self) -> Result<Option<OutboxEntry>> {
        let row=sqlx::query("SELECT o.* FROM message_outbox o WHERE o.status IN ('pending','failed') AND NOT EXISTS(SELECT 1 FROM message_outbox_suppressions s WHERE s.event_id=o.event_id) AND NOT EXISTS(SELECT 1 FROM event_log e WHERE e.event_type IN ('nostr_publication_plan','nostr_publication_plan_extension') AND json_extract(e.payload_json,'$.event_id')=o.event_id) ORDER BY o.source_event_seq,o.created_at LIMIT 1")
            .fetch_optional(&self.pool).await?;
        row.map(|r| {
            Ok(OutboxEntry {
                event_id: r.try_get("event_id")?,
                signed_event_json: r.try_get("signed_event_json")?,
                source_event_seq: u64::try_from(r.try_get::<i64, _>("source_event_seq")?)?,
                attempts: u64::try_from(r.try_get::<i64, _>("attempts")?)?,
            })
        })
        .transpose()
    }
    pub async fn plan_publication(&self, event_id: &str, relays: &[String]) -> Result<()> {
        let plan = Plan {
            event_id: event_id.into(),
            relays: crate::relay_routing::public_relays(relays)?,
        };
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let present: bool =
            sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM message_outbox WHERE event_id=?)")
                .bind(event_id)
                .fetch_one(&mut *tx)
                .await?;
        if !present {
            bail!("publication has no retained signed event");
        }
        let old:Option<String>=sqlx::query_scalar("SELECT payload_json FROM event_log WHERE event_type='nostr_publication_plan' AND json_extract(payload_json,'$.event_id')=? ORDER BY seq LIMIT 1").bind(event_id).fetch_optional(&mut *tx).await?;
        if old.is_none() {
            append_event_in_transaction(&mut tx, "nostr_publication_plan", &plan).await?;
        }
        tx.commit().await?;
        Ok(())
    }
    pub async fn reserve_publication(&self, now: i64) -> Result<Option<PublicationWork>> {
        if now < 0 {
            bail!("invalid publication clock");
        }
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let row=sqlx::query("SELECT o.event_id,o.signed_event_json,j.value AS relay,(SELECT a.payload_json FROM event_log a WHERE a.event_type='nostr_publication_attempt' AND json_extract(a.payload_json,'$.event_id')=o.event_id AND json_extract(a.payload_json,'$.relay')=j.value ORDER BY a.seq DESC LIMIT 1) AS last_attempt FROM event_log p JOIN message_outbox o ON o.event_id=json_extract(p.payload_json,'$.event_id') JOIN json_each(p.payload_json,'$.relays') j WHERE p.event_type IN ('nostr_publication_plan','nostr_publication_plan_extension') AND NOT EXISTS(SELECT 1 FROM message_outbox_suppressions s WHERE s.event_id=o.event_id) AND NOT EXISTS(SELECT 1 FROM event_log a WHERE a.event_type='nostr_publication_ack' AND json_extract(a.payload_json,'$.event_id')=o.event_id AND json_extract(a.payload_json,'$.relay')=j.value) AND COALESCE(json_extract((SELECT a.payload_json FROM event_log a WHERE a.event_type='nostr_publication_attempt' AND json_extract(a.payload_json,'$.event_id')=o.event_id AND json_extract(a.payload_json,'$.relay')=j.value ORDER BY a.seq DESC LIMIT 1),'$.next_attempt'),0)<=? ORDER BY CASE WHEN EXISTS(SELECT 1 FROM event_log m WHERE m.seq=o.source_event_seq AND m.event_type IN ('nostr_relay_metadata_intent','nostr_relay_metadata_observed')) THEN 1 ELSE 0 END,COALESCE(json_extract((SELECT a.payload_json FROM event_log a WHERE a.event_type='nostr_publication_attempt' AND json_extract(a.payload_json,'$.event_id')=o.event_id AND json_extract(a.payload_json,'$.relay')=j.value ORDER BY a.seq DESC LIMIT 1),'$.next_attempt'),0),o.source_event_seq,j.key LIMIT 1")
            .bind(now).fetch_optional(&mut *tx).await?;
        let Some(row) = row else { return Ok(None) };
        let old: Option<Attempt> = row
            .try_get::<Option<String>, _>("last_attempt")?
            .map(|s| serde_json::from_str(&s))
            .transpose()?;
        let attempt = Attempt {
            event_id: row.try_get("event_id")?,
            relay: row.try_get("relay")?,
            lease: Uuid::new_v4(),
            attempt: old.map_or(1, |a| a.attempt.saturating_add(1)),
            next_attempt: now.saturating_add(30),
        };
        append_event_in_transaction(&mut tx, "nostr_publication_attempt", &attempt).await?;
        let job = PublicationWork {
            event_id: attempt.event_id,
            signed_json: row.try_get("signed_event_json")?,
            relay: attempt.relay,
            lease: attempt.lease,
            attempt: attempt.attempt,
        };
        tx.commit().await?;
        Ok(Some(job))
    }
    pub async fn finish_publication(
        &self,
        job: &PublicationWork,
        accepted: bool,
        now: i64,
    ) -> Result<()> {
        if now < 0 {
            bail!("invalid publication clock");
        }
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let current:String=sqlx::query_scalar("SELECT payload_json FROM event_log WHERE event_type='nostr_publication_attempt' AND json_extract(payload_json,'$.event_id')=? AND json_extract(payload_json,'$.relay')=? ORDER BY seq DESC LIMIT 1")
            .bind(&job.event_id).bind(&job.relay).fetch_one(&mut *tx).await?;
        let current: Attempt = serde_json::from_str(&current)?;
        if current.lease != job.lease {
            return Ok(());
        }
        if accepted {
            let exists:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM event_log WHERE event_type='nostr_publication_ack' AND json_extract(payload_json,'$.event_id')=? AND json_extract(payload_json,'$.relay')=?)")
                .bind(&job.event_id).bind(&job.relay).fetch_one(&mut *tx).await?;
            if !exists {
                append_event_in_transaction(&mut tx,"nostr_publication_ack",&serde_json::json!({"event_id":job.event_id,"relay":job.relay,"accepted_at":now,"broadcaster_acceptance":crate::relay_routing::is_broadcaster(&job.relay)})).await?;
            }
            if !crate::relay_routing::is_broadcaster(&job.relay) {
                sqlx::query("UPDATE message_outbox SET status='published',attempts=attempts+1,last_error=NULL,published_at=? WHERE event_id=? AND status IN ('pending','failed')").bind(now).bind(&job.event_id).execute(&mut *tx).await?;
            }
        } else {
            sqlx::query("UPDATE message_outbox SET attempts=attempts+1,last_error='relay acceptance pending',status=CASE WHEN status='pending' THEN 'failed' ELSE status END WHERE event_id=?")
                .bind(&job.event_id).execute(&mut *tx).await?;
            let retry = Attempt {
                event_id: job.event_id.clone(),
                relay: job.relay.clone(),
                lease: job.lease,
                attempt: job.attempt,
                next_attempt: now.saturating_add(1i64 << job.attempt.min(8)),
            };
            append_event_in_transaction(&mut tx, "nostr_publication_attempt", &retry).await?;
        }
        tx.commit().await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn acknowledged_relays_survive_restart_while_other_relays_retry_without_starving_new_notes()
     {
        let dir = tempfile::tempdir().unwrap();
        let url = format!("sqlite://{}", dir.path().join("core.db").display());
        let db = LedgerStore::connect(&url).await.unwrap();
        let a = db.append_event("test", &()).await.unwrap();
        db.enqueue_signed_message(a, "event-a", "original signed bytes")
            .await
            .unwrap();
        let relays = vec![
            "wss://direct.example/".into(),
            "wss://sendit.nosflare.com/".into(),
        ];
        db.plan_publication("event-a", &relays).await.unwrap();
        db.plan_publication("event-a", &["wss://different.example/".into()])
            .await
            .unwrap();
        let first = db.reserve_publication(100).await.unwrap().unwrap();
        assert_eq!(first.relay, "wss://direct.example/");
        db.finish_publication(&first, false, 100).await.unwrap();
        let second = db.reserve_publication(100).await.unwrap().unwrap();
        assert!(crate::relay_routing::is_broadcaster(&second.relay));
        db.finish_publication(&second, true, 100).await.unwrap();
        assert!(db.next_outbox_entry().await.unwrap().is_some()); // broadcaster does not satisfy direct relay acceptance
        drop(db);
        let db = LedgerStore::connect(&url).await.unwrap();
        assert!(db.reserve_publication(101).await.unwrap().is_none());
        let b = db.append_event("test", &()).await.unwrap();
        db.enqueue_signed_message(b, "event-b", "other original bytes")
            .await
            .unwrap();
        db.plan_publication("event-b", &relays[..1]).await.unwrap();
        let newer = db.reserve_publication(101).await.unwrap().unwrap();
        assert_eq!(newer.event_id, "event-b");
        db.finish_publication(&newer, true, 101).await.unwrap();
        let retry = db.reserve_publication(103).await.unwrap().unwrap();
        assert_eq!(retry.event_id, "event-a");
        assert_eq!(retry.signed_json, "original signed bytes");
        assert_eq!(retry.relay, "wss://direct.example/");
        db.finish_publication(&retry, true, 103).await.unwrap();
        db.finish_publication(&retry, true, 103).await.unwrap();
        assert!(db.reserve_publication(200).await.unwrap().is_none());
        assert!(db.next_outbox_entry().await.unwrap().is_none());
        assert_eq!(db.feed_credit_sats().await.unwrap(), 0);
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelayMetadataIntent {
    pub author: String,
    pub basis_event_id: String,
    pub created_at: u64,
    pub tags: Vec<Vec<String>>,
    pub source_seq: u64,
}
impl LedgerStore {
    pub async fn relay_metadata_intent(
        &self,
        author: &str,
        basis_event_id: &str,
        tags: Vec<Vec<String>>,
        now: i64,
    ) -> Result<RelayMetadataIntent> {
        use sha2::{Digest as _, Sha256};
        crate::nostr_event::validate_public_key(author)?;
        crate::nostr_event::validate_hex(basis_event_id, 32)?;
        if now <= 0 || tags.len() > 64 {
            bail!("invalid relay metadata intent");
        }
        let identity = hex::encode(Sha256::digest(serde_json::to_vec(&(
            author,
            basis_event_id,
            &tags,
        ))?));
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let old=sqlx::query("SELECT seq,payload_json FROM event_log WHERE event_type='nostr_relay_metadata_intent' AND json_extract(payload_json,'$.identity')=? ORDER BY seq LIMIT 1")
            .bind(&identity).fetch_optional(&mut *tx).await?;
        let intent = if let Some(old) = old {
            let mut saved: RelayMetadataIntent =
                serde_json::from_value(
                    serde_json::from_str::<serde_json::Value>(
                        &old.get::<String, _>("payload_json"),
                    )?["intent"]
                        .clone(),
                )?;
            saved.source_seq = u64::try_from(old.get::<i64, _>("seq"))?;
            saved
        } else {
            let mut value = RelayMetadataIntent {
                author: author.into(),
                basis_event_id: basis_event_id.into(),
                created_at: now as u64,
                tags,
                source_seq: 0,
            };
            value.source_seq = append_event_in_transaction(
                &mut tx,
                "nostr_relay_metadata_intent",
                &serde_json::json!({"identity":identity,"intent":value}),
            )
            .await?;
            value
        };
        tx.commit().await?;
        Ok(intent)
    }
    pub async fn relay_metadata_signed(
        &self,
        intent: &RelayMetadataIntent,
    ) -> Result<Option<crate::nostr_event::VerifiedEvent>> {
        let raw: Option<String> = sqlx::query_scalar(
            "SELECT signed_event_json FROM message_outbox WHERE source_event_seq=?",
        )
        .bind(i64::try_from(intent.source_seq)?)
        .fetch_optional(&self.pool)
        .await?;
        raw.map(|r| crate::nostr_event::VerifiedEvent::from_json(&r))
            .transpose()
    }
    pub async fn retain_relay_metadata(
        &self,
        intent: &RelayMetadataIntent,
        event: &crate::nostr_event::VerifiedEvent,
    ) -> Result<()> {
        let e = event.event();
        if e.kind != 10002
            || e.pubkey != intent.author
            || e.created_at != intent.created_at
            || e.tags != intent.tags
            || !e.content.is_empty()
        {
            bail!("relay metadata signer changed frozen fields");
        }
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let saved:String=sqlx::query_scalar("SELECT payload_json FROM event_log WHERE seq=? AND event_type='nostr_relay_metadata_intent'").bind(i64::try_from(intent.source_seq)?).fetch_one(&mut *tx).await?;
        let saved: serde_json::Value = serde_json::from_str(&saved)?;
        let mut expected = intent.clone();
        expected.source_seq = 0;
        if saved["intent"] != serde_json::to_value(&expected)? {
            bail!("relay metadata intent differs from retained source");
        }
        let raw = serde_json::to_string(e)?;
        sqlx::query("INSERT INTO message_outbox(event_id,signed_event_json,status,source_event_seq) VALUES(?,?,'pending',?) ON CONFLICT(source_event_seq) WHERE source_event_seq IS NOT NULL DO NOTHING")
            .bind(&e.id).bind(&raw).bind(i64::try_from(intent.source_seq)?).execute(&mut *tx).await?;
        let old: String = sqlx::query_scalar(
            "SELECT signed_event_json FROM message_outbox WHERE source_event_seq=?",
        )
        .bind(i64::try_from(intent.source_seq)?)
        .fetch_one(&mut *tx)
        .await?;
        if old != raw {
            bail!("relay metadata signature already frozen");
        }
        tx.commit().await?;
        Ok(())
    }
}

impl LedgerStore {
    /// Reuse the author's already-signed NIP-65 record on each public destination.
    /// Additive metadata routes never rewrite an ordinary message's frozen plan.
    pub async fn route_relay_metadata(
        &self,
        event: &crate::nostr_event::VerifiedEvent,
        relays: &[String],
    ) -> Result<()> {
        if event.event().kind != 10002 {
            bail!("expected signed relay metadata");
        }
        let destinations = crate::relay_routing::public_relays(relays)?;
        let e = event.event();
        let raw = serde_json::to_string(e)?;
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let old: Option<String> =
            sqlx::query_scalar("SELECT signed_event_json FROM message_outbox WHERE event_id=?")
                .bind(&e.id)
                .fetch_optional(&mut *tx)
                .await?;
        if let Some(old) = old {
            if old != raw {
                bail!("retained relay metadata bytes differ");
            }
        } else {
            let seq = append_event_in_transaction(
                &mut tx,
                "nostr_relay_metadata_observed",
                &serde_json::json!({"event_id":e.id}),
            )
            .await?;
            sqlx::query("INSERT INTO message_outbox(event_id,signed_event_json,status,source_event_seq) VALUES(?,?,'pending',?)")
                .bind(&e.id).bind(&raw).bind(i64::try_from(seq)?).execute(&mut *tx).await?;
        }
        let previous:Vec<String>=sqlx::query_scalar("SELECT j.value FROM event_log p JOIN json_each(p.payload_json,'$.relays') j WHERE p.event_type IN ('nostr_publication_plan','nostr_publication_plan_extension') AND json_extract(p.payload_json,'$.event_id')=?")
            .bind(&e.id).fetch_all(&mut *tx).await?;
        let new: Vec<_> = destinations
            .into_iter()
            .filter(|r| !previous.contains(r))
            .collect();
        if !new.is_empty() {
            append_event_in_transaction(
                &mut tx,
                "nostr_publication_plan_extension",
                &Plan {
                    event_id: e.id.clone(),
                    relays: new,
                },
            )
            .await?;
        }
        tx.commit().await?;
        Ok(())
    }
}

#[cfg(test)]
mod metadata_tests {
    use super::*;
    #[tokio::test]
    async fn frozen_metadata_and_additive_routes_recover_without_new_signatures_or_paid_effects() {
        let root = tempfile::tempdir().unwrap();
        let url = format!("sqlite://{}", root.path().join("core.db").display());
        let db = LedgerStore::connect(&url).await.unwrap();
        let author = crate::nostr_event::test_event(1, 0, 1, "", vec![]).pubkey;
        let tags = vec![vec![
            "r".into(),
            "wss://storage.example/".into(),
            "write".into(),
        ]];
        let intent = db
            .relay_metadata_intent(&author, &"11".repeat(32), tags.clone(), 100)
            .await
            .unwrap();
        let same = db
            .relay_metadata_intent(&author, &"11".repeat(32), tags.clone(), 200)
            .await
            .unwrap();
        assert_eq!(intent.source_seq, same.source_seq);
        assert_eq!(same.created_at, 100);
        let signed = crate::nostr_event::VerifiedEvent::verify(crate::nostr_event::test_event(
            1, 10002, 100, "", tags,
        ))
        .unwrap();
        db.retain_relay_metadata(&intent, &signed).await.unwrap();
        db.route_relay_metadata(&signed, &["wss://first.example/".into()])
            .await
            .unwrap();
        let first = db.reserve_publication(100).await.unwrap().unwrap();
        db.finish_publication(&first, true, 100).await.unwrap();
        drop(db);
        let db = LedgerStore::connect(&url).await.unwrap();
        assert_eq!(
            db.relay_metadata_signed(&intent)
                .await
                .unwrap()
                .unwrap()
                .event(),
            signed.event()
        );
        db.route_relay_metadata(
            &signed,
            &[
                "wss://first.example/".into(),
                "wss://second.example/".into(),
            ],
        )
        .await
        .unwrap();
        let second = db.reserve_publication(101).await.unwrap().unwrap();
        assert_eq!(second.relay, "wss://second.example/");
        assert_eq!(second.event_id, first.event_id);
        db.finish_publication(&second, true, 101).await.unwrap();
        assert!(db.reserve_publication(200).await.unwrap().is_none());
        assert_eq!(db.feed_credit_sats().await.unwrap(), 0);
        let renderer = crate::presentation::MessageRenderer::embedded().unwrap();
        for event in db.events_after(0, 100).await.unwrap() {
            assert!(
                renderer
                    .render_with_zap(&event, 1000, None)
                    .unwrap()
                    .nostr_content
                    .is_none()
            );
        }
    }
}
