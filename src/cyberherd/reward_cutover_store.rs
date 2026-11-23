//! Uses reserved non-UUID audit keys, outside the signed HTTP command namespace.
//! No migration, core writes, funding reservation, transport or public message.
use super::Store;
use crate::cyberherd::{
    ingress::{CoreBinding, validate_owner},
    reward_cutover::{COMMAND, Decision, HistoricalFeeding, PREFIX, Receipt},
};
use anyhow::{Context, Result, ensure};
use sqlx::{Row as _, Sqlite, Transaction};
use uuid::Uuid;

async fn decision(tx: &mut Transaction<'_, Sqlite>, owner: &str) -> Result<Option<Decision>> {
    let row = sqlx::query(
        "SELECT input_json,result_json FROM operator_commands WHERE owner=? AND command_id=?",
    )
    .bind(owner)
    .bind(COMMAND)
    .fetch_optional(&mut **tx)
    .await?;
    row.map(|row| -> Result<_> {
        let raw: String = row.try_get("input_json")?;
        ensure!(raw.len() <= 4 * 1024 * 1024, "cutover audit exceeds bound");
        let value: Decision = serde_json::from_str(&raw)?;
        value.validate()?;
        ensure!(
            row.try_get::<String, _>("result_json")? == serde_json::to_string(&value.receipt())?,
            "cutover audit receipt differs"
        );
        Ok(value)
    })
    .transpose()
}

pub(super) async fn exclusion(
    tx: &mut Transaction<'_, Sqlite>,
    owner: &str,
    id: Uuid,
) -> Result<Option<HistoricalFeeding>> {
    let row = sqlx::query(
        "SELECT input_json,result_json FROM operator_commands WHERE owner=? AND command_id=?",
    )
    .bind(owner)
    .bind(format!("{PREFIX}{id}"))
    .fetch_optional(&mut **tx)
    .await?;
    let Some(row) = row else { return Ok(None) };
    let item: HistoricalFeeding = serde_json::from_str(&row.try_get::<String, _>("input_json")?)?;
    let cutover = decision(tx, owner)
        .await?
        .context("exclusion lost its cutover decision")?;
    ensure!(
        item.request_id == id
            && cutover.historical.contains(&item)
            && row.try_get::<String, _>("result_json")?
                == serde_json::to_string(&cutover.receipt())?,
        "historical exclusion audit differs"
    );
    Ok(Some(item))
}

pub(super) async fn validate_ingested(
    tx: &mut Transaction<'_, Sqlite>,
    owner: &str,
    epoch: Uuid,
    seq: u64,
    id: Uuid,
    proof: &str,
) -> Result<()> {
    let Some(cutover) = decision(tx, owner).await? else {
        return Ok(());
    };
    ensure!(
        cutover.epoch == epoch,
        "reward cutover belongs to another core epoch"
    );
    let excluded = exclusion(tx, owner, id).await?;
    if seq <= cutover.through {
        ensure!(
            excluded.is_some(),
            "historical feeding absent from approved inventory"
        );
    }
    if let Some(item) = excluded {
        ensure!(
            item.proof_hash == proof,
            "historical feeding contradicts cutover proof"
        );
    }
    Ok(())
}

impl Store {
    /// Trusted offline operation only: the qualified operator adapter must first
    /// verify complete core inventory and absence of core/provider claims. This
    /// domain method is not exposed through HTTP or the private payout bridge.
    pub async fn record_initial_reward_cutover(
        &self,
        owner: &str,
        value: &Decision,
    ) -> Result<Receipt> {
        validate_owner(owner)?;
        value.validate()?;
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let receipt = value.receipt();
        if let Some(old) = decision(&mut tx, owner).await? {
            ensure!(old == *value, "initial reward cutover cannot be replaced");
            for item in &value.historical {
                ensure!(
                    exclusion(&mut tx, owner, item.request_id).await?.as_ref() == Some(item),
                    "cutover exclusion missing on retry"
                );
            }
            tx.commit().await?;
            return Ok(receipt);
        }
        let row =
            sqlx::query("SELECT binding_json,received_through FROM core_streams WHERE owner=?")
                .bind(owner)
                .fetch_one(&mut *tx)
                .await?;
        let binding: CoreBinding =
            serde_json::from_str(&row.try_get::<String, _>("binding_json")?)?;
        binding.validate()?;
        ensure!(
            binding.epoch == value.epoch
                && value.through >= u64::try_from(row.try_get::<i64, _>("received_through")?)?,
            "cutover is stale or belongs to another epoch"
        );
        let known = sqlx::query("SELECT source_key,proof_hash FROM core_sources WHERE owner=? AND source_key LIKE 'feeding:%'")
            .bind(owner).fetch_all(&mut *tx).await?;
        for row in known {
            let key: String = row.try_get("source_key")?;
            let id: Uuid = key
                .strip_prefix("feeding:")
                .context("invalid feeding key")?
                .parse()?;
            ensure!(
                value.historical.iter().any(|item| item.request_id == id
                    && item.proof_hash == row.get::<String, _>("proof_hash")),
                "cutover omits or changes received historical feeding"
            );
        }
        let conflicting: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM reward_plans WHERE owner=? AND (state!='no_payout' OR reservation_json IS NOT NULL))")
            .bind(owner).fetch_one(&mut *tx).await?;
        ensure!(
            !conflicting,
            "existing reward authority requires explicit reconciliation"
        );
        let result = serde_json::to_string(&receipt)?;
        sqlx::query("INSERT INTO operator_commands(owner,command_id,input_json,result_json) VALUES(?,?,?,?)")
            .bind(owner).bind(COMMAND).bind(serde_json::to_string(value)?).bind(&result).execute(&mut *tx).await?;
        for item in &value.historical {
            sqlx::query("INSERT INTO operator_commands(owner,command_id,input_json,result_json) VALUES(?,?,?,?)")
                .bind(owner).bind(format!("{PREFIX}{}",item.request_id)).bind(serde_json::to_string(item)?).bind(&result).execute(&mut *tx).await?;
        }
        tx.commit().await?;
        Ok(receipt)
    }
}
