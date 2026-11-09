use anyhow::{Context, Result, bail};
use serde_json::Value;
use sqlx::{Row, Sqlite, Transaction};
use uuid::Uuid;

use super::LedgerStore;
use crate::{domain::payment::SettledPayment, nostr_event::VerifiedEvent, zap::ZapRequest};

#[derive(Debug, Clone)]
pub struct ZapReceiptJob {
    pub settlement_id: i64,
    pub lease_id: Uuid,
    pub source_event_seq: u64,
    pub paid_at: u64,
    pub invoice: String,
    pub request: ZapRequest,
    pub signed_json: Option<String>,
    pub relay: String,
}

pub(super) async fn stage_receipt(
    tx: &mut Transaction<'_, Sqlite>,
    payment: &SettledPayment,
    settlement_id: i64,
    event_seq: u64,
) -> Result<()> {
    if payment.source != "strike" {
        return Ok(());
    }
    let Some(context) = payment.context_json.as_ref() else {
        return Ok(());
    };
    let context: Value = serde_json::from_str(context)?;
    let Some(id) = context["receive_request_id"].as_str() else {
        return Ok(());
    };
    let row=sqlx::query("SELECT z.request_json,z.payee_pubkey,z.receipt_pubkey,r.amount_msat,r.address_user,r.credit_pool,r.description_hash FROM zap_receive_context z JOIN strike_receive_requests r USING(receive_request_id) WHERE z.receive_request_id=?")
        .bind(id).fetch_optional(&mut **tx).await?;
    let Some(row) = row else {
        return Ok(());
    };
    if row.try_get::<String, _>("address_user")? != payment.address_user
        || row.try_get::<String, _>("credit_pool")? != payment.credit_pool
    {
        bail!("zap settlement provenance contradicts the issued address");
    }
    let issued_amount = u64::try_from(row.try_get::<i64, _>("amount_msat")?)?;
    // Strike can optimize fiat/P2P receives. Credit its actual authoritative BTC
    // amount normally; never attest that a different invoice amount was paid.
    if issued_amount != payment.amount_msat {
        return Ok(());
    }
    let observed: i64 = sqlx::query_scalar("SELECT received_at FROM settled_payments WHERE id=?")
        .bind(settlement_id)
        .fetch_one(&mut **tx)
        .await?;
    let provider_time = context["completed_provider"]
        .as_str()
        .and_then(|value| chrono::DateTime::parse_from_rfc3339(value).ok())
        .map(|value| value.timestamp())
        .filter(|value| *value >= 0 && *value <= observed.saturating_add(300));
    let paid_at = payment.settled_at.or(provider_time).unwrap_or(observed);
    sqlx::query("INSERT INTO zap_receipt_jobs(settlement_id,source_event_seq,receive_request_id,paid_at) VALUES(?,?,?,?)")
        .bind(settlement_id).bind(i64::try_from(event_seq)?).bind(id).bind(paid_at).execute(&mut **tx).await?;
    Ok(())
}

fn decode_request(row: &sqlx::sqlite::SqliteRow) -> Result<ZapRequest> {
    let request = ZapRequest::validate(
        &row.try_get::<String, _>("request_json")?,
        &row.try_get::<String, _>("payee_pubkey")?,
        &row.try_get::<String, _>("receipt_pubkey")?,
        u64::try_from(row.try_get::<i64, _>("amount_msat")?)?,
    )?;
    if request.description_hash() != row.try_get::<String, _>("description_hash")? {
        bail!("persisted zap description hash contradicts its exact request bytes");
    }
    Ok(request)
}

impl LedgerStore {
    pub async fn zap_request(&self, id: Uuid) -> Result<Option<ZapRequest>> {
        let row=sqlx::query("SELECT z.request_json,z.payee_pubkey,z.receipt_pubkey,r.amount_msat,r.description_hash FROM zap_receive_context z JOIN strike_receive_requests r USING(receive_request_id) WHERE z.receive_request_id=?")
            .bind(id.to_string()).fetch_optional(&self.pool).await?;
        row.as_ref().map(decode_request).transpose()
    }

    pub async fn reserve_zap_receipt(&self) -> Result<Option<ZapReceiptJob>> {
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let row=sqlx::query("SELECT j.settlement_id,j.source_event_seq,j.paid_at,j.signed_json,r.invoice,z.request_json,z.payee_pubkey,z.receipt_pubkey,r.amount_msat,r.description_hash,d.relay FROM zap_receipt_jobs j JOIN zap_receive_context z USING(receive_request_id) JOIN strike_receive_requests r USING(receive_request_id) LEFT JOIN zap_receipt_relays d ON d.settlement_id=j.settlement_id AND d.published=0 AND d.next_attempt<=unixepoch() WHERE j.next_attempt<=unixepoch() AND (j.lease_until IS NULL OR j.lease_until<unixepoch()) AND (d.relay IS NOT NULL OR NOT EXISTS(SELECT 1 FROM zap_receipt_relays WHERE settlement_id=j.settlement_id)) ORDER BY j.source_event_seq,d.next_attempt,d.relay LIMIT 1")
            .fetch_optional(&mut *tx).await?;
        let Some(row) = row else {
            tx.commit().await?;
            return Ok(None);
        };
        let lease_id = Uuid::new_v4();
        let settlement_id = row.try_get("settlement_id")?;
        let request = decode_request(&row)?;
        let relay = if let Some(relay) = row.try_get::<Option<String>, _>("relay")? {
            relay
        } else {
            for relay in request.receipt_relays() {
                sqlx::query("INSERT INTO zap_receipt_relays(settlement_id,relay) VALUES(?,?)")
                    .bind(settlement_id)
                    .bind(relay)
                    .execute(&mut *tx)
                    .await?;
            }
            request
                .receipt_relays()
                .first()
                .context("zap receipt has no destination")?
                .clone()
        };
        sqlx::query("UPDATE zap_receipt_jobs SET lease_id=?,lease_until=unixepoch()+120 WHERE settlement_id=?").bind(lease_id.to_string()).bind(settlement_id).execute(&mut *tx).await?;
        let job = ZapReceiptJob {
            settlement_id,
            lease_id,
            source_event_seq: u64::try_from(row.try_get::<i64, _>("source_event_seq")?)?,
            paid_at: u64::try_from(row.try_get::<i64, _>("paid_at")?)?,
            invoice: row.try_get("invoice")?,
            request,
            signed_json: row.try_get("signed_json")?,
            relay,
        };
        tx.commit().await?;
        Ok(Some(job))
    }

