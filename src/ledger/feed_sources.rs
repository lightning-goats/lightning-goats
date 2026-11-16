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
    // Cashu coverage requires both the independently verified Strike deposit
    // and the authenticated owned-change result. Returned change is not covered.
    // Prior actual debits consume that coverage once; unsettled lots are skipped.
    let rows = sqlx::query("SELECT * FROM (SELECT l.ledger_id,l.remaining_sats,CASE WHEN c.ledger_entry_id IS NULL THEN l.remaining_sats ELSE MIN(l.remaining_sats,MAX(0,(SELECT COALESCE(SUM(r.input_sats-r.change_sats),0) FROM cashu_treasury_source_results r JOIN cashu_treasury_completions t ON t.intent_id=r.intent_id WHERE r.ledger_id=l.ledger_id)-(SELECT COALESCE(SUM(p.sats),0) FROM feed_reward_funding_parts p WHERE p.ledger_id=l.ledger_id))) END AS eligible_sats FROM reward_funding_lots l LEFT JOIN cashu_receipt_inbox c ON c.ledger_entry_id=l.ledger_id WHERE l.remaining_sats>0) ORDER BY CASE WHEN eligible_sats>0 THEN 0 ELSE 1 END,ledger_id LIMIT 4096")
        .fetch_all(&mut **tx).await?;
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
