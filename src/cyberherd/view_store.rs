use super::{Store, decode_snapshot};
use crate::cyberherd::{
    calendar::DayWindow,
    targets::{ConversationTarget, TargetPolicy},
    view::{Overlay, Roster},
};
use anyhow::{Result, bail};
use sqlx::Row as _;

impl Store {
    pub(crate) async fn observe_view_discovery(
        &self,
        owner: &str,
        available: bool,
        now: i64,
    ) -> Result<()> {
        if now < 0 {
            bail!("invalid discovery observation time");
        }
        sqlx::query("INSERT INTO view_discovery(owner,observed_at,available) VALUES(?,?,?) ON CONFLICT(owner) DO UPDATE SET observed_at=excluded.observed_at,available=excluded.available WHERE excluded.observed_at>=view_discovery.observed_at")
            .bind(owner).bind(now).bind(available).execute(&self.pool).await?;
        Ok(())
    }
    pub async fn public_roster(&self, owner: &str) -> Result<Roster> {
        Roster::from_snapshot(&self.snapshot(owner).await?)
    }
    pub(crate) async fn public_targets(
        &self,
        policy: &TargetPolicy,
        timezone: &str,
        now: i64,
    ) -> Result<Option<Vec<ConversationTarget>>> {
        let row = sqlx::query("SELECT available,observed_at FROM view_discovery WHERE owner=?")
            .bind(&policy.project_pubkey)
            .fetch_optional(&self.pool)
            .await?;
        let available = row
            .map(|r| -> Result<bool> {
                Ok(r.try_get::<bool, _>("available")?
                    && (0..=60).contains(&now.saturating_sub(r.try_get("observed_at")?)))
            })
            .transpose()?
            .unwrap_or(false);
        if !available {
            return Ok(None);
        }
        Ok(Some(
            self.target_projection(
                policy,
                &DayWindow::at(timezone, now)?,
                &[],
                Default::default(),
                now,
            )
            .await?
            .current(),
        ))
    }
    pub(crate) async fn roster_needs_delivery(&self, message: &Overlay) -> Result<bool> {
        message.validate()?;
        let previous: Option<String> =
            sqlx::query_scalar("SELECT document_json FROM roster_delivery WHERE owner=?")
                .bind(&message.roster.owner)
                .fetch_optional(&self.pool)
                .await?;
        Ok(previous.as_deref() != Some(serde_json::to_string(message)?.as_str()))
    }
    pub(crate) async fn acknowledge_roster(&self, message: &Overlay, seq: u64) -> Result<()> {
        message.validate()?;
        if seq == 0 || seq > i64::MAX as u64 {
            bail!("invalid roster acknowledgment");
        }
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let owner = &message.roster.owner;
        let current = decode_snapshot(
            &sqlx::query("SELECT * FROM owners WHERE owner=?")
                .bind(owner)
                .fetch_one(&mut *tx)
                .await?,
        )?;
        if current.revision < message.roster.revision {
            bail!("roster acknowledgment exceeds local revision");
        }
        if let Some(row) =
            sqlx::query("SELECT document_json,event_seq FROM roster_delivery WHERE owner=?")
                .bind(owner)
                .fetch_optional(&mut *tx)
                .await?
        {
            let old: Overlay = serde_json::from_str(&row.try_get::<String, _>("document_json")?)?;
            if old.epoch != message.epoch
                || old.roster.revision > message.roster.revision
                || (old.roster.revision == message.roster.revision
                    && (old != *message || row.try_get::<i64, _>("event_seq")? != seq as i64))
            {
                bail!("roster acknowledgment rewound or changed");
            }
        }
        sqlx::query("INSERT INTO roster_delivery(owner,document_json,event_seq) VALUES(?,?,?) ON CONFLICT(owner) DO UPDATE SET document_json=excluded.document_json,event_seq=excluded.event_seq")
            .bind(owner).bind(serde_json::to_string(message)?).bind(seq as i64).execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(())
    }
}
