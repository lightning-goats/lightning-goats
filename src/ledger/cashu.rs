//! Atomic authenticated Cashu receipt import. No mint calls, proof material,
//! native Strike settlement, membership, or payout authority is manufactured.
use anyhow::{Context, Result, ensure};
use lightning_goats_cashu_receipt::WireReceipt;
use serde_json::json;
use sqlx::{Row, Sqlite, Transaction};

use super::{LedgerStore, append_event_in_transaction, to_i64, to_u64};
use crate::cashu::receipt_authority::{PeerReceipt, ReceiptPolicy, RemainingBudget};

pub(super) mod feed_proceeds;
mod financial_jobs;
mod sweep_jobs;
mod treasury;
mod treasury_planning;
mod treasury_scheduling;
mod treasury_status;
pub use treasury::TreasuryPlan;
#[cfg(unix)]
pub(crate) use treasury::TreasurySettlementState;
pub use treasury_status::{CashuSettlementStatus, FinancialPlanStatus};

/// The immutable original acknowledgment, including on a retry after spending.
pub use lightning_goats_cashu_receipt::acknowledgment::ImportResult as CashuImportResult;

impl LedgerStore {
    pub(crate) async fn record_collector_handoff(
        &self,
        h: &lightning_goats_cashu_receipt::CollectorHandoff,
    ) -> Result<()> {
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let ledger: i64 = sqlx::query_scalar("SELECT i.ledger_entry_id FROM cashu_receipt_inbox i
            JOIN retained_cashu_lots g ON g.ledger_id=i.ledger_entry_id
            WHERE i.wallet_instance=? AND i.wallet_epoch=? AND i.core_epoch=? AND i.acquisition_id=?
              AND i.evidence_digest=? AND i.net_sats>=? AND i.core_epoch=(SELECT stream_id FROM overlay_identity WHERE singleton=1)")
            .bind(h.wallet_instance.to_string()).bind(h.wallet_epoch.to_string()).bind(h.core_epoch.to_string())
            .bind(h.acquisition_id.to_string()).bind(&h.evidence_digest).bind(to_i64(h.collected_sats,"collected amount")?)
            .fetch_one(&mut *tx).await?;
        sqlx::query(
            "INSERT INTO collector_handoff_amounts VALUES(?,?) ON CONFLICT(ledger_id) DO NOTHING",
        )
        .bind(ledger)
        .bind(to_i64(h.collected_sats, "collected amount")?)
        .execute(&mut *tx)
        .await?;
        let recorded: i64 = sqlx::query_scalar(
            "SELECT collected_sats FROM collector_handoff_amounts WHERE ledger_id=?",
        )
        .bind(ledger)
        .fetch_one(&mut *tx)
        .await?;
        ensure!(
            to_u64(recorded, "collected amount")? == h.collected_sats,
            "conflicting collector acknowledgment"
        );
        tx.commit().await?;
        Ok(())
    }

    /// Only reachable with a kernel-authenticated private receipt. The separate
    /// socket server must load policy from protected core configuration. New
    /// import and duplicate acknowledgment deliberately have different gates.
    pub async fn import_cashu_receipt(
        &self,
        policy: &ReceiptPolicy,
        authenticated: &PeerReceipt,
    ) -> Result<CashuImportResult> {
        self.import_cashu_with_custody(policy, authenticated, None)
            .await
    }

    pub(crate) async fn import_cashu_with_custody(
        &self,
        policy: &ReceiptPolicy,
        authenticated: &PeerReceipt,
        retained_mint: Option<&str>,
    ) -> Result<CashuImportResult> {
        let receipt = authenticated.receipt();
        let data = receipt.data();
        let contact = lightning_goats_cashu_receipt::contact_fee::ContactTender::source_terms(
            &data.source_ref,
        )?;
        let digest = hex::encode(receipt.digest()?);
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let epoch: String =
            sqlx::query_scalar("SELECT stream_id FROM overlay_identity WHERE singleton=1")
                .fetch_optional(&mut *tx)
                .await?
                .context("core epoch must be initialized before Cashu import")?;
        ensure!(
            epoch == policy.core_epoch.to_string(),
            "core epoch changed; Cashu recovery required"
        );
        // Validate the authenticated registration even if an exact replay exists.
        // Current policy limits can be reduced/off without hiding prior receipts.
        policy.check_registration(authenticated)?;
        let private_ack = || CashuImportResult {
            retained_cashu: false,
            acquisition_id: data.acquisition_id,
            receipt_digest: digest.clone(),
            event_seq: 0,
            net_sats: data.net_sats,
            original_feed_credit_sats: 0,
        };
        let previous_private:Option<String>=sqlx::query_scalar("SELECT receipt_digest FROM private_contact_cashu_receipts WHERE acquisition_id=? OR (wallet_instance=? AND evidence_digest=?)")
            .bind(data.acquisition_id.to_string()).bind(data.wallet_instance.to_string()).bind(&data.acquisition_evidence_digest)
            .fetch_optional(&mut *tx).await?;
        if let Some(original) = previous_private {
            ensure!(
                contact.is_some() && original == digest,
                "private receipt identity conflict"
            );
            tx.commit().await?;
            return Ok(private_ack());
        }
        let existing = sqlx::query(
            "SELECT * FROM cashu_receipt_inbox WHERE wallet_instance=? AND acquisition_id=?",
        )
        .bind(data.wallet_instance.to_string())
        .bind(data.acquisition_id.to_string())
        .fetch_optional(&mut *tx)
        .await?;
        if let Some(row) = existing {
            ensure!(
                row.try_get::<String, _>("receipt_digest")? == digest,
                "Cashu acquisition conflicts with original receipt"
            );
            let mut result = verify_row(&mut tx, &row, receipt).await?;
            result.retained_cashu = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM retained_cashu_lots WHERE ledger_id=?)",
            )
            .bind(row.try_get::<i64, _>("ledger_entry_id")?)
            .fetch_one(&mut *tx)
            .await?;
            tx.commit().await?;
            return Ok(result);
        }

