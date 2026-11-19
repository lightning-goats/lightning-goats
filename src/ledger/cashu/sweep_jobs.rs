//! Source-bound sweep planning. No donation credit or provider request is created.
use super::super::{LedgerStore, to_u64};
use super::{treasury::require_epoch, treasury_planning::reserve_first};
use crate::cashu::{receipt_authority::ReceiptPolicy, treasury::Policy, treasury_sweep};
use anyhow::{Context, Result, ensure};
use lightning_goats_cashu_receipt::treasury::{Intent, MAX_SOURCES};
use sqlx::{Row, Sqlite, Transaction};
use std::collections::BTreeMap;

struct Available {
    parts: Vec<(i64, u64)>,
    balance: u64,
    oldest: u64,
}
async fn available(
    tx: &mut Transaction<'_, Sqlite>,
    registration: &ReceiptPolicy,
    mint: &str,
    largest_first: bool,
) -> Result<Option<Available>> {
    let rows = sqlx::query(
        "WITH custody AS (
            SELECT c.ledger_entry_id,c.accepted_at,c.net_sats-COALESCE((
                SELECT SUM(CASE WHEN t.intent_id IS NULL OR r.ledger_id IS NULL
                                THEN s.input_sats ELSE r.input_sats-r.change_sats END)
                FROM cashu_treasury_sources s
                LEFT JOIN cashu_treasury_completions t ON t.intent_id=s.intent_id
                LEFT JOIN cashu_treasury_source_results r
                    ON r.intent_id=s.intent_id AND r.ledger_id=s.ledger_id
                WHERE s.ledger_id=c.ledger_entry_id
            ),0) AS available
            FROM cashu_receipt_inbox c
            WHERE c.wallet_instance=? AND c.wallet_epoch=? AND c.core_epoch=? AND c.mint_id=?
        ) SELECT *,SUM(available) OVER() AS balance,MIN(available) OVER() AS minimum,
                 MIN(accepted_at) OVER() AS oldest
          FROM custody WHERE available!=0
          ORDER BY CASE WHEN ? THEN available END DESC,ledger_entry_id LIMIT ?",
    )
    .bind(registration.wallet_instance.to_string())
    .bind(registration.wallet_epoch.to_string())
    .bind(registration.core_epoch.to_string())
    .bind(mint)
    .bind(largest_first)
    .bind(MAX_SOURCES as i64)
    .fetch_all(&mut **tx)
    .await?;
    let Some(first) = rows.first() else {
        return Ok(None);
    };
    ensure!(
        first.try_get::<i64, _>("minimum")? > 0,
        "sweep custody over-allocation"
    );
    let balance = to_u64(first.try_get("balance")?, "available mint custody")?;
    let oldest = to_u64(first.try_get("oldest")?, "oldest available custody")?;
    let mut parts = rows
        .into_iter()
        .map(|r| {
            Ok((
                r.try_get("ledger_entry_id")?,
                to_u64(r.try_get("available")?, "available source custody")?,
            ))
        })
        .collect::<Result<Vec<_>>>()?;
    if largest_first {
        parts.sort_unstable_by_key(|(ledger, _)| *ledger);
    }
    Ok(Some(Available {
        parts,
        balance,
        oldest,
    }))
}

impl LedgerStore {
    /// Protected local operator planning. No invoice, provider or feeder calls.
    /// Preview rolls back the same source-selection transaction used by apply.
    pub async fn plan_operator_cashu_settlement(
        &self,
        registration: &ReceiptPolicy,
        policy: &Policy,
        request: &treasury_sweep::Request,
        apply: bool,
    ) -> Result<Option<Intent>> {
        ensure!(
            registration.wallet_uid > 0
                && !registration.wallet_instance.is_nil()
                && !registration.wallet_epoch.is_nil()
                && !registration.core_epoch.is_nil(),
            "operator settlement wallet registration is absent"
        );
        policy.validate()?;
        ensure!(policy.enabled, "treasury planning is disabled");
        let mint = policy
            .approved_mints
            .get(&request.mint_id)
            .context("operator settlement mint is not approved")?;
        ensure!(
            request.maximum_input_sats <= policy.limits.maximum_input_sats
                && request.maximum_input_sats <= mint.maximum_input_sats,
            "operator settlement exceeds protected input ceiling"
        );
        let sweep = treasury_sweep::Policy {
            enabled: true,
            requests: vec![request.clone()],
            ..Default::default()
        };
        self.plan_cashu_sweep_mode(registration, policy, &sweep, apply, true)
            .await
    }

    pub(crate) async fn plan_cashu_sweep(
        &self,
        registration: &ReceiptPolicy,
        policy: &Policy,
        sweep: &treasury_sweep::Policy,
    ) -> Result<Option<Intent>> {
        self.plan_cashu_sweep_mode(registration, policy, sweep, true, false)
            .await
    }

