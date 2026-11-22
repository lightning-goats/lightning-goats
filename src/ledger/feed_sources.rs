//! Source selection before physical intent, distinct from treasury waiting.
//! Frozen parts also drive the actual debit and its reward provenance.
use super::{to_i64, to_u64};
use anyhow::{Result, ensure};
use sqlx::{Row, Sqlite, Transaction};
use uuid::Uuid;

pub(super) enum Selection {
    Legacy,
    Waiting,
    Ready(Uuid, Vec<(i64, u64)>),
}

// Cashu coverage requires both the independently verified destination receipt and
// the authenticated wallet result. The furthest verified input covers gross
// contribution credit. Spending fee change again cannot duplicate that input.
const SOURCE_COVERAGE: &str = "SELECT l.ledger_id,l.remaining_sats,
    CASE WHEN c.ledger_entry_id IS NULL OR EXISTS(SELECT 1 FROM collector_handoff_amounts g WHERE g.ledger_id=l.ledger_id) THEN l.remaining_sats
         ELSE MIN(l.remaining_sats,MAX(0,
           (SELECT COALESCE(MAX(start_sats+input_sats),0) FROM cashu_funding_ranges WHERE ledger_id=l.ledger_id)
           -(SELECT COALESCE(SUM(p.sats),0) FROM feed_reward_funding_parts p WHERE p.ledger_id=l.ledger_id)))
    END AS eligible_sats
    FROM reward_funding_lots l LEFT JOIN cashu_receipt_inbox c ON c.ledger_entry_id=l.ledger_id";

pub(super) async fn select(tx: &mut Transaction<'_, Sqlite>, threshold: u64) -> Result<Selection> {
    let cashu: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM cashu_receipt_inbox)")
        .fetch_one(&mut **tx)
        .await?;
    if !cashu {
        return Ok(Selection::Legacy);
    }
    ensure!(
        super::reward_funding::prepare(tx).await?,
        "Cashu source history is not initialized"
    );
    let rows = sqlx::query(&format!(
        "{SOURCE_COVERAGE} WHERE l.remaining_sats>0
         ORDER BY CASE WHEN eligible_sats>0 THEN 0 ELSE 1 END,l.ledger_id LIMIT 4096"
    ))
    .fetch_all(&mut **tx)
    .await?;
    let mut lots = Vec::new();
    for row in rows {
        lots.push(financial::Lot {
            ledger: row.try_get("ledger_id")?,
            remaining: to_u64(row.try_get("remaining_sats")?, "remaining source credit")?,
            eligible: to_u64(row.try_get("eligible_sats")?, "available source credit")?,
        });
    }
    financial::select(tx, threshold, &lots).await
}

mod financial;
#[cfg(unix)]
pub(super) use financial::pending_allocation;

pub(super) async fn freeze(
    tx: &mut Transaction<'_, Sqlite>,
    id: Uuid,
    plan: Uuid,
    threshold: u64,
    parts: &[(i64, u64)],
) -> Result<()> {
    ensure!(
        parts
            .iter()
            .try_fold(0u64, |sum, (_, n)| sum.checked_add(*n))
            == Some(threshold),
        "source allocation does not cover physical intent"
    );
    financial::dispatch(tx, plan, id, threshold, parts).await?;
    sqlx::query("INSERT INTO feed_source_reservations(request_id,threshold_sats) VALUES(?,?)")
        .bind(id.to_string())
        .bind(to_i64(threshold, "frozen threshold")?)
        .execute(&mut **tx)
        .await?;
    for (ledger, sats) in parts {
        sqlx::query("INSERT INTO feed_source_parts(request_id,ledger_id,sats) VALUES(?,?,?)")
            .bind(id.to_string())
            .bind(ledger)
            .bind(to_i64(*sats, "frozen source sats")?)
            .execute(&mut **tx)
            .await?;
    }
    Ok(())
}

pub(super) async fn frozen(tx: &mut Transaction<'_, Sqlite>, id: Uuid) -> Result<bool> {
    Ok(sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM feed_source_reservations WHERE request_id=?)",
    )
    .bind(id.to_string())
    .fetch_one(&mut **tx)
    .await?)
}
pub(super) async fn triggering_credit(
    tx: &mut Transaction<'_, Sqlite>,
    id: Uuid,
) -> Result<Option<(i64, u64)>> {
    sqlx::query("SELECT p.ledger_id,l.delta_sats FROM feed_source_parts p JOIN ledger_entries l ON l.id=p.ledger_id WHERE p.request_id=? ORDER BY p.ledger_id DESC LIMIT 1")
        .bind(id.to_string()).fetch_optional(&mut **tx).await?
        .map(|row|Ok((row.try_get("ledger_id")?,to_u64(row.try_get("delta_sats")?,"source receipt amount")?))).transpose()
}