        // Reuse the audited source-lot substrate only after explicit startup
        // initialization; importing a receipt never initializes paid history or
        // enables the independently gated CyberHerd payout policy.
        ensure!(
            super::reward_funding::prepare(&mut tx).await?,
            "source funding history requires explicit initialization"
        );
        let now: i64 = sqlx::query_scalar("SELECT unixepoch()")
            .fetch_one(&mut *tx)
            .await?;
        let previous: Option<i64> =
            sqlx::query_scalar("SELECT last_seen FROM cashu_import_clock WHERE singleton=1")
                .fetch_optional(&mut *tx)
                .await?;
        ensure!(
            now >= 0 && previous.is_none_or(|last| now >= last),
            "Cashu import clock regressed"
        );
        let used = sqlx::query("SELECT COUNT(*)+(SELECT COUNT(*) FROM private_contact_cashu_receipts WHERE accepted_at>=?1) AS count,COALESCE(SUM(l.delta_sats),0) AS sats FROM cashu_receipt_inbox i JOIN ledger_entries l ON l.id=i.ledger_entry_id WHERE i.accepted_at>=?1")
            .bind(now.saturating_sub(86_400)).fetch_one(&mut *tx).await?;
        // Until a verified treasury allocation consumes custody, every acquired
        // sat remains exposure, including credit retired by the payment cap.
        let custody: i64 =
            sqlx::query_scalar(concat!(include_str!("cashu/treasury_balances.sql"), "SELECT (SELECT COALESCE(SUM(net_sats),0) FROM cashu_receipt_inbox)+(SELECT COALESCE(SUM(net_sats),0) FROM private_contact_cashu_receipts)-(SELECT COALESCE(SUM(input_sats-change_sats),0) FROM settled_sources)"))
                .fetch_one(&mut *tx)
                .await?;
        let limits = &policy.limits;
        for bound in [limits.credit_sats_per_24h, limits.maximum_custody_sats] {
            ensure!(
                bound > 0 && bound <= i64::MAX as u64,
                "Cashu core limits are disabled or invalid"
            );
        }
        ensure!(
            if limits.unlimited_receipts_per_24h {
                limits.receipts_per_24h == 0
            } else {
                (1..=i64::MAX as u64).contains(&limits.receipts_per_24h)
            },
            "Cashu receipt count limit is disabled or invalid"
        );
        let remaining = RemainingBudget {
            receipt_count: if limits.unlimited_receipts_per_24h {
                u64::MAX
            } else {
                limits
                    .receipts_per_24h
                    .saturating_sub(to_u64(used.try_get("count")?, "Cashu count")?)
            },
            credit_sats: limits
                .credit_sats_per_24h
                .saturating_sub(to_u64(used.try_get("sats")?, "Cashu rate usage")?),
            custody_sats: limits
                .maximum_custody_sats
                .saturating_sub(to_u64(custody, "Cashu custody")?),
        };
        if let Some((id, delivery)) = contact {
            ensure!(
                data.transport == "cashu"
                    && data.net_sats >= lightning_goats_cashu_receipt::contact_fee::FEE_SATS,
                "invalid contact custody receipt"
            );
            // Contact is subject to receipt-count and custody limits. It never uses feeder credit.
            policy.check_new(
                authenticated,
                &RemainingBudget {
                    credit_sats: u64::MAX,
                    ..remaining
                },
            )?;
            let inserted=sqlx::query("INSERT INTO private_contact_cashu_receipts(acquisition_id,delivery_id,wallet_instance,wallet_epoch,core_epoch,receipt_digest,evidence_digest,mint_id,net_sats,receive_fee_sats)
                SELECT ?,t.delivery_id,?,?,?,?,?,?,?,? FROM private_contact_tenders t WHERE t.delivery_id=? AND t.contact_id=? AND t.state='reserved'
                AND NOT EXISTS(SELECT 1 FROM cashu_receipt_inbox WHERE wallet_instance=? AND (acquisition_id=? OR evidence_digest=?))")
                .bind(data.acquisition_id.to_string()).bind(data.wallet_instance.to_string()).bind(data.wallet_epoch.to_string()).bind(&epoch)
                .bind(&digest).bind(&data.acquisition_evidence_digest).bind(&data.mint_id).bind(i64::try_from(data.net_sats)?).bind(i64::try_from(data.receive_fee_sats)?)
                .bind(delivery.to_string()).bind(id.to_string()).bind(data.wallet_instance.to_string()).bind(data.acquisition_id.to_string()).bind(&data.acquisition_evidence_digest)
                .execute(&mut *tx).await?;
            ensure!(
                inserted.rows_affected() == 1,
                "missing original contact tender or conflicting receipt"
            );
            sqlx::query("INSERT INTO cashu_import_clock VALUES(1,?) ON CONFLICT(singleton) DO UPDATE SET last_seen=excluded.last_seen").bind(now).execute(&mut *tx).await?;
            tx.commit().await?;
            return Ok(private_ack());
        }
        policy.check_new(authenticated, &remaining)?;
        let retained = retained_mint == Some(data.mint_id.as_str());
        let contribution = if retained {
            data.gross_sats
        } else {
            data.net_sats
        };
        ensure!(
            contribution <= remaining.credit_sats,
            "Cashu daily feeder credit limit exceeded"
        );
        let amount = to_i64(contribution, "Cashu contribution")?;
        let user = receipt_user(&mut tx, receipt).await?;
        let ledger_id = sqlx::query("INSERT INTO ledger_entries(entry_type,source_key,delta_sats,payment_source,payment_source_id,address_user,credit_pool) VALUES('CASHU_RECEIPT',?,?,?,?,?,'herd')")
            .bind(source_key(receipt)).bind(amount).bind(&data.transport).bind(data.acquisition_id.to_string()).bind(&user)
            .execute(&mut *tx).await?.last_insert_rowid();
        let after = super::feed_cap::cap_new_credit(&mut tx, ledger_id, contribution).await?;
        // Intentionally narrow projection. The mint, balances, private evidence,
        // source event IDs and all proof/operation material stay private.
        let event = append_event_in_transaction(
            &mut tx,
            "payment_received",
            &public_event(receipt, &user, contribution, after),
        )
        .await?;
        sqlx::query("INSERT INTO cashu_receipt_inbox(wallet_instance,acquisition_id,wallet_epoch,core_epoch,mint_id,evidence_digest,receipt_digest,receipt_json,net_sats,ledger_entry_id,event_seq,original_feed_credit_sats,accepted_at) VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?)")
            .bind(data.wallet_instance.to_string()).bind(data.acquisition_id.to_string()).bind(data.wallet_epoch.to_string())
            .bind(&epoch).bind(&data.mint_id).bind(&data.acquisition_evidence_digest).bind(&digest)
            .bind(String::from_utf8(receipt.encode()?)?).bind(to_i64(data.net_sats, "Cashu custody")?).bind(ledger_id).bind(to_i64(event,"Cashu event")?)
            .bind(to_i64(after,"Cashu credit after cap")?).bind(now).execute(&mut *tx).await?;
        if retained {
            sqlx::query("INSERT INTO retained_cashu_lots(ledger_id) VALUES (?)")
                .bind(ledger_id)
                .execute(&mut *tx)
                .await?;
        }
        sqlx::query("INSERT INTO cashu_import_clock VALUES(1,?) ON CONFLICT(singleton) DO UPDATE SET last_seen=excluded.last_seen")
            .bind(now).execute(&mut *tx).await?;
        // The immutable private receipt exists before source proof capture.
        super::reward_funding::prepare(&mut tx).await?;
        let result = CashuImportResult {
            retained_cashu: retained_mint == Some(data.mint_id.as_str()),
            acquisition_id: data.acquisition_id,
            receipt_digest: digest,
            event_seq: event,
            net_sats: data.net_sats,
            original_feed_credit_sats: after,
        };
        tx.commit().await?;
        Ok(result)
    }
}

