//! Durable quote issuance, budgets and binding; no provider I/O while holding a DB lock.
use anyhow::{Context, Result, bail};
use sqlx::{Row, Sqlite, Transaction};
use uuid::Uuid;

use super::{LedgerStore, XmrCreditIntent};
use crate::quotes::{QuotePolicy, QuoteRequest, RESERVATION_SECONDS, StoredXmrQuote};

async fn check_clock(tx: &mut Transaction<'_, Sqlite>, now: i64) -> Result<()> {
    let prior: i64 = sqlx::query_scalar("SELECT last_epoch FROM xmr_quote_clock WHERE singleton=1")
        .fetch_one(&mut **tx)
        .await?;
    if now < prior || now < 0 {
        bail!("quote clock regressed; operator reconciliation required");
    }
    sqlx::query("UPDATE xmr_quote_clock SET last_epoch=? WHERE singleton=1")
        .bind(now)
        .execute(&mut **tx)
        .await?;
    Ok(())
}
fn stored_result(row: &sqlx::sqlite::SqliteRow, identity: &str, now: i64) -> Result<String> {
    if row.get::<String, _>("identity_json") != identity {
        bail!("quote request ID is already bound to different inputs");
    }
    if now < row.get::<i64, _>("requested_at") {
        bail!("quote clock regressed");
    }
    match row.get::<String, _>("status").as_str() {
        "ready" => row
            .try_get::<Option<String>, _>("document_json")?
            .context("missing persisted quote"),
        "reserved" if now < row.get::<i64, _>("reservation_until") => {
            bail!("quote request is in progress; retry the same ID")
        }
        _ => bail!(
            "quote request failed or reservation expired; an explicit new request is required"
        ),
    }
}
impl LedgerStore {
    pub(crate) async fn reserve_xmr_quote(
        &self,
        request: &QuoteRequest,
        identity: &str,
        policy: &QuotePolicy,
        now: i64,
    ) -> Result<Option<String>> {
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        check_clock(&mut tx, now).await?;
        if let Some(row) = sqlx::query("SELECT * FROM xmr_quote_requests WHERE id=?")
            .bind(request.id.to_string())
            .fetch_optional(&mut *tx)
            .await?
        {
            let existing = stored_result(&row, identity, now)?;
            tx.commit().await?;
            return Ok(Some(existing));
        }
        if request.target_sats < policy.min_target_sats
            || request.target_sats > policy.max_target_sats
        {
            bail!("quote target outside configured bounds");
        }
        let since = now.saturating_sub(policy.admission_window_seconds as i64);
        let total: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM xmr_quote_requests")
            .fetch_one(&mut *tx)
            .await?;
        let recent: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM xmr_quote_requests WHERE requested_at>?")
                .bind(since)
                .fetch_one(&mut *tx)
                .await?;
        let client: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM xmr_quote_requests WHERE client_bucket=? AND requested_at>?",
        )
        .bind(&request.client_bucket)
        .bind(since)
        .fetch_one(&mut *tx)
        .await?;
        let pending:i64=sqlx::query_scalar("SELECT COUNT(*) FROM xmr_quote_requests WHERE status='reserved' AND reservation_until>?").bind(now).fetch_one(&mut *tx).await?;
        if total >= i64::from(policy.max_retained_requests)
            || recent >= i64::from(policy.max_requests_per_window)
            || client >= i64::from(policy.max_requests_per_client_window)
            || pending >= i64::from(policy.max_pending_requests)
        {
            bail!("quote admission capacity exhausted");
        }
        let until = now
            .checked_add(RESERVATION_SECONDS)
            .context("reservation expiry overflow")?;
        sqlx::query("INSERT INTO xmr_quote_requests(id,identity_json,client_bucket,requested_at,reservation_until,status) VALUES (?,?,?,?,?,'reserved')")
            .bind(request.id.to_string()).bind(identity).bind(&request.client_bucket).bind(now).bind(until).execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(None)
    }
    pub(crate) async fn complete_xmr_quote(
        &self,
        id: Uuid,
        identity: &str,
        quote: &StoredXmrQuote,
        now: i64,
    ) -> Result<()> {
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        check_clock(&mut tx, now).await?;
        let row = sqlx::query("SELECT * FROM xmr_quote_requests WHERE id=?")
            .bind(id.to_string())
            .fetch_one(&mut *tx)
            .await?;
        if row.get::<String, _>("status") != "reserved"
            || row.get::<String, _>("identity_json") != identity
            || now >= row.get::<i64, _>("reservation_until")
            || quote.id() != id
            || quote.quote().issued_at() != now
        {
            bail!("quote reservation no longer valid");
        }
        let rate = quote.quote().rate();
        let (n, d) = rate.ratio();
        if let Some(prior) = sqlx::query("SELECT * FROM xmr_rate_watermarks WHERE source=?")
            .bind(rate.source())
            .fetch_optional(&mut *tx)
            .await?
        {
            let time: i64 = prior.try_get("observed_at")?;
            let pn: u64 = prior.try_get::<String, _>("numerator")?.parse()?;
            let pd: u64 = prior.try_get::<String, _>("denominator")?.parse()?;
            if time > rate.observed_at()
                || (time == rate.observed_at()
                    && u128::from(n) * u128::from(pd) != u128::from(pn) * u128::from(d))
            {
                bail!("rate observation regressed or changed at the same timestamp");
            }
            if time < rate.observed_at() {
                sqlx::query("UPDATE xmr_rate_watermarks SET observed_at=?,numerator=?,denominator=? WHERE source=?").bind(rate.observed_at()).bind(n.to_string()).bind(d.to_string()).bind(rate.source()).execute(&mut *tx).await?;
            }
        } else {
            sqlx::query("INSERT INTO xmr_rate_watermarks VALUES (?,?,?,?)")
                .bind(rate.source())
                .bind(rate.observed_at())
                .bind(n.to_string())
                .bind(d.to_string())
                .execute(&mut *tx)
                .await?;
        }
        sqlx::query("UPDATE xmr_quote_requests SET status='ready',document_json=? WHERE id=? AND status='reserved'").bind(&quote.document_json).bind(id.to_string()).execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(())
    }
    pub(crate) async fn fail_xmr_quote(&self, id: Uuid) -> Result<()> {
        sqlx::query(
            "UPDATE xmr_quote_requests SET status='failed' WHERE id=? AND status='reserved'",
        )
        .bind(id.to_string())
        .execute(&self.pool)
        .await?;
        Ok(())
    }
    pub(crate) async fn read_xmr_quote(
        &self,
        id: Uuid,
        identity: &str,
        now: i64,
    ) -> Result<String> {
        let row = sqlx::query("SELECT * FROM xmr_quote_requests WHERE id=?")
            .bind(id.to_string())
            .fetch_optional(&self.pool)
            .await?
            .context("unknown quote request")?;
        stored_result(&row, identity, now)
    }
    pub(crate) async fn bind_xmr_quote(
        &self,
        quote: &StoredXmrQuote,
        intent: &XmrCreditIntent,
        now: i64,
    ) -> Result<()> {
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        check_clock(&mut tx, now).await?;
        let saved: String = sqlx::query_scalar(
            "SELECT document_json FROM xmr_quote_requests WHERE id=? AND status='ready'",
        )
        .bind(quote.id().to_string())
        .fetch_one(&mut *tx)
        .await?;
        if saved != quote.document_json {
            bail!("quote changed before binding");
        }
        if let Some(row) = sqlx::query(
            "SELECT receive_scope,valuation_id FROM xmr_quote_bindings WHERE quote_id=?",
        )
        .bind(quote.id().to_string())
        .fetch_optional(&mut *tx)
        .await?
        {
            if row.get::<String, _>("receive_scope") != intent.receive_scope
                || row.get::<String, _>("valuation_id") != format!("xmr:{}", intent.id)
            {
                bail!("quote already has a different receive binding");
            }
        } else {
            if now < quote.quote().issued_at() || now >= quote.quote().expires_at() {
                bail!("cannot bind an expired or future quote");
            }
            // Binding and #95 valuation registration share this SAME transaction.
            super::credit::register_xmr_intent_in_transaction(&mut tx, intent).await?;
            sqlx::query("INSERT INTO xmr_quote_bindings VALUES (?,?,?,?)")
                .bind(quote.id().to_string())
                .bind(format!("xmr:{}", intent.id))
                .bind(&intent.receive_scope)
                .bind(now)
                .execute(&mut *tx)
                .await?;
        }
        super::credit::register_xmr_intent_in_transaction(&mut tx, intent).await?;
        tx.commit().await?;
        Ok(())
    }
}
