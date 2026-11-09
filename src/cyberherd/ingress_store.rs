//! Receiving checkpoint and ordered work commit together in the separate store.
use anyhow::{Context, Result, bail};
use sqlx::{Row, Sqlite, Transaction};
use uuid::Uuid;

use super::{Store, decode_snapshot};
use crate::{
    core_bridge::{AuthenticatedBatch, EventBody, Feeding},
    cyberherd::ingress::{
        CoreBinding, CoreCursor, FeedingBarrier, Input, MAX_PENDING, Pending, hash,
        payment_identity, validate_owner,
    },
};

pub(super) async fn enqueue(
    tx: &mut Transaction<'_, Sqlite>,
    owner: &str,
    key: &str,
    kind: &str,
    json: &str,
) -> Result<()> {
    let pending: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM ingress WHERE owner=? AND state='pending'")
            .bind(owner)
            .fetch_one(&mut **tx)
            .await?;
    if pending >= MAX_PENDING {
        return Err(crate::cyberherd::ingress::IngressFull.into());
    }
    sqlx::query("INSERT INTO ingress(owner,source_key,kind,payload_json) VALUES(?,?,?,?)")
        .bind(owner)
        .bind(key)
        .bind(kind)
        .bind(json)
        .execute(&mut **tx)
        .await?;
    Ok(())
}

impl Store {
    /// A confirmed pause/unknown observation restricts all already staged paid
    /// work, including items behind a blocked head. It cannot resume anything.
    pub async fn defer_pending_payments(&self, owner: &str) -> Result<()> {
        validate_owner(owner)?;
        sqlx::query("UPDATE ingress SET was_deferred=1 WHERE owner=? AND kind='payment' AND state='pending' AND was_deferred=0")
            .bind(owner).execute(&self.pool).await?;
        Ok(())
    }

    pub async fn relay_window(&self, owner: &str, query_key: &str, start: i64) -> Result<i64> {
        validate_owner(owner)?;
        crate::nostr_event::validate_hex(query_key, 32)?;
        if start < 0 {
            bail!("invalid relay window start");
        }
        Ok(sqlx::query_scalar::<_, i64>(
            "SELECT scanned_through FROM relay_windows WHERE owner=? AND query_key=?",
        )
        .bind(owner)
        .bind(query_key)
        .fetch_optional(&self.pool)
        .await?
        .unwrap_or(start)
        .max(start))
    }

    pub(crate) async fn advance_relay_window(
        &self,
        owner: &str,
        query_key: &str,
        through: i64,
    ) -> Result<()> {
        validate_owner(owner)?;
        crate::nostr_event::validate_hex(query_key, 32)?;
        if through < 0 {
            bail!("invalid relay window end");
        }
        sqlx::query("INSERT INTO relay_windows(owner,query_key,scanned_through) VALUES(?,?,?) ON CONFLICT(owner,query_key) DO UPDATE SET scanned_through=max(scanned_through,excluded.scanned_through)")
            .bind(owner).bind(query_key).bind(through).execute(&self.pool).await?;
        Ok(())
    }