    async fn plan_cashu_sweep_mode(
        &self,
        registration: &ReceiptPolicy,
        policy: &Policy,
        sweep: &treasury_sweep::Policy,
        apply: bool,
        replay_assigned: bool,
    ) -> Result<Option<Intent>> {
        policy.validate()?;
        sweep.validate(policy)?;
        if !policy.enabled || !sweep.enabled {
            return Ok(None);
        }
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        require_epoch(&mut tx, registration.core_epoch).await?;
        let now: i64 = sqlx::query_scalar("SELECT unixepoch()")
            .fetch_one(&mut *tx)
            .await?;
        let now = to_u64(now, "sweep time")?;
        for request in &sweep.requests {
            let encoded = serde_json::to_string(&(
                registration.core_epoch,
                registration.wallet_instance,
                registration.wallet_epoch,
                request,
            ))?;
            ensure!(
                encoded.len() <= 4096,
                "operator sweep request exceeds bound"
            );
            let saved: Option<String> =
                sqlx::query_scalar("SELECT request_json FROM cashu_sweep_requests WHERE id=?")
                    .bind(request.id.to_string())
                    .fetch_optional(&mut *tx)
                    .await?;
            if let Some(saved) = saved {
                ensure!(saved == encoded, "operator sweep identity changed");
            } else {
                sqlx::query("INSERT INTO cashu_sweep_requests(id,request_json) VALUES(?,?)")
                    .bind(request.id.to_string())
                    .bind(encoded)
                    .execute(&mut *tx)
                    .await?;
            }
            let assigned: bool = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM cashu_sweep_jobs WHERE request_id=?)",
            )
            .bind(request.id.to_string())
            .fetch_one(&mut *tx)
            .await?;
            if assigned {
                if replay_assigned {
                    let id: String = sqlx::query_scalar(
                        "SELECT intent_id FROM cashu_sweep_jobs WHERE request_id=?",
                    )
                    .bind(request.id.to_string())
                    .fetch_one(&mut *tx)
                    .await?;
                    let original =
                        super::treasury::load_intent(&mut tx, uuid::Uuid::parse_str(&id)?).await?;
                    ensure!(original.is_some(), "operator settlement intent is missing");
                    finish(tx, apply).await?;
                    return Ok(original);
                }
                continue;
            }
            let Some(mut funds) = available(&mut tx, registration, &request.mint_id, false).await?
            else {
                continue;
            };
            let mut left = request.maximum_input_sats;
            for (_, sats) in &mut funds.parts {
                *sats = (*sats).min(left);
                left -= *sats;
            }
            funds.parts.retain(|(_, sats)| *sats > 0);
            let candidate = BTreeMap::from([(request.mint_id.clone(), funds.parts)]);
            if let Some(intent) = reserve_first(&mut tx, registration, policy, candidate).await? {
                let evidence = serde_json::json!({"request_id":request.id}).to_string();
                sqlx::query("INSERT INTO cashu_sweep_jobs(intent_id,reason,request_id,trigger_json) VALUES(?,'operator',?,?)")
                    .bind(intent.id.to_string()).bind(request.id.to_string()).bind(evidence).execute(&mut *tx).await?;
                finish(tx, apply).await?;
                return Ok(Some(intent));
            }
        }
        let mut candidates = BTreeMap::new();
        let mut reasons = BTreeMap::new();
        for mint in policy.approved_mints.keys() {
            let Some(mut funds) = available(&mut tx, registration, mint, false).await? else {
                continue;
            };
            ensure!(now >= funds.oldest, "sweep receipt time is in the future");
            let age = now - funds.oldest;
            let reason = if sweep.maximum_holding_age_seconds > 0
                && age >= sweep.maximum_holding_age_seconds
            {
                "age"
            } else if sweep.balance_trigger_sats > 0 && funds.balance >= sweep.balance_trigger_sats
            {
                // A prefix of small receipts can hide a later receipt that would
                // let this batch reach the trigger within the wire source limit.
                if funds.parts.len() == MAX_SOURCES
                    && funds.parts.iter().map(|(_, sats)| sats).sum::<u64>()
                        < sweep.balance_trigger_sats
                {
                    funds = available(&mut tx, registration, mint, true)
                        .await?
                        .context("sweep custody disappeared within transaction")?;
                }
                "balance"
            } else {
                continue;
            };
            let evidence=serde_json::json!({"available_sats":funds.balance,"oldest_age_seconds":age,"maximum_holding_age_seconds":sweep.maximum_holding_age_seconds,"balance_trigger_sats":sweep.balance_trigger_sats}).to_string();
            candidates.insert(mint.clone(), funds.parts);
            reasons.insert(mint.clone(), (reason, evidence));
        }
        if let Some(intent) = reserve_first(&mut tx, registration, policy, candidates).await? {
            let (reason, evidence) = reasons
                .get(&intent.mint_id)
                .context("sweep trigger scope changed")?;
            sqlx::query("INSERT INTO cashu_sweep_jobs(intent_id,reason,request_id,trigger_json) VALUES(?,?,NULL,?)")
                .bind(intent.id.to_string()).bind(*reason).bind(evidence).execute(&mut *tx).await?;
            finish(tx, apply).await?;
            return Ok(Some(intent));
        }
        finish(tx, apply).await?;
        Ok(None)
    }
}

async fn finish(tx: Transaction<'_, Sqlite>, apply: bool) -> Result<()> {
    if apply {
        tx.commit().await?;
    } else {
        tx.rollback().await?;
    }
    Ok(())
}
