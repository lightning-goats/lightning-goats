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

/// Check all original lots independently of the bounded fresh candidate list.
pub(in crate::ledger) async fn pending_allocation(
    tx: &mut Transaction<'_, Sqlite>,
    id: Uuid,
    threshold: u64,
    epoch: Option<Uuid>,
) -> Result<(Vec<(i64, u64)>, bool)> {
    let rows = sqlx::query(&format!(
        "SELECT f.ledger_id,f.sats,c.remaining_sats,c.eligible_sats
         FROM feed_financial_parts f LEFT JOIN ({SOURCE_COVERAGE}) c ON c.ledger_id=f.ledger_id
         WHERE f.plan_id=?2 ORDER BY f.ledger_id LIMIT 4097"
    ))
    .bind(epoch.map(|e| e.to_string()))
    .bind(id.to_string())
    .fetch_all(&mut **tx)
    .await?;
    ensure!(
        !rows.is_empty() && rows.len() <= 4096,
        "invalid financial allocation size"
    );
    let mut parts = Vec::with_capacity(rows.len());
    let mut covered = true;
    for row in rows {
        let sats = to_u64(row.try_get("sats")?, "financial source sats")?;
        let remaining = to_u64(row.try_get("remaining_sats")?, "reserved source balance")?;
        ensure!(remaining >= sats, "reserved financial credit was consumed");
        covered &= to_u64(row.try_get("eligible_sats")?, "reserved source coverage")? >= sats;
        parts.push((row.try_get("ledger_id")?, sats));
    }
    ensure!(
        parts
            .iter()
            .try_fold(0u64, |sum, (_, n)| sum.checked_add(*n))
            == Some(threshold),
        "financial allocation changed"
    );
    Ok((parts, covered))
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
    epoch: Option<Uuid>,
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
            let (frozen, ready) = pending_allocation(tx, id, threshold, epoch).await?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{domain::payment::SettledPayment, ledger::LedgerStore};

    #[tokio::test]
    async fn covered_plan_keeps_frozen_sources_outside_the_fresh_candidate_list() -> Result<()> {
        let directory = tempfile::tempdir()?;
        let url = format!("sqlite://{}", directory.path().join("feed.db").display());
        let ledger = LedgerStore::connect(&url).await?;
        ledger.initialize_reward_funding().await?;
        ledger.configure_feeder_credit_cap(1000).await?;
        for name in ["earlier-source", "frozen-source"] {
            ledger
                .record_payment(&SettledPayment {
                    source: "strike".into(),
                    source_id: name.into(),
                    payment_hash: None,
                    address_user: "herd".into(),
                    credit_pool: "herd".into(),
                    amount_msat: 1_000_000,
                    settled_at: Some(1700000000),
                    context_json: None,
                })
                .await?;
        }
        let mut tx = ledger.pool.begin().await?;
        let (earlier_source, source): (i64, i64) =
            sqlx::query_as("SELECT MIN(ledger_id),MAX(ledger_id) FROM reward_funding_lots")
                .fetch_one(&mut *tx)
                .await?;
        let plan = Uuid::new_v4();
        sqlx::query("INSERT INTO feed_financial_plans(id,threshold_sats) VALUES(?,1000)")
            .bind(plan.to_string())
            .execute(&mut *tx)
            .await?;
        sqlx::query("INSERT INTO feed_financial_parts(plan_id,ledger_id,sats) VALUES(?,?,1000)")
            .bind(plan.to_string())
            .bind(source)
            .execute(&mut *tx)
            .await?;
        let earlier = Lot {
            ledger: earlier_source,
            remaining: 1000,
            eligible: 1000,
        };
        let Selection::Ready(id, allocation) = select(&mut tx, 1000, &[earlier], None).await?
        else {
            anyhow::bail!("the frozen sources cover the original plan");
        };
        assert_eq!(id, plan);
        assert_eq!(allocation, vec![(source, 1000)]);
        Ok(())
    }
}
