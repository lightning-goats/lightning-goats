//! The payout UID reports its protected journal; it cannot provide a financial
//! verification capability. Core reads payment IDs with its own read-only key.
use anyhow::{Context, Result, bail};
use sqlx::{Row, Sqlite, Transaction};

use super::{LedgerStore, reward_funding::decode_reservation, to_i64, to_u64};
use crate::payout::outcome::{Header, Packet, Progress, Reader, next_hash};

#[cfg(test)]
#[path = "reward_outcomes_tests.rs"]
mod tests;

impl LedgerStore {
    /// Read-only progress is available even after execution/policy is disabled.
    pub(crate) async fn payout_outcome_progress(&self, header: &Header) -> Result<Progress> {
        let mut tx = self.pool.begin().await?;
        validate_scope(&mut tx, header).await?;
        let value = progress(&mut tx, header).await?;
        tx.commit().await?;
        Ok(value)
    }

    pub(crate) async fn reconcile_payout_outcome(
        &self,
        packet: &Packet,
        reader: &Reader,
    ) -> Result<Progress> {
        packet.validate()?;
        // No network under a database transaction. Reject changed/foreign
        // reports and return exact retries before consuming provider capacity.
        let mut tx = self.pool.begin().await?;
        validate_scope(&mut tx, &packet.header).await?;
        if let Some(done) = existing_packet(&mut tx, packet).await? {
            tx.commit().await?;
            return Ok(done);
        }
        validate_cursor(&mut tx, packet).await?;
        super::payout_execution::validate_outcome(&mut tx, packet).await?;
        tx.commit().await?;
        let verified = reader.verify(packet).await?;
        let packet = verified.packet();
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        validate_scope(&mut tx, &packet.header).await?;
        if let Some(done) = existing_packet(&mut tx, packet).await? {
            tx.commit().await?;
            return Ok(done);
        }
        validate_cursor(&mut tx, packet).await?;
        super::payout_execution::validate_outcome(&mut tx, packet).await?;
        let now = budget_time(&mut tx, true).await?;
        if now < packet.header.reservation.created_at {
            bail!("outcome verification predates reservation");
        }
        sqlx::query("INSERT INTO reward_outcome_manifests(reservation_id,header_json) VALUES(?,?) ON CONFLICT(reservation_id) DO NOTHING")
            .bind(packet.header.reservation.id.to_string()).bind(serde_json::to_string(&packet.header)?).execute(&mut *tx).await?;
        sqlx::query("INSERT INTO reward_outcome_rows(reservation_id,position,member,row_json,before_hash,chain_hash,gross_sats,spent_sats,held_sats,verified_at,payment_id,invoice_hash) VALUES(?,?,?,?,?,?,?,?,?,?,?,?)")
            .bind(packet.header.reservation.id.to_string()).bind(i64::from(packet.after)).bind(&packet.row.member)
            .bind(serde_json::to_string(&packet.row)?).bind(&packet.before_hash).bind(next_hash(&packet.before_hash,&packet.row)?)
            .bind(to_i64(packet.row.gross_sats,"outcome gross")?).bind(to_i64(verified.spent(),"outcome spend")?)
            .bind(to_i64(verified.held(),"outcome hold")?).bind(now)
            .bind(verified.payment_id().map(|id|id.to_string())).bind(verified.invoice_hash()).execute(&mut *tx).await?;
        let mut result = progress(&mut tx, &packet.header).await?;
        let gross: i64 = sqlx::query_scalar(
            "SELECT COALESCE(SUM(gross_sats),0) FROM reward_outcome_rows WHERE reservation_id=?",
        )
        .bind(packet.header.reservation.id.to_string())
        .fetch_one(&mut *tx)
        .await?;
        if to_u64(gross, "outcome gross total")? > packet.header.reservation.reserved_sats {
            bail!("manifest exceeds reservation");
        }
        if result.through == packet.header.rows {
            let missing:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM payout_execution_claims c WHERE c.reservation_id=? AND NOT EXISTS(SELECT 1 FROM reward_outcome_rows r WHERE r.reservation_id=c.reservation_id AND r.member=c.member))")
                .bind(packet.header.reservation.id.to_string()).fetch_one(&mut *tx).await?;
            if missing {
                bail!("manifest omits a core-retained possible execution");
            }
            if result.chain_hash != packet.header.final_hash
                || to_u64(gross, "outcome gross total")? != packet.header.reservation.reserved_sats
            {
                bail!("final outcome hash/count/gross disagrees with reservation");
            }
            result.finalized = true;
            result.released_sats = packet
                .header
                .reservation
                .reserved_sats
                .checked_sub(result.spent_sats)
                .and_then(|value| value.checked_sub(result.held_sats))
                .context("outcome accounting overflow")?;
            result.validate(&packet.header)?;
            sqlx::query("INSERT INTO reward_outcome_finals(reservation_id,progress_json,held_sats) VALUES(?,?,?)")
                .bind(packet.header.reservation.id.to_string()).bind(serde_json::to_string(&result)?)
                .bind(to_i64(result.held_sats,"final hold")?).execute(&mut *tx).await?;
        }
        tx.commit().await?;
        Ok(result)
    }
}

