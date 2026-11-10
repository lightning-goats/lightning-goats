use super::{
    Store,
    invoices::{touch_clock, write_lock},
    outcomes::load_manifest,
};
use crate::{core_bridge::AuthenticatedFailedCost, payout::cost_recovery::Receipt};
use anyhow::{Context as _, Result, bail};
impl Store {
    #[cfg(unix)]
    pub async fn connect_existing_recovery(
        database_url: &str,
        policy: super::Policy,
    ) -> Result<Self> {
        policy.validate()?;
        crate::payout::cost_recovery::existing_private_database(database_url)?;
        let pool = crate::sqlite::connect_existing_durable(database_url, 1).await?;
        crate::sqlite::require_current_schema(&pool, &super::MIGRATOR).await?;
        let saved: String =
            sqlx::query_scalar("SELECT scope_json FROM payout_binding WHERE singleton=1")
                .fetch_one(&pool)
                .await?;
        if saved != serde_json::to_string(&policy.scope)? {
            bail!("payout recovery scope changed");
        }
        Ok(Self { pool, policy })
    }
    /// Recovery-only read/ack: no sender credential, signing or provider execute.
    #[cfg(unix)]
    pub async fn sync_failed_cost(
        &self,
        core: &crate::core_bridge::unix::Client,
        grant: uuid::Uuid,
        member: &str,
    ) -> Result<Option<Receipt>> {
        let mut tx = self.pool.begin().await?;
        let (header, _) = load_manifest(&mut tx, grant)
            .await?
            .context("outcome manifest is absent")?;
        tx.commit().await?;
        let authenticated = core.failed_cost(&header, member).await?;
        self.acknowledge_failed_cost(
            grant,
            member,
            &authenticated,
            crate::cyberherd::calendar::now()?,
        )
        .await
    }
    pub(crate) async fn acknowledge_failed_cost(
        &self,
        grant: uuid::Uuid,
        member: &str,
        authenticated: &AuthenticatedFailedCost,
        now: i64,
    ) -> Result<Option<Receipt>> {
        if authenticated.core_uid() != self.policy.scope.core_uid {
            bail!("cost receipt has a different core peer");
        }
        let Some(receipt) = authenticated.receipt() else {
            return Ok(None);
        };
        let mut tx = self.pool.begin().await?;
        write_lock(&mut tx).await?;
        let (header, rows) = load_manifest(&mut tx, grant)
            .await?
            .context("outcome manifest is absent")?;
        let row = rows
            .iter()
            .find(|r| r.member == member)
            .context("failed cost member is absent")?;
        receipt.validate(&header, row)?;
        let original_json: String =
            sqlx::query_scalar("SELECT progress_json FROM payout_outcome_acks WHERE grant_id=?")
                .bind(grant.to_string())
                .fetch_one(&mut *tx)
                .await?;
        let original: crate::payout::outcome::Progress = serde_json::from_str(&original_json)?;
        original.validate(&header)?;
        if !original.finalized
            || original.held_sats < receipt.decision.gross_sats
            || now < receipt.verified_at
        {
            bail!("cost receipt has no original final hold");
        }
        let encoded = serde_json::to_string(receipt)?;
        if let Some(old) = sqlx::query_scalar::<_, String>(
            "SELECT document_json FROM payout_cost_acks WHERE grant_id=? AND member=?",
        )
        .bind(grant.to_string())
        .bind(member)
        .fetch_optional(&mut *tx)
        .await?
        {
            if old != encoded {
                bail!("core cost receipt changed");
            }
            tx.commit().await?;
            return Ok(Some(receipt.clone()));
        }
        touch_clock(&mut tx, now).await?;
        sqlx::query("INSERT INTO payout_cost_acks(grant_id,member,id,document_json,gross_sats,spent_sats,accepted_at) VALUES(?,?,?,?,?,?,?)")
            .bind(grant.to_string()).bind(member).bind(receipt.decision.id.to_string()).bind(encoded).bind(i64::try_from(receipt.decision.gross_sats)?).bind(i64::try_from(receipt.decision.actual_cost_sats)?).bind(now).execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(Some(receipt.clone()))
    }
}

impl Store {
    /// Conservative outstanding holds plus spending inside the rolling window.
    /// This is a report, not execution permission or an available-wallet balance.
    pub async fn budget_usage_sats(&self) -> Result<u64> {
        let now = crate::cyberherd::calendar::now()?;
        let mut tx = self.pool.begin().await?;
        let previous: i64 =
            sqlx::query_scalar("SELECT observed_at FROM payout_clock WHERE singleton=1")
                .fetch_one(&mut *tx)
                .await?;
        if now < previous {
            bail!("payout clock moved backwards");
        }
        let usage = super::outcomes::usage(&mut tx, now).await?;
        tx.commit().await?;
        Ok(usage)
    }
}
