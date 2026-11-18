//! Private journal accounting, not a live mint balance or payment authority.
use crate::{journal::custody::Custody, service_config::Config};
use anyhow::{Result, ensure};
use rusqlite::{Connection, params};
use serde::Serialize;
use std::collections::BTreeSet;
use uuid::Uuid;

#[derive(Serialize)]
pub struct Health {
    pub observed_at: i64,
    pub expected_core_epoch: Option<Uuid>,
    pub retained_acknowledgments_other_core_epochs: Option<u64>,
    pub malformed_retained_acknowledgments: u64,
    pub totals: Funds,
    pub queue: Queue,
    pub mints: Vec<MintFunds>,
    pub mints_truncated: bool,
}
#[derive(Serialize)]
pub struct Funds {
    pub unverified_input_sats: u64,
    pub acquired_net_sats: u64,
    pub journal_custody_sats: u64,
    pub pending_input_reserved_sats: u64,
    pub ambiguous_submitted_input_sats: u64,
    pub reported_treasury_deposit_sats: u64,
    pub receive_fee_sats: u64,
    pub completed_conversion_fee_sats: u64,
    pub split_preparation_fee_sats: u64,
    pub unsettled_preparation_fee_sats: u64,
    pub total_accounted_fee_sats: u64,
    pub operations_admitted_last_24h: u64,
    pub input_admitted_last_24h_sats: u64,
}
#[derive(Serialize)]
pub struct MintFunds {
    pub mint_id: String,
    pub configured: bool,
    pub funds: Funds,
}
#[derive(Serialize)]
pub struct Queue {
    pub pending_receipts: u64,
    pub pending_age_unknown: u64,
    pub future_admission_timestamps: u64,
    pub oldest_pending_admitted_at: Option<i64>,
    pub oldest_pending_age_seconds: Option<u64>,
    pub oldest_scheduled_due_at: Option<i64>,
    pub scheduled_lag_seconds: Option<u64>,
}

impl Funds {
    pub(crate) fn read(db: &Connection, mint: Option<&str>, now: i64) -> Result<Self> {
        let custody = Custody::read(db, mint)?;
        let reported_treasury_deposit_sats: u64 = db.query_row("SELECT COALESCE(SUM(json_extract(i.issued_json,'$.intent.deposit_sats')),0) FROM treasury_instructions i JOIN treasury_results r ON r.intent_id=i.id WHERE ?1 IS NULL OR json_extract(i.issued_json,'$.intent.mint_id')=?1",[mint],|r|r.get(0))?;
        let (pending_input_reserved_sats, ambiguous_submitted_input_sats): (u64,u64) = db.query_row(
            "SELECT COALESCE(SUM(h.input_sats),0),COALESCE(SUM(CASE WHEN i.state='submitted' THEN h.input_sats ELSE 0 END),0) FROM treasury_source_holds h JOIN treasury_instructions i ON i.id=h.intent_id JOIN acquisitions a ON a.id=h.acquisition_id WHERE i.state!='reconciled' AND (?1 IS NULL OR a.mint_id=?1)",
            [mint], |r|Ok((r.get(0)?,r.get(1)?)))?;
        let (operations_admitted_last_24h, input_admitted_last_24h_sats): (u64,u64) = db.query_row(
            "SELECT COUNT(*),COALESCE(SUM(a.input_sats),0) FROM treasury_execution_admissions a JOIN treasury_instructions i ON i.id=a.intent_id WHERE a.admitted_at>?2 AND a.admitted_at<=?3 AND (?1 IS NULL OR json_extract(i.issued_json,'$.intent.mint_id')=?1)",
            params![mint,now.saturating_sub(86400),now], |r|Ok((r.get(0)?,r.get(1)?)))?;
        let completed_conversion_fee_sats = custody
            .consumed_sats
            .checked_sub(reported_treasury_deposit_sats)
            .ok_or_else(|| anyhow::anyhow!("reported conversion conservation mismatch"))?;
        let total_accounted_fee_sats = custody
            .receive_fee_sats
            .checked_add(completed_conversion_fee_sats)
            .and_then(|v| v.checked_add(custody.unsettled_preparation_fee_sats))
            .ok_or_else(|| anyhow::anyhow!("fee health overflow"))?;
        Ok(Self {
            unverified_input_sats: custody.unverified_input_sats,
            acquired_net_sats: custody.acquired_net_sats,
            journal_custody_sats: custody.retained_sats,
            pending_input_reserved_sats,
            ambiguous_submitted_input_sats,
            reported_treasury_deposit_sats,
            receive_fee_sats: custody.receive_fee_sats,
            completed_conversion_fee_sats,
            split_preparation_fee_sats: custody.split_preparation_fee_sats,
            unsettled_preparation_fee_sats: custody.unsettled_preparation_fee_sats,
            total_accounted_fee_sats,
            operations_admitted_last_24h,
            input_admitted_last_24h_sats,
        })
    }
}