pub(super) async fn validate_scope(
    tx: &mut Transaction<'_, Sqlite>,
    header: &Header,
) -> Result<()> {
    header.validate()?;
    let epoch: String =
        sqlx::query_scalar("SELECT stream_id FROM overlay_identity WHERE singleton=1")
            .fetch_one(&mut **tx)
            .await?;
    let row = sqlx::query("SELECT * FROM reward_reservations WHERE id=?")
        .bind(header.reservation.id.to_string())
        .fetch_one(&mut **tx)
        .await?;
    if epoch != header.reservation.epoch.to_string()
        || decode_reservation(&row)? != header.reservation
    {
        bail!("outcome does not match current core reservation");
    }
    let funding = super::reward_funding::confirmed_funding(tx, header.reservation.request_id)
        .await?
        .context("outcome reservation lost confirmed funding")?;
    if funding.proof_hash != header.reservation.funding_hash
        || header.reservation.reserved_sats > funding.strike_sats
    {
        bail!("outcome reservation funding changed");
    }
    if let Some(saved) = sqlx::query_scalar::<_, String>(
        "SELECT header_json FROM reward_outcome_manifests WHERE reservation_id=?",
    )
    .bind(header.reservation.id.to_string())
    .fetch_optional(&mut **tx)
    .await?
    {
        if saved != serde_json::to_string(header)? {
            bail!("outcome manifest changed");
        }
    }
    Ok(())
}

async fn existing_packet(
    tx: &mut Transaction<'_, Sqlite>,
    packet: &Packet,
) -> Result<Option<Progress>> {
    if let Some(row)=sqlx::query("SELECT row_json,before_hash FROM reward_outcome_rows WHERE reservation_id=? AND position=?")
        .bind(packet.header.reservation.id.to_string()).bind(i64::from(packet.after)).fetch_optional(&mut **tx).await? {
        if row.try_get::<String,_>("row_json")?!=serde_json::to_string(&packet.row)? || row.try_get::<String,_>("before_hash")?!=packet.before_hash {
            bail!("outcome retry changed verified row");
        }
        return Ok(Some(progress(tx,&packet.header).await?));
    }
    Ok(None)
}

async fn validate_cursor(tx: &mut Transaction<'_, Sqlite>, packet: &Packet) -> Result<()> {
    let old = progress(tx, &packet.header).await?;
    if old.finalized || old.through != packet.after || old.chain_hash != packet.before_hash {
        bail!("outcome report skips or changes verified cursor");
    }
    let last:Option<String>=sqlx::query_scalar("SELECT member FROM reward_outcome_rows WHERE reservation_id=? ORDER BY position DESC LIMIT 1")
        .bind(packet.header.reservation.id.to_string()).fetch_optional(&mut **tx).await?;
    if last.is_some_and(|last| last >= packet.row.member) {
        bail!("outcome members must be strictly sorted");
    }
    Ok(())
}

