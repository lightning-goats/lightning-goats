//! Durable provenance for verified native BTC receipts.

use anyhow::{Result, bail};
use sqlx::{Row, Sqlite, Transaction};

use super::to_i64;

/// Called ONLY inside record_payment's settlement/ledger/event transaction.
/// One BTC identity valuation per pre-existing source identity; no extra credit.
pub(super) async fn record_native_btc(
    tx: &mut Transaction<'_, Sqlite>,
    settlement_id: i64,
    ledger_id: i64,
    event_seq: u64,
) -> Result<()> {
    let row = sqlx::query("SELECT source,source_id,address_user,credit_pool,amount_msat FROM settled_payments WHERE id=?")
        .bind(settlement_id).fetch_one(&mut **tx).await?;
    let msat: i64 = row.try_get("amount_msat")?;
    if msat <= 0 || msat % 1000 != 0 {
        bail!("invalid native BTC settlement");
    }
    let sats = msat / 1000;
    let id = format!("btc:{settlement_id}");
    let source_id: String = row.try_get("source_id")?;
    sqlx::query(
        "INSERT INTO credit_valuations (
            id,asset,network,provider,account_scope,receive_scope,address_user,
            credit_pool,expected_atomic,target_sats,max_credit_sats,policy_version
        ) VALUES (?,'BTC','legacy-unspecified',?,'legacy-settled-payment',?,?,?,1,1,?,'btc-identity-v1')",
    )
    .bind(&id)
    .bind(row.try_get::<String, _>("source")?)
    .bind(&source_id)
    .bind(row.try_get::<String, _>("address_user")?)
    .bind(row.try_get::<String, _>("credit_pool")?)
    .bind(i64::MAX)
    .execute(&mut **tx)
    .await?;
    sqlx::query("INSERT INTO credit_allocations (valuation_id) VALUES (?)")
        .bind(&id)
        .execute(&mut **tx)
        .await?;
    let receipt = sqlx::query("INSERT INTO asset_receipts (valuation_id,receipt_key,amount_atomic,first_seen_at,unlocked,eligible,btc_settlement_id) VALUES (?,?,?,NULL,1,1,?)")
        .bind(&id).bind(source_id).bind(sats).bind(settlement_id).execute(&mut **tx).await?.last_insert_rowid();
    sqlx::query("INSERT INTO credit_grants (receipt_id,delta_sats,cumulative_atomic,cumulative_sats,ledger_entry_id,event_seq) VALUES (?,?,?,?,?,?)")
        .bind(receipt).bind(sats).bind(sats).bind(sats)
        .bind(ledger_id).bind(to_i64(event_seq,"event sequence")?).execute(&mut **tx).await?;
    sqlx::query(
        "UPDATE credit_allocations SET eligible_atomic=?,credited_sats=? WHERE valuation_id=?",
    )
    .bind(sats)
    .bind(sats)
    .bind(id)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

pub(super) async fn verify_native_btc(
    tx: &mut Transaction<'_, Sqlite>,
    settlement_id: i64,
) -> Result<()> {
    let valid: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM settled_payments p JOIN asset_receipts r ON r.btc_settlement_id=p.id JOIN credit_valuations v ON v.id=r.valuation_id JOIN credit_allocations a ON a.valuation_id=v.id JOIN credit_grants g ON g.receipt_id=r.id JOIN ledger_entries l ON l.id=g.ledger_entry_id WHERE p.id=? AND p.amount_msat%1000=0 AND v.asset='BTC' AND v.provider=p.source AND v.receive_scope=p.source_id AND v.address_user=p.address_user AND v.credit_pool=p.credit_pool AND v.expected_atomic=1 AND v.target_sats=1 AND r.receipt_key=p.source_id AND r.amount_atomic=p.amount_msat/1000 AND r.unlocked=1 AND r.eligible=1 AND a.eligible_atomic=r.amount_atomic AND a.credited_sats=r.amount_atomic AND a.hold_reason IS NULL AND g.delta_sats=r.amount_atomic AND g.cumulative_atomic=r.amount_atomic AND g.cumulative_sats=r.amount_atomic AND l.delta_sats=r.amount_atomic AND l.source_key='payment:'||p.source||':'||p.source_id AND l.entry_type='HERD_RECEIPT')")
        .bind(settlement_id).fetch_one(&mut **tx).await?;
    if !valid {
        bail!("native BTC receipt provenance is inconsistent");
    }
    Ok(())
}