    pub async fn save_signed_zap_receipt(
        &self,
        job: &ZapReceiptJob,
        event: &VerifiedEvent,
    ) -> Result<String> {
        let signed = event.event();
        if signed.kind != 9735
            || signed.pubkey != job.request.receipt_signer()
            || signed.created_at != job.paid_at
            || !signed.content.is_empty()
            || signed.tags != job.request.receipt_tags(&job.invoice)?
        {
            bail!("signed zap receipt does not match the frozen settled-payment intent");
        }
        let json = serde_json::to_string(signed)?;
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let current=sqlx::query("SELECT j.signed_json,j.lease_id,j.paid_at,r.invoice,z.request_json,z.payee_pubkey,z.receipt_pubkey,r.amount_msat,r.description_hash FROM zap_receipt_jobs j JOIN zap_receive_context z USING(receive_request_id) JOIN strike_receive_requests r USING(receive_request_id) WHERE j.settlement_id=?").bind(job.settlement_id).fetch_one(&mut *tx).await?;
        let original = decode_request(&current)?;
        if signed.created_at != u64::try_from(current.try_get::<i64, _>("paid_at")?)?
            || signed.pubkey != original.receipt_signer()
            || signed.tags != original.receipt_tags(&current.try_get::<String, _>("invoice")?)?
        {
            bail!("signed zap receipt contradicts the durable source intent");
        }
        if current.try_get::<Option<String>, _>("lease_id")?.as_deref()
            != Some(job.lease_id.to_string().as_str())
        {
            bail!("zap receipt lease is no longer owned");
        }
        let stored = if let Some(existing) = current.try_get::<Option<String>, _>("signed_json")? {
            existing
        } else {
            let result=sqlx::query("UPDATE zap_receipt_jobs SET event_id=?,signed_json=?,lease_until=unixepoch()+120 WHERE settlement_id=? AND lease_id=? AND lease_until>=unixepoch()")
                .bind(&signed.id).bind(&json).bind(job.settlement_id).bind(job.lease_id.to_string()).execute(&mut *tx).await?;
            if result.rows_affected() != 1 {
                bail!("zap receipt lease expired before signature persistence");
            }
            json
        };
        tx.commit().await?;
        Ok(stored)
    }

    pub async fn finish_zap_receipt_step(
        &self,
        job: &ZapReceiptJob,
        published: bool,
        signing_failed: bool,
    ) -> Result<()> {
        if published && signing_failed {
            bail!("contradictory zap receipt completion");
        }
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let owned:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM zap_receipt_jobs WHERE settlement_id=? AND lease_id=? AND lease_until>=unixepoch())").bind(job.settlement_id).bind(job.lease_id.to_string()).fetch_one(&mut *tx).await?;
        if !owned {
            bail!("zap receipt lease is no longer owned");
        }
        if published {
            let signed: bool = sqlx::query_scalar(
                "SELECT event_id IS NOT NULL FROM zap_receipt_jobs WHERE settlement_id=?",
            )
            .bind(job.settlement_id)
            .fetch_one(&mut *tx)
            .await?;
            if !signed {
                bail!("unsigned zap receipt cannot be marked published");
            }
        }
        if signing_failed {
            sqlx::query("UPDATE zap_receipt_jobs SET signing_attempts=signing_attempts+1,next_attempt=unixepoch()+min(3600,30*(1<<min(signing_attempts,6))) WHERE settlement_id=?").bind(job.settlement_id).execute(&mut *tx).await?;
        } else {
            let updated=sqlx::query("UPDATE zap_receipt_relays SET published=?,attempts=attempts+1,next_attempt=unixepoch()+min(3600,30*(1<<min(attempts,6))) WHERE settlement_id=? AND relay=?")
                .bind(i64::from(published)).bind(job.settlement_id).bind(&job.relay).execute(&mut *tx).await?;
            if updated.rows_affected() != 1 {
                bail!("zap receipt destination differs from the durable intent");
            }
        }
        let released=sqlx::query("UPDATE zap_receipt_jobs SET lease_id=NULL,lease_until=NULL WHERE settlement_id=? AND lease_id=?").bind(job.settlement_id).bind(job.lease_id.to_string()).execute(&mut *tx).await?;
        if released.rows_affected() != 1 {
            bail!("zap receipt lease release lost ownership");
        }
        tx.commit().await?;
        Ok(())
    }
}