async fn progress(tx: &mut Transaction<'_, Sqlite>, header: &Header) -> Result<Progress> {
    let id = header.reservation.id.to_string();
    if let Some(saved) = sqlx::query_scalar::<_, String>(
        "SELECT progress_json FROM reward_outcome_finals WHERE reservation_id=?",
    )
    .bind(&id)
    .fetch_optional(&mut **tx)
    .await?
    {
        let value: Progress = serde_json::from_str(&saved)?;
        value.validate(header)?;
        return Ok(value);
    }
    let sums=sqlx::query("SELECT COUNT(*) AS count,COALESCE(SUM(spent_sats),0) AS spent,COALESCE(SUM(held_sats),0) AS held,MAX(verified_at) AS at FROM reward_outcome_rows WHERE reservation_id=?")
        .bind(&id).fetch_one(&mut **tx).await?;
    let last:Option<String>=sqlx::query_scalar("SELECT chain_hash FROM reward_outcome_rows WHERE reservation_id=? ORDER BY position DESC LIMIT 1")
        .bind(&id).fetch_optional(&mut **tx).await?;
    let value = Progress {
        reservation_id: header.reservation.id,
        epoch: header.reservation.epoch,
        manifest_hash: header.final_hash.clone(),
        through: sums.try_get::<i64, _>("count")?.try_into()?,
        chain_hash: last.unwrap_or(header.seed()?),
        finalized: false,
        spent_sats: to_u64(sums.try_get("spent")?, "verified spend")?,
        held_sats: to_u64(sums.try_get("held")?, "verified hold")?,
        released_sats: 0,
        verified_at: sums
            .try_get::<Option<i64>, _>("at")?
            .unwrap_or(header.reservation.created_at),
    };
    value.validate(header)?;
    Ok(value)
}

/// Held/unknown reservations never age out. Only completed, core-verified
/// spending enters the rolling window; failed/unproved costs remain held.
pub(super) async fn budget_usage(tx: &mut Transaction<'_, Sqlite>, now: i64) -> Result<u64> {
    let used:i64=sqlx::query_scalar("SELECT COALESCE((SELECT SUM(r.reserved_sats) FROM reward_reservations r WHERE NOT EXISTS(SELECT 1 FROM reward_outcome_finals f WHERE f.reservation_id=r.id)),0)+COALESCE((SELECT SUM(held_sats) FROM reward_outcome_finals),0)-COALESCE((SELECT SUM(gross_sats) FROM payout_failed_costs),0)+COALESCE((SELECT SUM(o.spent_sats) FROM reward_outcome_rows o JOIN reward_outcome_finals f ON f.reservation_id=o.reservation_id WHERE o.verified_at>?),0)+COALESCE((SELECT SUM(spent_sats) FROM payout_failed_costs WHERE verified_at>?),0)")
        .bind(now.saturating_sub(86400)).bind(now.saturating_sub(86400)).fetch_one(&mut **tx).await?;
    to_u64(used, "held and recent reward spending")
}
pub(super) async fn budget_time(tx: &mut Transaction<'_, Sqlite>, touch: bool) -> Result<i64> {
    let now: i64 = sqlx::query_scalar("SELECT unixepoch()")
        .fetch_one(&mut **tx)
        .await?;
    let previous: i64 =
        sqlx::query_scalar("SELECT observed_at FROM reward_budget_clock WHERE singleton=1")
            .fetch_one(&mut **tx)
            .await?;
    if now < previous {
        bail!("core reward clock moved backwards");
    }
    if touch {
        sqlx::query("UPDATE reward_budget_clock SET observed_at=? WHERE singleton=1")
            .bind(now)
            .execute(&mut **tx)
            .await?;
    }
    Ok(now)
}
