use super::{LedgerStore, append_event_in_transaction, to_u64};
use crate::{
    config::RuntimeMode,
    core_bridge::EventBody,
    cyberherd::feeding_delivery::{Overlay, Proof},
};
use anyhow::{Context as _, Result, bail};
use serde_json::Value;
use sqlx::{Row as _, Sqlite, Transaction};
use uuid::Uuid;

impl LedgerStore {
    /// Startup-only root configuration. Existing events retain their frozen
    /// choice; disabling affects future confirmations, never historical bytes.
    pub async fn configure_feeding_notices(&self, owner: Option<&str>) -> Result<()> {
        if let Some(owner) = owner {
            crate::nostr_event::validate_public_key(owner)?;
        }
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        sqlx::query("DELETE FROM feeding_notice_policy")
            .execute(&mut *tx)
            .await?;
        if let Some(owner) = owner {
            sqlx::query("INSERT INTO feeding_notice_policy(singleton,owner) VALUES(1,?)")
                .bind(owner)
                .execute(&mut *tx)
                .await?;
        }
        tx.commit().await?;
        Ok(())
    }
    pub(crate) async fn accept_feeding_overlay(
        &self,
        message: &Overlay,
        mode: RuntimeMode,
    ) -> Result<u64> {
        message.validate()?;
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let epoch: String =
            sqlx::query_scalar("SELECT stream_id FROM overlay_identity WHERE singleton=1")
                .fetch_one(&mut *tx)
                .await?;
        if epoch != message.epoch.to_string() {
            bail!("feeding overlay belongs to another core epoch");
        }
        let document = serde_json::to_string(message)?;
        if let Some(row)=sqlx::query("SELECT document_json,event_seq FROM feeding_overlay_receipts WHERE epoch=? AND owner=? AND identity=?")
            .bind(&epoch).bind(&message.broadcast.proof.owner).bind(&message.broadcast.proof.identity).fetch_optional(&mut *tx).await? {
            if row.try_get::<String,_>("document_json")?!=document {bail!("feeding overlay retry changed frozen message");}
            let seq=to_u64(row.try_get("event_seq")?,"feeding overlay receipt")?;
            tx.commit().await?;return Ok(seq);
        }
        if !mode.feeder_enabled() {
            bail!("feeding presentation is disabled for runtime mode");
        }
        let source = &message.broadcast.proof;
        let row=sqlx::query("SELECT event_type,payload_json FROM event_log WHERE seq=? AND length(payload_json)<=65536")
            .bind(i64::try_from(source.source_seq)?).fetch_one(&mut *tx).await?;
        let kind: String = row.try_get("event_type")?;
        let raw: String = row.try_get("payload_json")?;
        let EventBody::Feeding(feed) =
            super::core_bridge::export_body(&mut tx, source.source_seq, &kind, Some(&raw)).await?
        else {
            bail!("overlay source is not an actual confirmed feeding");
        };
        if feed.request_id != message.broadcast.request_id
            || feed.presentation.as_ref() != Some(source)
        {
            bail!("feeding overlay changed authoritative proof");
        }
        let seq=append_event_in_transaction(&mut tx,"cyberherd_message",&serde_json::json!({"message":message.message,"category":"feeder_trigger","message_id":source.identity,"goats":message.broadcast.goats})).await?;
        sqlx::query("INSERT INTO feeding_overlay_receipts(epoch,owner,identity,document_json,event_seq) VALUES(?,?,?,?,?)")
            .bind(epoch).bind(&source.owner).bind(&source.identity).bind(document).bind(i64::try_from(seq)?).execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(seq)
    }
}

/// Reconstruct from the actual immutable source/debit and its prefix balance,
/// never from an arbitrary public event named feeder_confirmed.
pub(super) async fn export_proof(
    tx: &mut Transaction<'_, Sqlite>,
    seq: u64,
    id: Uuid,
    debit: i64,
    value: &Value,
) -> Result<Option<Proof>> {
    if value.get("announce_feeding").and_then(Value::as_bool) == Some(false) {
        return Ok(None);
    }
    let Some(owner) = value.get("presentation_owner").and_then(Value::as_str) else {
        return Ok(None);
    };
    crate::nostr_event::validate_public_key(owner)?;
    let credit = value["feed_credit_sats"]
        .as_u64()
        .context("feeding presentation lacks saved balance")?;
    let actual: i64 =
        sqlx::query_scalar("SELECT COALESCE(SUM(delta_sats),0) FROM ledger_entries WHERE id<=?")
            .bind(debit)
            .fetch_one(&mut **tx)
            .await?;
    if to_u64(actual, "post-feeding balance")? != credit {
        bail!("feeding presentation balance contradicts debit");
    }
    let captured = match (
        value.get("triggering_credit_id"),
        value.get("triggering_payment_amount_sats"),
    ) {
        (None, None) => None,
        (Some(id), Some(amount)) => Some((
            id.as_i64().context("invalid triggering credit id")?,
            amount
                .as_u64()
                .context("invalid triggering credit amount")?,
        )),
        _ => bail!("feeding presentation lost triggering credit identity"),
    };
    let actual = super::feed::triggering_credit(tx, debit, credit).await?;
    if captured != actual {
        bail!("feeding presentation attribution contradicts consumed credit");
    }
    let key = if let Some((credit_id, _)) = actual {
        sqlx::query_scalar::<_, String>(
            "SELECT source_key FROM ledger_entries WHERE id=? AND delta_sats>0",
        )
        .bind(credit_id)
        .fetch_one(&mut **tx)
        .await?
    } else {
        format!("unattributed-feed:{id}")
    };
    let epoch: String =
        sqlx::query_scalar("SELECT stream_id FROM overlay_identity WHERE singleton=1")
            .fetch_one(&mut **tx)
            .await?;
    let proof = Proof {
        owner: owner.into(),
        identity: crate::cyberherd::ingress::hash(&(&epoch, owner, "feeding-announcement", key))?,
        source_seq: seq,
        triggering_payment_amount_sats: actual.map(|(_, amount)| amount),
        feed_credit_sats: credit,
    };
    proof.validate()?;
    Ok(Some(proof))
}
