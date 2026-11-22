//! Protected, read-only evidence for an original Cashu settlement.
use super::super::{LedgerStore, feed_sources::pending_allocation, to_u64};
use super::treasury::load_intent;
use anyhow::{Context, Result, ensure};
use serde::Serialize;
use sqlx::Row;
use uuid::Uuid;

#[derive(Serialize)]
pub struct CashuSettlementStatus {
    pub intent_id: Uuid,
    pub core_epoch: Uuid,
    pub mint_id: String,
    pub input_sats: u64,
    pub expected_deposit_sats: u64,
    pub issuance_claimed: bool,
    pub receive_request_id: Option<Uuid>,
    pub verified_receive_id: Option<Uuid>,
    pub strike_deposit_verified_at: Option<i64>,
    pub wallet_report_retained: bool,
    pub settlement_complete: bool,
    pub settled_fee_sats: Option<u64>,
    pub settled_change_sats: Option<u64>,
    pub delivery_attempts: u32,
    pub delivery_closed: bool,
    pub inspection_attempts: u32,
    pub next_inspection_at: Option<i64>,
    pub linked_feed_plan: Option<FinancialPlanStatus>,
}

#[derive(Serialize)]
pub struct FinancialPlanStatus {
    pub id: Uuid,
    pub state: String,
    pub threshold_sats: u64,
    pub pending_sources_covered: Option<bool>,
    pub feeder_request_id: Option<Uuid>,
    pub feeder_status: Option<String>,
}

impl LedgerStore {
    /// Read a single transaction snapshot. The result carries no authority to
    /// query a provider, create a payment, or dispatch a physical attempt.
    pub async fn inspect_cashu_settlement(&self, id: Uuid) -> Result<CashuSettlementStatus> {
        ensure!(!id.is_nil(), "invalid settlement intent");
        let mut tx = self.pool.begin().await?;
        let intent = load_intent(&mut tx, id)
            .await?
            .context("unknown Cashu settlement intent")?;
        let row = sqlx::query(
            "SELECT q.intent_id IS NOT NULL AS issuance_claimed,s.receive_request_id,
                    d.receive_id,d.verified_at,r.intent_id IS NOT NULL AS wallet_report,
                    c.intent_id IS NOT NULL AS complete,c.fee_sats,c.change_sats,
                    COALESCE(w.attempts,0) AS delivery_attempts,
                    COALESCE(n.delivery_closed,0) AS delivery_closed,
                    COALESCE(n.attempts,0) AS inspection_attempts,n.next_attempt,
                    p.id AS plan_id,p.state AS plan_state,p.threshold_sats,
                    p.request_id,f.status AS feeder_status
             FROM cashu_treasury_intents i
             LEFT JOIN cashu_treasury_claims q ON q.intent_id=i.id
             LEFT JOIN cashu_treasury_issuances s ON s.intent_id=i.id
             LEFT JOIN cashu_treasury_deposits d ON d.intent_id=i.id
             LEFT JOIN cashu_treasury_wallet_results r ON r.intent_id=i.id
             LEFT JOIN cashu_treasury_completions c ON c.intent_id=i.id
             LEFT JOIN cashu_treasury_work w ON w.intent_id=i.id AND w.stage='wallet'
             LEFT JOIN cashu_treasury_inspections n ON n.intent_id=i.id
             LEFT JOIN feed_financial_treasury j ON j.intent_id=i.id
             LEFT JOIN feed_financial_plans p ON p.id=j.plan_id
             LEFT JOIN feed_attempts f ON f.id=p.request_id
             WHERE i.id=?",
        )
        .bind(id.to_string())
        .fetch_one(&mut *tx)
        .await?;
        let receive_request_id = read_id(&row, "receive_request_id")?;
        let verified_receive_id = read_id(&row, "receive_id")?;
        let wallet_report_retained = row.try_get::<bool, _>("wallet_report")?;
        let settlement_complete = row.try_get::<bool, _>("complete")?;
        ensure!(
            settlement_complete == (verified_receive_id.is_some() && wallet_report_retained),
            "retained settlement status is inconsistent"
        );
        let linked_feed_plan = if let Some(plan_id) = read_id(&row, "plan_id")? {
            let state: String = row.try_get("plan_state")?;
            let threshold_sats = to_u64(row.try_get("threshold_sats")?, "financial threshold")?;
            let pending_sources_covered = if state == "pending" {
                Some(
                    pending_allocation(
                        &mut tx,
                        plan_id,
                        threshold_sats,
                        self.retained_wallet_epoch,
                    )
                    .await?
                    .1,
                )
            } else {
                None
            };
            Some(FinancialPlanStatus {
                id: plan_id,
                state,
                threshold_sats,
                pending_sources_covered,
                feeder_request_id: read_id(&row, "request_id")?,
                feeder_status: row.try_get("feeder_status")?,
            })
        } else {
            None
        };
        let status = CashuSettlementStatus {
            intent_id: id,
            core_epoch: intent.core_epoch,
            mint_id: intent.mint_id,
            input_sats: intent.input_sats,
            expected_deposit_sats: intent.deposit_sats,
            issuance_claimed: row.try_get("issuance_claimed")?,
            receive_request_id,
            verified_receive_id,
            strike_deposit_verified_at: row.try_get("verified_at")?,
            wallet_report_retained,
            settlement_complete,
            settled_fee_sats: row
                .try_get::<Option<i64>, _>("fee_sats")?
                .map(|sats| to_u64(sats, "settled fee"))
                .transpose()?,
            settled_change_sats: row
                .try_get::<Option<i64>, _>("change_sats")?
                .map(|sats| to_u64(sats, "settled change"))
                .transpose()?,
            delivery_attempts: row.try_get("delivery_attempts")?,
            delivery_closed: row.try_get("delivery_closed")?,
            inspection_attempts: row.try_get("inspection_attempts")?,
            next_inspection_at: row.try_get("next_attempt")?,
            linked_feed_plan,
        };
        tx.commit().await?;
        Ok(status)
    }
}

fn read_id(row: &sqlx::sqlite::SqliteRow, name: &str) -> Result<Option<Uuid>> {
    Ok(row
        .try_get::<Option<String>, _>(name)?
        .map(|encoded| encoded.parse())
        .transpose()?)
}