fn source_key(receipt: &WireReceipt) -> String {
    format!(
        "cashu:{}:{}",
        receipt.data().wallet_instance,
        receipt.data().acquisition_id
    )
}
async fn receipt_user(tx: &mut Transaction<'_, Sqlite>, receipt: &WireReceipt) -> Result<String> {
    let data = receipt.data();
    let Some((id, amount)) =
        lightning_goats_cashu_receipt::receive_status::FeedingCheckout::source_terms(
            &data.source_ref,
        )?
    else {
        return Ok("herd".into());
    };
    ensure!(
        data.transport == "cashu" && data.net_sats >= amount,
        "checkout tender does not match receipt"
    );
    let user:String=sqlx::query_scalar("SELECT json_extract(intent,'$.user') FROM breez_project_invoices WHERE id=? AND json_type(intent,'$.checkout')='object' AND json_extract(intent,'$.credit_pool')='herd' AND json_extract(intent,'$.amount_sat')=?")
        .bind(id.to_string()).bind(i64::try_from(amount)?).fetch_one(&mut **tx).await?;
    Ok(user)
}
fn public_event(
    receipt: &WireReceipt,
    user: &str,
    contribution: u64,
    after: u64,
) -> serde_json::Value {
    json!({"source":receipt.data().transport,"amount_sats":contribution,
        "feed_credit_sats":after,"address_user":user,"credit_pool":"herd"})
}