impl Health {
    pub(super) fn read(db: &Connection, config: &Config, now: i64) -> Result<Self> {
        let expected_core_epoch = config
            .receipt_delivery
            .as_ref()
            .map(|c| c.core_epoch)
            .or_else(|| config.treasury_intake.as_ref().map(|c| c.core_epoch));
        let malformed_retained_acknowledgments = db.query_row("SELECT COUNT(*) FROM acquisitions WHERE core_ack IS NOT NULL AND NOT json_valid(core_ack)",[],|r|r.get(0))?;
        let retained_acknowledgments_other_core_epochs = expected_core_epoch.map(|epoch| db.query_row(
            "SELECT COUNT(*) FROM acquisitions WHERE core_ack IS NOT NULL AND CASE WHEN json_valid(core_ack) THEN json_extract(core_ack,'$.core_epoch') IS NOT ? ELSE 0 END",
            [epoch.to_string()],|r|r.get::<_,u64>(0))).transpose()?;
        let totals = Funds::read(db, None, now)?;
        let mut ids = config
            .mints
            .iter()
            .map(|m| m.id.clone())
            .collect::<BTreeSet<_>>();
        let mut query = db.prepare("SELECT mint_id FROM acquisitions UNION SELECT json_extract(issued_json,'$.intent.mint_id') FROM treasury_instructions ORDER BY 1 LIMIT 65")?;
        for id in query.query_map([], |r| r.get::<_, String>(0))? {
            ids.insert(id?);
        }
        let mints_truncated = ids.len() > 64;
        let mut mints = Vec::new();
        for mint_id in ids.into_iter().take(64) {
            mints.push(MintFunds {
                configured: config.mints.iter().any(|m| m.id == mint_id),
                funds: Funds::read(db, Some(&mint_id), now)?,
                mint_id,
            });
        }
        let has_times: bool = db.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='acquisition_admission_times')",[],|r|r.get(0))?;
        let pending_receipts: u64 = db.query_row(
            "SELECT COUNT(*) FROM acquisitions WHERE state!='credited'",
            [],
            |r| r.get(0),
        )?;
        let (pending_age_unknown, future_admission_timestamps, oldest_pending_admitted_at): (
            u64,
            u64,
            Option<i64>,
        ) = if has_times {
            db.query_row("SELECT COALESCE(SUM(t.admitted_at IS NULL),0),COALESCE(SUM(t.admitted_at>?1),0),MIN(CASE WHEN t.admitted_at<=?1 THEN t.admitted_at END) FROM acquisitions a LEFT JOIN acquisition_admission_times t ON t.acquisition_id=a.id WHERE a.state!='credited'",[now],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?)))?
        } else {
            (pending_receipts, 0, None)
        };
        let oldest_scheduled_due_at: Option<i64> = db.query_row(
            "SELECT MIN(due) FROM (SELECT s.next_attempt due FROM acquisition_schedule s JOIN acquisitions a ON a.id=s.acquisition_id WHERE a.state IN ('validated','acquisition_prepared','acquiring') UNION ALL SELECT s.next_attempt FROM receipt_delivery_schedule s JOIN acquisitions a ON a.id=s.acquisition_id WHERE a.state IN ('acquired','credit_export_pending') UNION ALL SELECT s.next_attempt FROM treasury_work_schedule s JOIN treasury_instructions i ON i.id=s.intent_id WHERE i.state!='reconciled') WHERE due<=?",[now],|r|r.get(0))?;
        let age = |timestamp: Option<i64>| -> Result<Option<u64>> {
            timestamp
                .map(|t| {
                    ensure!(t >= 0 && t <= now, "invalid retained health timestamp");
                    Ok(u64::try_from(now - t)?)
                })
                .transpose()
        };
        let queue = Queue {
            pending_receipts,
            pending_age_unknown,
            future_admission_timestamps,
            oldest_pending_admitted_at,
            oldest_pending_age_seconds: age(oldest_pending_admitted_at)?,
            oldest_scheduled_due_at,
            scheduled_lag_seconds: age(oldest_scheduled_due_at)?,
        };
        Ok(Self {
            observed_at: now,
            expected_core_epoch,
            retained_acknowledgments_other_core_epochs,
            malformed_retained_acknowledgments,
            totals,
            queue,
            mints,
            mints_truncated,
        })
    }
}
