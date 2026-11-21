//! Shared bounded admission for feed settlement and treasury sweeps.
use super::super::to_u64;
use super::treasury::{TreasuryPlan, daily_fee_remaining, reserve_in_transaction};
use crate::cashu::{receipt_authority::ReceiptPolicy, treasury::Policy};
use anyhow::{Context, Result};
use lightning_goats_cashu_receipt::treasury::{Intent, MAX_SOURCES};
use sqlx::{Row, Sqlite, Transaction};
use std::collections::BTreeMap;
use uuid::Uuid;
pub(super) async fn reserve_first(
    tx: &mut Transaction<'_, Sqlite>,
    registration: &ReceiptPolicy,
    policy: &Policy,
    by_mint: BTreeMap<String, Vec<(i64, u64)>>,
) -> Result<Option<Intent>> {
    let Some(daily_fee_limit) = policy.limits.fee_sats_per_24h else {
        return Ok(None);
    };
    let now: i64 = sqlx::query_scalar("SELECT unixepoch()")
        .fetch_one(&mut **tx)
        .await?;
    let daily = sqlx::query("SELECT COUNT(*) AS count,COALESCE(SUM(input_sats),0) AS sats FROM cashu_treasury_intents WHERE created_at>=?")
            .bind(now.saturating_sub(86_400)).fetch_one(&mut **tx).await?;
    if !policy
        .limits
        .daily_operation_allowed(to_u64(daily.try_get("count")?, "treasury daily count")?)
    {
        return Ok(None);
    }
    let pending: i64 = sqlx::query_scalar(concat!(include_str!("treasury_balances.sql"), "SELECT COALESCE(SUM(i.input_sats),0) FROM cashu_treasury_intents i WHERE NOT EXISTS(SELECT 1 FROM settled_intents c WHERE c.intent_id=i.id)"))
            .fetch_one(&mut **tx).await?;
    let global_available = policy
        .limits
        .maximum_in_flight_sats
        .saturating_sub(to_u64(pending, "treasury in-flight amount")?)
        .min(
            policy
                .limits
                .daily_input_remaining(to_u64(daily.try_get("sats")?, "treasury daily amount")?),
        );
    let fees_available = daily_fee_remaining(tx, now, daily_fee_limit).await?;
    let mut ordered = Vec::new();
    for (mint_id, parts) in by_mint {
        if !policy.approved_mints.contains_key(&mint_id) {
            continue;
        }
        let last: i64 = sqlx::query_scalar(
            "SELECT COALESCE(MAX(rowid),0) FROM cashu_treasury_intents WHERE mint_id=?",
        )
        .bind(&mint_id)
        .fetch_one(&mut **tx)
        .await?;
        ordered.push((last, mint_id, parts));
    }
    ordered.sort_by(|a, b| (&a.0, &a.1).cmp(&(&b.0, &b.1)));
    for (_, mint_id, parts) in ordered {
        let Some(mint) = policy.approved_mints.get(&mint_id) else {
            continue;
        };
        let mint_pending:i64=sqlx::query_scalar(concat!(include_str!("treasury_balances.sql"), "SELECT COALESCE(SUM(i.input_sats),0) FROM cashu_treasury_intents i WHERE i.mint_id=? AND NOT EXISTS(SELECT 1 FROM settled_intents c WHERE c.intent_id=i.id)"))
                .bind(&mint_id).fetch_one(&mut **tx).await?;
        let mint_available = mint
            .maximum_in_flight_sats
            .saturating_sub(to_u64(mint_pending, "mint in-flight amount")?);
        let limits = &policy.limits;
        let minimum = limits.minimum_deposit_sats.max(mint.minimum_deposit_sats);
        let fee_ceiling = limits
            .maximum_fee_sats
            .min(mint.maximum_fee_sats)
            .min(daily_fee_limit);
        let maximum = limits
            .maximum_input_sats
            .min(mint.maximum_input_sats)
            .min(global_available)
            .min(mint_available)
            .min(
                limits
                    .maximum_deposit_sats
                    .min(mint.maximum_deposit_sats)
                    .checked_add(fee_ceiling)
                    .context("planner amount overflow")?,
            );
        let mut selected = Vec::new();
        let mut total = 0u64;
        for (ledger, sats) in parts.into_iter().take(MAX_SOURCES) {
            let take = sats.min(maximum - total);
            if take > 0 {
                selected.push((ledger, take));
                total = total.checked_add(take).context("planner input overflow")?;
            }
            if total == maximum {
                break;
            }
        }
        if total < minimum {
            continue;
        }
        let fee = fee_ceiling.min(total - minimum);
        if fee > fees_available {
            continue;
        }
        let request = TreasuryPlan {
            id: Uuid::new_v4(),
            mint_id,
            sources: selected,
            deposit_sats: total - fee,
            maximum_fee_sats: fee,
        };
        return Ok(Some(
            reserve_in_transaction(tx, registration, policy, &request).await?,
        ));
    }
    Ok(None)
}
