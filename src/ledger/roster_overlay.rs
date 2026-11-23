use super::{LedgerStore, append_event_in_transaction, to_u64};
use crate::cyberherd::view::Overlay;
use anyhow::{Result, bail};
use sqlx::Row as _;

impl LedgerStore {
    /// Display-only intake capability. It can never import food, money, resets
    /// or a Nostr job. Revision fencing prevents stale cards after a newer state.
    pub(crate) async fn accept_roster_overlay(&self, message: &Overlay) -> Result<u64> {
        message.validate()?;
        let identity = message.identity()?;
        let document = serde_json::to_string(message)?;
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let epoch: String =
            sqlx::query_scalar("SELECT stream_id FROM overlay_identity WHERE singleton=1")
                .fetch_one(&mut *tx)
                .await?;
        if epoch != message.epoch.to_string() {
            bail!("roster belongs to another core epoch");
        }
        let roster = &message.roster;
        if let Some(row)=sqlx::query("SELECT document_json,event_seq FROM roster_overlay_receipts WHERE epoch=? AND owner=? AND identity=?")
            .bind(&epoch).bind(&roster.owner).bind(&identity).fetch_optional(&mut *tx).await? {
            if row.try_get::<String,_>("document_json")?!=document {bail!("roster retry changed original bytes");}
            let seq=to_u64(row.try_get("event_seq")?,"roster receipt")?;tx.commit().await?;return Ok(seq);
        }
        if let Some(row) = sqlx::query(
            "SELECT document_json FROM roster_overlay_state WHERE singleton=1 AND owner=?",
        )
        .bind(&roster.owner)
        .fetch_optional(&mut *tx)
        .await?
        {
            let previous: Overlay =
                serde_json::from_str(&row.try_get::<String, _>("document_json")?)?;
            if previous.epoch == message.epoch
                && (roster.revision <= previous.roster.revision
                    || roster.cycle < previous.roster.cycle
                    || roster.cycle_start < previous.roster.cycle_start
                    || (roster.cycle == previous.roster.cycle
                        && roster.cycle_start != previous.roster.cycle_start))
            {
                bail!("roster revision/cycle cannot rewind or change in place");
            }
        }
        let seq = append_event_in_transaction(&mut tx, "cyberherd_roster", roster).await?;
        sqlx::query("INSERT INTO roster_overlay_receipts(epoch,owner,identity,document_json,event_seq) VALUES(?,?,?,?,?)")
            .bind(epoch).bind(&roster.owner).bind(identity).bind(&document).bind(i64::try_from(seq)?).execute(&mut *tx).await?;
        sqlx::query("INSERT INTO roster_overlay_state(singleton,owner,cycle,revision,document_json,event_seq) VALUES(1,?,?,?,?,?) ON CONFLICT(singleton) DO UPDATE SET owner=excluded.owner,cycle=excluded.cycle,revision=excluded.revision,document_json=excluded.document_json,event_seq=excluded.event_seq")
            .bind(&roster.owner).bind(roster.cycle).bind(roster.revision).bind(document).bind(i64::try_from(seq)?).execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(seq)
    }
}