async fn verify_row(
    tx: &mut Transaction<'_, Sqlite>,
    row: &sqlx::sqlite::SqliteRow,
    receipt: &WireReceipt,
) -> Result<CashuImportResult> {
    let stored = WireReceipt::decode(row.try_get::<String, _>("receipt_json")?.as_bytes())?;
    ensure!(
        stored.digest()? == receipt.digest()?,
        "Cashu inbox data changed"
    );
    let data = receipt.data();
    ensure!(
        row.try_get::<String, _>("wallet_instance")? == data.wallet_instance.to_string()
            && row.try_get::<String, _>("acquisition_id")? == data.acquisition_id.to_string()
            && row.try_get::<String, _>("wallet_epoch")? == data.wallet_epoch.to_string()
            && row.try_get::<String, _>("mint_id")? == data.mint_id
            && row.try_get::<String, _>("evidence_digest")? == data.acquisition_evidence_digest
            && row.try_get::<String, _>("receipt_digest")? == hex::encode(receipt.digest()?)
            && row.try_get::<i64, _>("net_sats")? == to_i64(data.net_sats, "stored custody")?,
        "Cashu inbox index disagrees with original receipt"
    );
    let ledger = sqlx::query("SELECT * FROM ledger_entries WHERE id=?")
        .bind(row.try_get::<i64, _>("ledger_entry_id")?)
        .fetch_one(&mut **tx)
        .await?;
    let data = receipt.data();
    let user = receipt_user(tx, receipt).await?;
    let contribution = to_u64(ledger.try_get("delta_sats")?, "Cashu contribution")?;
    let retained: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM retained_cashu_lots WHERE ledger_id=?)")
            .bind(row.try_get::<i64, _>("ledger_entry_id")?)
            .fetch_one(&mut **tx)
            .await?;
    ensure!(
        ledger.try_get::<String, _>("entry_type")? == "CASHU_RECEIPT"
            && ledger.try_get::<String, _>("source_key")? == source_key(receipt)
            && (contribution == data.net_sats || (retained && contribution == data.gross_sats))
            && ledger
                .try_get::<Option<String>, _>("payment_source")?
                .as_deref()
                == Some(data.transport.as_str())
            && ledger
                .try_get::<Option<String>, _>("payment_source_id")?
                .as_deref()
                == Some(data.acquisition_id.to_string().as_str())
            && ledger
                .try_get::<Option<String>, _>("address_user")?
                .as_deref()
                == Some(user.as_str())
            && ledger
                .try_get::<Option<String>, _>("credit_pool")?
                .as_deref()
                == Some("herd")
            && ledger
                .try_get::<Option<String>, _>("payment_hash")?
                .is_none()
            && ledger
                .try_get::<Option<String>, _>("feed_attempt_id")?
                .is_none(),
        "Cashu receipt lost original credit provenance"
    );
    let after = to_u64(
        row.try_get("original_feed_credit_sats")?,
        "original Cashu acknowledgment",
    )?;
    let event = sqlx::query("SELECT event_type,payload_json FROM event_log WHERE seq=?")
        .bind(row.try_get::<i64, _>("event_seq")?)
        .fetch_one(&mut **tx)
        .await?;
    let mut payload: serde_json::Value =
        serde_json::from_str(&event.try_get::<String, _>("payload_json")?)?;
    // The normal event writer snapshots advisory feeder status for rendering.
    // Validate the original financial projection even after that status changes.
    if let Some(fields) = payload.as_object_mut() {
        fields.remove("feeding");
    }
    ensure!(
        event.try_get::<String, _>("event_type")? == "payment_received"
            && payload == public_event(receipt, &user, contribution, after),
        "Cashu receipt lost original public event"
    );
    Ok(CashuImportResult {
        retained_cashu: false,
        acquisition_id: data.acquisition_id,
        receipt_digest: hex::encode(receipt.digest()?),
        event_seq: to_u64(row.try_get("event_seq")?, "Cashu event")?,
        net_sats: data.net_sats,
        original_feed_credit_sats: after,
    })
}

pub(super) async fn lot_identity(
    tx: &mut Transaction<'_, Sqlite>,
    ledger_id: i64,
) -> Result<serde_json::Value> {
    let row = sqlx::query("SELECT * FROM cashu_receipt_inbox WHERE ledger_entry_id=?")
        .bind(ledger_id)
        .fetch_one(&mut **tx)
        .await?;
    let receipt = WireReceipt::decode(row.try_get::<String, _>("receipt_json")?.as_bytes())?;
    verify_row(tx, &row, &receipt).await?;
    Ok(json!(["cashu-custody-v1", hex::encode(receipt.digest()?)]))
}

#[cfg(test)]
mod tests;
