//! Verified result intake freezes the original plan breakdown with each member
//! result. Delivery may wait for joining receipts without blocking money.
use super::{Store, reward_state::decode};
use crate::{
    cyberherd::{ingress::CoreBinding, rewards::Row as PlannedRow},
    payout::results::{AuthenticatedPage, Status},
    reward_funding::Reservation,
};
use anyhow::{Context, Result, bail};
use sqlx::Row as _;

impl Store {
    /// Round-robin pending plans: an unfinalized older payout cannot starve
    /// later verified results. This selection grants no money or public post.
    pub(crate) async fn next_reward_result(
        &self,
        owner: &str,
    ) -> Result<Option<(Reservation, u16)>> {
        crate::nostr_event::validate_public_key(owner)?;
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let row = sqlx::query("SELECT p.*,COALESCE(c.through,0) AS result_through FROM reward_plans p JOIN feeding_barriers b ON b.owner=p.owner AND b.request_id=p.request_id LEFT JOIN reward_result_cursors c ON c.owner=p.owner AND c.request_id=p.request_id WHERE p.owner=? AND p.state='reserved' AND COALESCE(c.complete,0)=0 ORDER BY COALESCE(c.poll_order,0),b.ingress_seq LIMIT 1")
            .bind(owner).fetch_optional(&mut *tx).await?;
        let Some(row) = row else { return Ok(None) };
        let plan = decode(&row)?;
        let reservation = plan
            .reservation
            .context("reserved plan lost its reservation")?;
        let cursor = u16::try_from(row.try_get::<i64, _>("result_through")?)?;
        let order: i64 =
            sqlx::query_scalar("SELECT COALESCE(MAX(poll_order),0) FROM reward_result_cursors")
                .fetch_one(&mut *tx)
                .await?;
        let order = order
            .checked_add(1)
            .context("result scheduling clock exhausted")?;
        sqlx::query("INSERT INTO reward_result_cursors(owner,request_id,poll_order) VALUES(?,?,?) ON CONFLICT(owner,request_id) DO UPDATE SET poll_order=excluded.poll_order")
            .bind(owner).bind(reservation.request_id.to_string()).bind(order).execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(Some((reservation, cursor)))
    }

    pub async fn record_reward_results(&self, received: &AuthenticatedPage) -> Result<()> {
        let page = received.page();
        page.validate()?;
        let reservation = &page.reservation;
        let owner = &reservation.owner;
        let request = reservation.request_id.to_string();
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let binding: String =
            sqlx::query_scalar("SELECT binding_json FROM core_streams WHERE owner=?")
                .bind(owner)
                .fetch_one(&mut *tx)
                .await?;
        let binding: CoreBinding = serde_json::from_str(&binding)?;
        binding.validate()?;
        if binding.core_uid != received.core_uid() || binding.epoch != reservation.epoch {
            bail!("verified result has wrong authenticated core binding");
        }
        let raw = sqlx::query("SELECT * FROM reward_plans WHERE owner=? AND request_id=?")
            .bind(owner)
            .bind(&request)
            .fetch_one(&mut *tx)
            .await?;
        let plan = decode(&raw)?;
        if plan.reservation.as_ref() != Some(reservation) || plan.hash != reservation.plan_hash {
            bail!("verified results contradict original reserved plan");
        }
        let mut payable: Vec<&PlannedRow> = plan
            .plan
            .rows
            .iter()
            .filter(|row| row.skip.is_none())
            .collect();
        payable.sort_by(|a, b| a.pubkey.cmp(&b.pubkey));
        if payable.len() != usize::from(page.total_rows) {
            bail!("verified result roster differs from original payable rows");
        }
        for (result, planned) in page
            .rows
            .iter()
            .zip(payable.iter().skip(usize::from(page.after)))
        {
            if result.member != planned.pubkey || result.gross_sats != planned.gross_sats {
                bail!("verified result changed frozen member allocation");
            }
            if let Status::Completed { net_sats, fee_sats } = result.result {
                let valid = if plan.plan.planning_policy.fee_mode.is_fixed() {
                    net_sats == planned.net_sats && fee_sats <= planned.fee_limit_sats
                } else {
                    net_sats >= plan.plan.planning_policy.minimum_net_sats
                        && net_sats <= planned.net_sats
                        && net_sats
                            .checked_add(fee_sats)
                            .is_some_and(|total| total <= planned.gross_sats)
                        && planned.recipient.as_ref().is_some_and(|r| {
                            net_sats
                                .checked_mul(1000)
                                .is_some_and(|msat| msat >= r.min_msat && msat <= r.max_msat)
                        })
                };
                if !valid {
                    bail!("verified payment changed planned amount/fee ceiling");
                }
            }
        }
        let json = serde_json::to_string(page)?;
        if let Some(old) = sqlx::query_scalar::<_,String>("SELECT page_json FROM reward_result_pages WHERE owner=? AND request_id=? AND after_index=?")
            .bind(owner).bind(&request).bind(i64::from(page.after)).fetch_optional(&mut *tx).await? {
            if old != json { bail!("verified result retry changed original receipt"); }
            tx.commit().await?;
            return Ok(());
        }
        sqlx::query("INSERT INTO reward_result_cursors(owner,request_id) VALUES(?,?) ON CONFLICT(owner,request_id) DO NOTHING")
            .bind(owner).bind(&request).execute(&mut *tx).await?;
        let cursor =
            sqlx::query("SELECT * FROM reward_result_cursors WHERE owner=? AND request_id=?")
                .bind(owner)
                .bind(&request)
                .fetch_one(&mut *tx)
                .await?;
        if cursor.try_get::<i64, _>("through")? != i64::from(page.after)
            || cursor.try_get::<bool, _>("complete")?
            || cursor
                .try_get::<Option<String>, _>("manifest_hash")?
                .is_some_and(|h| h != page.manifest_hash)
            || cursor
                .try_get::<Option<i64>, _>("total_rows")?
                .is_some_and(|n| n != i64::from(page.total_rows))
        {
            bail!("verified result page skips or changes durable cursor");
        }
        sqlx::query("INSERT INTO reward_result_pages(owner,request_id,after_index,page_json) VALUES(?,?,?,?)")
            .bind(owner).bind(&request).bind(i64::from(page.after)).bind(json).execute(&mut *tx).await?;
        for (result, planned) in page
            .rows
            .iter()
            .zip(payable.iter().skip(usize::from(page.after)))
        {
            let payload = serde_json::json!({"request_id":reservation.request_id,
                "result":result,"breakdown":planned,"manifest_hash":page.manifest_hash,
                "roster_revision":plan.plan.roster_revision});
            sqlx::query("INSERT INTO domain_events(owner,cycle,source_id,event_type,snapshot_json) VALUES(?,?,?,'payout_result',?)")
                .bind(owner).bind(plan.plan.cycle).bind(format!("payout:{}:{}",reservation.id,result.member))
                .bind(serde_json::to_string(&payload)?).execute(&mut *tx).await?;
        }
        sqlx::query("UPDATE reward_result_cursors SET manifest_hash=?,total_rows=?,through=?,complete=? WHERE owner=? AND request_id=?")
            .bind(&page.manifest_hash).bind(i64::from(page.total_rows)).bind(i64::from(page.through))
            .bind(page.complete()).bind(owner).bind(&request).execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(())
    }
}
