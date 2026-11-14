//! Export only independently verified final results, without provider secrets.
use super::{LedgerStore, reward_funding::decode_reservation};
use crate::{
    payout::{
        outcome::{Disposition, Header, Packet, Progress, Row as Outcome},
        results::{PAGE_ROWS, Page, Row, Status},
    },
    reward_funding::Reservation,
};
use anyhow::{Context, Result, bail};
use sqlx::Row as _;

impl LedgerStore {
    pub(crate) async fn reward_results(
        &self,
        reservation: &Reservation,
        after: u16,
    ) -> Result<Option<Page>> {
        reservation.validate()?;
        if after > 1000 {
            bail!("invalid verified result cursor");
        }
        let mut tx = self.pool.begin().await?;
        let epoch: String =
            sqlx::query_scalar("SELECT stream_id FROM overlay_identity WHERE singleton=1")
                .fetch_one(&mut *tx)
                .await?;
        let row = sqlx::query("SELECT * FROM reward_reservations WHERE id=?")
            .bind(reservation.id.to_string())
            .fetch_one(&mut *tx)
            .await?;
        if epoch != reservation.epoch.to_string() || decode_reservation(&row)? != *reservation {
            bail!("verified result reservation/epoch changed");
        }
        let saved = sqlx::query("SELECT m.header_json,f.progress_json FROM reward_outcome_finals f JOIN reward_outcome_manifests m ON m.reservation_id=f.reservation_id WHERE f.reservation_id=?")
            .bind(reservation.id.to_string()).fetch_optional(&mut *tx).await?;
        let Some(saved) = saved else { return Ok(None) };
        let header: Header = serde_json::from_str(&saved.try_get::<String, _>("header_json")?)?;
        let progress: Progress =
            serde_json::from_str(&saved.try_get::<String, _>("progress_json")?)?;
        progress.validate(&header)?;
        if !progress.finalized || header.reservation != *reservation || after > header.rows {
            bail!("verified final result scope changed");
        }
        let records = sqlx::query("SELECT position,member,row_json,before_hash,gross_sats,spent_sats,held_sats,verified_at FROM reward_outcome_rows WHERE reservation_id=? AND position>=? ORDER BY position LIMIT ?")
            .bind(reservation.id.to_string()).bind(i64::from(after)).bind(i64::from(PAGE_ROWS))
            .fetch_all(&mut *tx).await?;
        let mut rows = Vec::new();
        for (offset, record) in records.into_iter().enumerate() {
            let row: Outcome = serde_json::from_str(&record.try_get::<String, _>("row_json")?)?;
            let binding = Packet {
                header: header.clone(),
                after: record.try_get::<i64, _>("position")?.try_into()?,
                before_hash: record.try_get("before_hash")?,
                row: row.clone(),
            }
            .validate()?;
            let spent = u64::try_from(record.try_get::<i64, _>("spent_sats")?)?;
            let held = u64::try_from(record.try_get::<i64, _>("held_sats")?)?;
            if record.try_get::<i64, _>("position")? != i64::from(after) + i64::try_from(offset)?
                || row.member != record.try_get::<String, _>("member")?
                || row.gross_sats != u64::try_from(record.try_get::<i64, _>("gross_sats")?)?
            {
                bail!("verified result row columns changed");
            }
            let result = match &row.disposition {
                Disposition::Payment {
                    state: crate::payout::provider::PaymentState::Completed,
                    fee_sats,
                    total_sats,
                    ..
                } => {
                    if spent != *total_sats || held != 0 {
                        bail!("completed result cost changed");
                    }
                    let binding = binding.context("completed result lost native quote")?;
                    Status::Completed {
                        net_sats: binding.quote.request.amount_sats,
                        fee_sats: *fee_sats,
                    }
                }
                Disposition::Payment {
                    state: crate::payout::provider::PaymentState::Failed,
                    ..
                } => {
                    if spent != 0 || held != row.gross_sats {
                        bail!("failed result hold changed");
                    }
                    Status::Failed
                }
                Disposition::Unsubmitted { .. } => {
                    if spent != 0 || held != 0 {
                        bail!("unsubmitted result cost changed");
                    }
                    Status::Unsubmitted
                }
                _ => bail!("pending result cannot be final"),
            };
            rows.push(Row {
                member: row.member,
                gross_sats: row.gross_sats,
                verified_at: record.try_get("verified_at")?,
                result,
            });
        }
        let page = Page {
            reservation: reservation.clone(),
            manifest_hash: header.final_hash,
            after,
            through: after
                .checked_add(rows.len().try_into()?)
                .context("result cursor overflow")?,
            total_rows: header.rows,
            rows,
        };
        page.validate()?;
        tx.commit().await?;
        Ok(Some(page))
    }
}