    /// An explicit operator-selected epoch/cutoff, never inferred from a relay or
    /// silently replaced when a core database/identity changes.
    pub async fn initialize_core_ingress(&self, owner: &str, binding: &CoreBinding) -> Result<()> {
        validate_owner(owner)?;
        binding.validate()?;
        let json = serde_json::to_string(binding)?;
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let existing: Option<String> =
            sqlx::query_scalar("SELECT binding_json FROM core_streams WHERE owner=?")
                .bind(owner)
                .fetch_optional(&mut *tx)
                .await?;
        if let Some(existing) = existing {
            if existing != json {
                bail!("core binding changed; explicit recovery required");
            }
        } else {
            sqlx::query(
                "INSERT INTO core_streams(owner,binding_json,received_through) VALUES(?,?,?)",
            )
            .bind(owner)
            .bind(json)
            .bind(i64::try_from(binding.start_after)?)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    pub async fn core_cursor(&self, owner: &str) -> Result<CoreCursor> {
        validate_owner(owner)?;
        let row =
            sqlx::query("SELECT binding_json,received_through FROM core_streams WHERE owner=?")
                .bind(owner)
                .fetch_one(&self.pool)
                .await?;
        let binding: CoreBinding =
            serde_json::from_str(&row.try_get::<String, _>("binding_json")?)?;
        binding.validate()?;
        Ok(CoreCursor {
            binding,
            received_through: u64::try_from(row.try_get::<i64, _>("received_through")?)?,
        })
    }

    pub async fn ingest_core(&self, owner: &str, page: &AuthenticatedBatch) -> Result<usize> {
        validate_owner(owner)?;
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let row =
            sqlx::query("SELECT binding_json,received_through FROM core_streams WHERE owner=?")
                .bind(owner)
                .fetch_one(&mut *tx)
                .await?;
        let binding: CoreBinding =
            serde_json::from_str(&row.try_get::<String, _>("binding_json")?)?;
        binding.validate()?;
        page.validate(Some(binding.epoch), page.after)?;
        let cursor = u64::try_from(row.try_get::<i64, _>("received_through")?)?;
        if page.core_uid() != binding.core_uid
            || page.after > cursor
            || page.after < binding.start_after
        {
            bail!("core input does not match pinned peer/cursor");
        }
        let epoch = binding.epoch.to_string();
        let mut queued = 0;
        for event in &page.events {
            let envelope_hash = hash(event)?;
            let old: Option<String> = sqlx::query_scalar(
                "SELECT envelope_hash FROM core_envelopes WHERE owner=? AND epoch=? AND core_seq=?",
            )
            .bind(owner)
            .bind(&epoch)
            .bind(i64::try_from(event.seq)?)
            .fetch_optional(&mut *tx)
            .await?;
            if event.seq <= cursor {
                if old.as_deref() != Some(&envelope_hash) {
                    bail!("replayed core event contradicts retained proof");
                }
                continue;
            }
            if old.is_some() {
                bail!("core checkpoint contradicts retained event history");
            }
            let domain = match &event.body {
                EventBody::Payment(payment) => {
                    let (key, proof) = payment_identity(payment)?;
                    Some((key, proof, "payment", serde_json::to_string(payment)?))
                }
                EventBody::Feeding(feeding) => Some((
                    format!("feeding:{}", feeding.request_id),
                    hash(feeding)?,
                    "feeding",
                    serde_json::to_string(feeding)?,
                )),
                // Virtual credit is not a paid invoice or real reward reserve.
                EventBody::OperatorCredit { .. } | EventBody::Other => None,
            };
            if let Some((key, proof, kind, json)) = domain {
                let previous: Option<String> = sqlx::query_scalar(
                    "SELECT proof_hash FROM core_sources WHERE owner=? AND source_key=?",
                )
                .bind(owner)
                .bind(&key)
                .fetch_optional(&mut *tx)
                .await?;
                if let Some(previous) = previous {
                    if previous != proof {
                        bail!("financial source conflicts with its immutable proof");
                    }
                } else {
                    enqueue(&mut tx, owner, &key, kind, &json).await?;
                    sqlx::query(
                        "INSERT INTO core_sources(owner,source_key,proof_hash) VALUES(?,?,?)",
                    )
                    .bind(owner)
                    .bind(key)
                    .bind(proof)
                    .execute(&mut *tx)
                    .await?;
                    queued += 1;
                }
            }
            sqlx::query(
                "INSERT INTO core_envelopes(owner,epoch,core_seq,envelope_hash) VALUES(?,?,?,?)",
            )
            .bind(owner)
            .bind(&epoch)
            .bind(i64::try_from(event.seq)?)
            .bind(envelope_hash)
            .execute(&mut *tx)
            .await?;
        }
        sqlx::query("UPDATE core_streams SET received_through=? WHERE owner=?")
            .bind(i64::try_from(cursor.max(page.through))?)
            .bind(owner)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(queued)
    }

    pub async fn ingress_head(&self, owner: &str) -> Result<Option<Pending>> {
        validate_owner(owner)?;
        let row = sqlx::query("SELECT seq,source_key,kind,payload_json,was_deferred FROM ingress WHERE owner=? AND state='pending' ORDER BY seq LIMIT 1")
            .bind(owner).fetch_optional(&self.pool).await?;
        row.map(|row| {
            let json: String = row.try_get("payload_json")?;
            let input = match row.try_get::<String, _>("kind")?.as_str() {
                "social" => Input::Social(serde_json::from_str(&json)?),
                "payment" => Input::Payment(serde_json::from_str(&json)?),
                "feeding" => Input::Feeding(serde_json::from_str(&json)?),
                _ => bail!("invalid ordered ingress kind"),
            };
            Ok(Pending {
                seq: row.try_get("seq")?,
                source_key: row.try_get("source_key")?,
                input,
                was_deferred: row.try_get("was_deferred")?,
            })
        })
        .transpose()
    }

    pub(crate) async fn defer_ingress(&self, owner: &str, seq: i64) -> Result<()> {
        sqlx::query(
            "UPDATE ingress SET was_deferred=1 WHERE owner=? AND seq=? AND state='pending'",
        )
        .bind(owner)
        .bind(seq)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub(crate) async fn finish_ingress(
        &self,
        owner: &str,
        seq: i64,
        result: &impl serde::Serialize,
    ) -> Result<()> {
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        require_head(&mut tx, owner, seq).await?;
        sqlx::query("UPDATE ingress SET state='done',result_json=? WHERE owner=? AND seq=?")
            .bind(serde_json::to_string(result)?)
            .bind(owner)
            .bind(seq)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(())
    }

    pub(crate) async fn freeze_feeding_barrier(
        &self,
        owner: &str,
        seq: i64,
    ) -> Result<FeedingBarrier> {
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        require_head(&mut tx, owner, seq).await?;
        let row = sqlx::query("SELECT kind,payload_json FROM ingress WHERE owner=? AND seq=?")
            .bind(owner)
            .bind(seq)
            .fetch_one(&mut *tx)
            .await?;
        if row.try_get::<String, _>("kind")? != "feeding" {
            bail!("only a confirmed feed may freeze a membership barrier");
        }
        let feeding_json: String = row.try_get("payload_json")?;
        let feeding: Feeding = serde_json::from_str(&feeding_json)?;
        let row = sqlx::query("SELECT * FROM owners WHERE owner=?")
            .bind(owner)
            .fetch_one(&mut *tx)
            .await?;
        let snapshot = decode_snapshot(&row)?;
        sqlx::query("INSERT INTO feeding_barriers(owner,request_id,ingress_seq,feeding_json,snapshot_json) VALUES(?,?,?,?,?)")
            .bind(owner).bind(feeding.request_id.to_string()).bind(seq).bind(feeding_json).bind(serde_json::to_string(&snapshot)?).execute(&mut *tx).await?;
        sqlx::query("UPDATE ingress SET state='done',result_json='\"membership_barrier\"' WHERE owner=? AND seq=?")
            .bind(owner).bind(seq).execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(FeedingBarrier {
            ingress_seq: seq,
            feeding,
            snapshot,
        })
    }

    pub async fn feeding_barrier(&self, owner: &str, id: Uuid) -> Result<Option<FeedingBarrier>> {
        validate_owner(owner)?;
        let row = sqlx::query("SELECT ingress_seq,feeding_json,snapshot_json FROM feeding_barriers WHERE owner=? AND request_id=?")
            .bind(owner).bind(id.to_string()).fetch_optional(&self.pool).await?;
        row.map(|row| {
            Ok(FeedingBarrier {
                ingress_seq: row.try_get("ingress_seq")?,
                feeding: serde_json::from_str(&row.try_get::<String, _>("feeding_json")?)?,
                snapshot: serde_json::from_str(&row.try_get::<String, _>("snapshot_json")?)?,
            })
        })
        .transpose()
    }
}

async fn require_head(tx: &mut Transaction<'_, Sqlite>, owner: &str, seq: i64) -> Result<()> {
    let head: Option<i64> =
        sqlx::query_scalar("SELECT MIN(seq) FROM ingress WHERE owner=? AND state='pending'")
            .bind(owner)
            .fetch_one(&mut **tx)
            .await?;
    if head.context("no pending ordered work")? != seq {
        bail!("cannot skip prior CyberHerd input");
    }
    Ok(())
}
