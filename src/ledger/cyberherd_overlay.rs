use super::{LedgerStore, append_event_in_transaction, to_u64};
use crate::{config::RuntimeMode, core_bridge::Availability, cyberherd::overlay_delivery::Message};
use anyhow::{Result, bail};
use sqlx::Row as _;

impl LedgerStore {
    pub(crate) async fn accept_member_overlay(
        &self,
        message: &Message,
        mode: RuntimeMode,
    ) -> Result<u64> {
        message.validate()?;
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let epoch: String =
            sqlx::query_scalar("SELECT stream_id FROM overlay_identity WHERE singleton=1")
                .fetch_one(&mut *tx)
                .await?;
        if epoch != message.epoch.to_string() {
            bail!("member overlay belongs to another core epoch");
        }
        let document = serde_json::to_string(message)?;
        if let Some(row)=sqlx::query("SELECT document_json,event_seq FROM cyberherd_overlay_receipts WHERE epoch=? AND owner=? AND identity=?")
            .bind(&epoch).bind(&message.owner).bind(&message.identity).fetch_optional(&mut *tx).await? {
            if row.try_get::<String,_>("document_json")?!=document {bail!("member overlay retry changed its frozen message");}
            let seq=to_u64(row.try_get("event_seq")?,"member overlay sequence")?;
            tx.commit().await?;return Ok(seq);
        }
        // Avoid a separate pool read while holding the write transaction: use
        // the same freshness/ready contract over this snapshot's status row.
        if !mode.feeder_enabled() {
            bail!("member thread presentation is paused");
        }
        let row = sqlx::query(
            "SELECT paused,reason,observed_at FROM feeding_presentation WHERE singleton=1",
        )
        .fetch_one(&mut *tx)
        .await?;
        let now = crate::cyberherd::calendar::now()?;
        let availability = if !row.try_get::<bool, _>("paused")?
            && row.try_get::<String, _>("reason")? == "ready"
        {
            Availability::Ready
        } else {
            Availability::Unknown
        };
        let state = crate::core_bridge::Safety {
            availability,
            observed_at: Some(row.try_get("observed_at")?),
            nostr_enabled: mode.nostr_enabled(),
        };
        if state.current_availability(now) != Availability::Ready {
            bail!("member thread presentation awaits a confirmed unpaused feeder");
        }
        let seq = append_event_in_transaction(
            &mut tx,
            "cyberherd_message",
            &serde_json::json!({
                "message":message.message,"category":message.category,"message_id":message.identity
            }),
        )
        .await?;
        sqlx::query("INSERT INTO cyberherd_overlay_receipts(epoch,owner,identity,document_json,event_seq) VALUES(?,?,?,?,?)")
            .bind(epoch).bind(&message.owner).bind(&message.identity).bind(document).bind(i64::try_from(seq)?).execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(seq)
    }
}
