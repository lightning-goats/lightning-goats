//! Net Lightning proceeds attributable to the original Cashu credit consumed by a feed.
use super::super::to_u64;
use crate::reward_funding::StrikeFundingAnchor;
use anyhow::{Context, Result, ensure};
use sqlx::{Row, Sqlite, Transaction};
use uuid::Uuid;

pub(in crate::ledger) async fn freeze(
    tx: &mut Transaction<'_, Sqlite>,
    feed: Uuid,
    ledger: i64,
) -> Result<()> {
    // A source's settled deposits occupy disjoint intervals. Its verified fees
    // advance the interval cursor, while its returned change remains custody.
    sqlx::query(
        "WITH consumed AS (
        SELECT COALESCE(SUM(CASE WHEN request_id!=?1 THEN sats ELSE 0 END),0) AS first,
               SUM(sats) AS last FROM feed_reward_funding_parts WHERE ledger_id=?2
    ), proceeds AS (
        SELECT intent_id, MIN(last,start_sats+deposit_sats)-MAX(first,start_sats) AS sats
        FROM cashu_settled_ranges,consumed WHERE ledger_id=?2
    ) INSERT INTO feed_cashu_proceeds(request_id,ledger_id,intent_id,sats)
      SELECT ?1,?2,intent_id,sats FROM proceeds WHERE sats>0",
    )
    .bind(feed.to_string())
    .bind(ledger)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

pub(in crate::ledger) async fn parts(
    tx: &mut Transaction<'_, Sqlite>,
    feed: Uuid,
    ledger: i64,
) -> Result<Vec<(String, i64)>> {
    Ok(sqlx::query_as("SELECT intent_id,sats FROM feed_cashu_proceeds WHERE request_id=? AND ledger_id=? ORDER BY intent_id")
        .bind(feed.to_string()).bind(ledger).fetch_all(&mut **tx).await?)
}

pub(in crate::ledger) async fn anchor(
    tx: &mut Transaction<'_, Sqlite>,
    intent: &str,
) -> Result<StrikeFundingAnchor> {
    let row = sqlx::query(
        "SELECT d.receive_id,d.deposit_sats,d.payment_hash AS deposit_hash,
        json_extract(d.evidence_json,'$.payment_hash') AS settled_hash,q.*
        FROM cashu_treasury_deposits d JOIN cashu_treasury_issuances i USING(intent_id)
        JOIN strike_receive_requests q USING(receive_request_id)
        WHERE d.intent_id=? AND d.payment_hash=i.payment_hash AND i.payment_hash=q.payment_hash",
    )
    .bind(intent)
    .fetch_one(&mut **tx)
    .await?;
    let anchor = StrikeFundingAnchor {
        receive_id: row.try_get::<String, _>("receive_id")?.parse()?,
        receive_request_id: row.try_get::<String, _>("receive_request_id")?.parse()?,
        settled_msat: to_u64(row.try_get("deposit_sats")?, "treasury deposit")?
            .checked_mul(1000)
            .context("treasury funding overflow")?,
        requested_msat: to_u64(row.try_get("amount_msat")?, "treasury invoice")?,
        address_user: row.try_get("address_user")?,
        credit_pool: row.try_get("credit_pool")?,
        description_hash: row.try_get("description_hash")?,
        payment_hash: row.try_get("deposit_hash")?,
        invoice: row.try_get("invoice")?,
        settled_hash: row.try_get("settled_hash")?,
    };
    ensure!(
        anchor.is_cashu_treasury() && anchor.settled_msat == anchor.requested_msat,
        "treasury funding anchor mismatch"
    );
    anchor.validate()?;
    Ok(anchor)
}

// Keep the original ledger cursor for the donation remainder. Append one
// cursor per distinct Lightning receive so payout qualifies a batch only once.
pub(in crate::ledger) const PAGE: &str = "WITH original AS (
    SELECT p.ledger_id,p.sats-COALESCE((SELECT SUM(c.sats) FROM feed_cashu_proceeds c
        WHERE c.request_id=p.request_id AND c.ledger_id=p.ledger_id),0) AS sats,
        l.kind,l.proof_hash,NULL AS intent_id FROM feed_reward_funding_parts p
        JOIN reward_funding_lots l USING(ledger_id) WHERE p.request_id=?1
), deposits AS (
    SELECT (SELECT MAX(ledger_id) FROM original)+ROW_NUMBER() OVER(ORDER BY intent_id) AS ledger_id,
        SUM(sats) AS sats,'cashu_deposit' AS kind,'' AS proof_hash,intent_id FROM feed_cashu_proceeds
        WHERE request_id=?1 GROUP BY intent_id
), funding_parts AS (
    SELECT * FROM original WHERE sats>0 UNION ALL SELECT * FROM deposits
)";
