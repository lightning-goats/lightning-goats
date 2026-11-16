//! One pending financial allocation; physical serialization remains in feed.rs.
use super::*;

pub(super) struct Lot {
    pub ledger: i64,
    pub remaining: u64,
    pub eligible: u64,
}

fn choose(lots: &[Lot], threshold: u64, settled: bool) -> Option<Vec<(i64, u64)>> {
    let mut ordered = lots.iter().collect::<Vec<_>>();
    ordered.sort_by_key(|lot| lot.ledger);
    let mut left = threshold;
    let mut parts = Vec::new();
    for lot in ordered {
        let sats = if settled { lot.eligible } else { lot.remaining }.min(left);
        if sats > 0 {
            parts.push((lot.ledger, sats));
            left -= sats;
        }
        if left == 0 {
            return Some(parts);
        }
    }
    None
}

async fn parts(tx: &mut Transaction<'_, Sqlite>, id: Uuid) -> Result<Vec<(i64, u64)>> {
    let rows = sqlx::query("SELECT ledger_id,sats FROM feed_financial_parts WHERE plan_id=? ORDER BY ledger_id LIMIT 4097")
        .bind(id.to_string()).fetch_all(&mut **tx).await?;
    ensure!(
        !rows.is_empty() && rows.len() <= 4096,
        "invalid financial allocation size"
    );
    rows.into_iter()
        .map(|r| {
            Ok((
                r.try_get("ledger_id")?,
                to_u64(r.try_get("sats")?, "financial source sats")?,
            ))
        })
        .collect()
}

async fn release(tx: &mut Transaction<'_, Sqlite>, id: Uuid, reason: &str) -> Result<()> {
    ensure!(sqlx::query("UPDATE feed_financial_plans SET state='released',disposition=? WHERE id=? AND state='pending'")
        .bind(reason).bind(id.to_string()).execute(&mut **tx).await?.rows_affected()==1,
        "financial plan changed before release");
    Ok(())
}

pub(super) async fn select(
    tx: &mut Transaction<'_, Sqlite>,
    threshold: u64,
    lots: &[Lot],
) -> Result<Selection> {
    let pending =
        sqlx::query("SELECT id,threshold_sats FROM feed_financial_plans WHERE state='pending'")
            .fetch_optional(&mut **tx)
            .await?;
    let mut waiting = None;
    if let Some(row) = pending {
        let id: Uuid = row.try_get::<String, _>("id")?.parse()?;
        if to_u64(row.try_get("threshold_sats")?, "financial threshold")? != threshold {
            release(tx, id, "threshold_changed").await?;
        } else {
            let frozen = parts(tx, id).await?;
            ensure!(
                frozen
                    .iter()
                    .try_fold(0u64, |sum, (_, n)| sum.checked_add(*n))
                    == Some(threshold),
                "financial allocation changed"
            );
            // Check frozen lots directly: bounded candidate selection may omit an
            // older pending source when many later settled donations arrive.
            let mut ready = true;
            for (ledger, sats) in &frozen {
                let remaining: i64 = sqlx::query_scalar(
                    "SELECT remaining_sats FROM reward_funding_lots WHERE ledger_id=?",
                )
                .bind(ledger)
                .fetch_one(&mut **tx)
                .await?;
                ensure!(
                    to_u64(remaining, "reserved source balance")? >= *sats,
                    "reserved financial credit was consumed"
                );
                ready &= lots
                    .iter()
                    .find(|l| l.ledger == *ledger)
                    .is_some_and(|l| l.eligible >= *sats);
            }
            if ready {
                return Ok(Selection::Ready(id, frozen));
            }
            waiting = Some(id);
        }
    }
    let settled = choose(lots, threshold, true);
    if let Some(id) = waiting {
        if settled.is_none() {
            return Ok(Selection::Waiting);
        }
        // Transfer history is independent of this unconsumed credit reservation.
        release(tx, id, "settled_supersession").await?;
    }
    let ready = settled.is_some();
    let Some(selected) = settled.or_else(|| choose(lots, threshold, false)) else {
        return Ok(Selection::Waiting);
    };
    ensure!(selected.len() <= 4096, "financial source bound exceeded");
    let id = Uuid::new_v4();
    sqlx::query("INSERT INTO feed_financial_plans(id,threshold_sats) VALUES(?,?)")
        .bind(id.to_string())
        .bind(to_i64(threshold, "financial threshold")?)
        .execute(&mut **tx)
        .await?;
    if let Some(original) = waiting {
        sqlx::query(
            "INSERT INTO feed_financial_supersessions(original_id,replacement_id) VALUES(?,?)",
        )
        .bind(original.to_string())
        .bind(id.to_string())
        .execute(&mut **tx)
        .await?;
    }
    for (ledger, sats) in &selected {
        sqlx::query("INSERT INTO feed_financial_parts(plan_id,ledger_id,sats) VALUES(?,?,?)")
            .bind(id.to_string())
            .bind(ledger)
            .bind(to_i64(*sats, "financial source")?)
            .execute(&mut **tx)
            .await?;
    }
    Ok(if ready {
        Selection::Ready(id, selected)
    } else {
        Selection::Waiting
    })
}

pub(super) async fn dispatch(
    tx: &mut Transaction<'_, Sqlite>,
    plan: Uuid,
    request: Uuid,
    threshold: u64,
    selected: &[(i64, u64)],
) -> Result<()> {
    ensure!(
        parts(tx, plan).await? == selected,
        "physical sources differ from financial allocation"
    );
    ensure!(sqlx::query("UPDATE feed_financial_plans SET state='dispatched',disposition='physical_intent',request_id=? WHERE id=? AND state='pending' AND threshold_sats=?")
        .bind(request.to_string()).bind(plan.to_string()).bind(to_i64(threshold, "financial threshold")?)
        .execute(&mut **tx).await?.rows_affected()==1, "financial plan is not available for dispatch");
    Ok(())
}
