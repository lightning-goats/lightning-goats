use super::{
    LedgerStore,
    reward_funding::{confirmed_funding, decode_reservation},
    reward_outcomes::{budget_time, budget_usage},
    to_i64,
};
use crate::payout::execution::{Lease, Limits, Record, Request, State};
use anyhow::{Context, Result, bail};
use sqlx::Row;
use sqlx::{Sqlite, Transaction};
use uuid::Uuid;

impl LedgerStore {
    pub(crate) async fn payout_execution_state(
        &self,
        epoch: Uuid,
        id: Uuid,
        owner: &str,
        member: &str,
        limits: Limits<'_>,
    ) -> Result<State> {
        crate::nostr_event::validate_public_key(member)?;
        let mut page = self
            .payout_funding_page(
                epoch,
                id,
                owner,
                0,
                limits.policy,
                limits.enabled && limits.account.is_some(),
            )
            .await?;
        let mut tx = self.pool.begin().await?;
        let claim = load(&mut tx, id, member).await?;
        let reported: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM reward_outcome_manifests WHERE reservation_id=?)",
        )
        .bind(id.to_string())
        .fetch_one(&mut *tx)
        .await?;
        page.execution_enabled = page.execution_enabled
            && !reported
            && claim.as_ref().is_none_or(|c| {
                c.request
                    .quote()
                    .is_ok_and(|q| Some(q.request.scope.account_id) == limits.account)
            });
        page.nostr_enabled = limits.nostr;
        let claim = claim.map(|record| Lease {
            record,
            execution_enabled: page.execution_enabled,
            nostr_enabled: page.nostr_enabled,
            observed_at: page.observed_at,
        });
        let state = State { page, claim };
        state.validate(epoch, id, owner, member)?;
        tx.commit().await?;
        Ok(state)
    }
    pub(crate) async fn claim_payout_execution(
        &self,
        request: &Request,
        limits: Limits<'_>,
    ) -> Result<Lease> {
        let quote = request.quote()?;
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        validate_scope(&mut tx, request).await?;
        let now = budget_time(&mut tx, true).await?;
        let existing = load(&mut tx, request.reservation.id, &request.member).await?;
        let reported: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM reward_outcome_manifests WHERE reservation_id=?)",
        )
        .bind(request.reservation.id.to_string())
        .fetch_one(&mut *tx)
        .await?;
        let used = budget_usage(&mut tx, now).await?;
        let funding = confirmed_funding(&mut tx, request.reservation.request_id)
            .await?
            .context("claimed funding missing")?;
        let allowed = limits.enabled
            && !reported
            && Some(quote.request.scope.account_id) == limits.account
            && limits.policy.is_some_and(|p| {
                p.owner == request.reservation.owner
                    && p.budget(funding.strike_sats)
                        .is_ok_and(|cap| request.reservation.reserved_sats <= cap)
                    && used <= p.max_sats_per_24h
            })
            && (!quote.request.nip57 || limits.nostr);
        let record = if let Some(record) = existing {
            if record.request != *request {
                bail!(
                    "core retains a different original execution identity; restore reconciliation required"
                );
            }
            record
        } else {
            if !allowed
                || now < quote.observed_at
                || now >= quote.usable_until
                || u64::try_from(now)? >= quote.request.invoice_expires_at
            {
                bail!("core does not permit this new execution claim");
            }
            let sum:i64=sqlx::query_scalar("SELECT COALESCE(SUM(gross_sats),0) FROM payout_execution_claims WHERE reservation_id=?").bind(request.reservation.id.to_string()).fetch_one(&mut *tx).await?;
            if u64::try_from(sum)?
                .checked_add(quote.request.gross_sats)
                .is_none_or(|sum| sum > request.reservation.reserved_sats)
            {
                bail!("execution claims exceed reservation");
            }
            let scope = serde_json::to_string(&quote.request.scope)?;
            let different:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM payout_execution_claims WHERE reservation_id=? AND scope_json!=?)")
                .bind(request.reservation.id.to_string()).bind(&scope).fetch_one(&mut *tx).await?;
            if different {
                bail!("execution provider scope changed within reservation");
            }
            let record = Record {
                id: Uuid::new_v4(),
                request: request.clone(),
                created_at: now,
            };
            record.validate()?;
            sqlx::query("INSERT INTO payout_execution_claims(reservation_id,member,id,document_json,scope_json,quote_id,quote_intent_id,invoice_id,invoice_hash,gross_sats,created_at) VALUES(?,?,?,?,?,?,?,?,?,?,?)")
                .bind(request.reservation.id.to_string()).bind(&request.member).bind(record.id.to_string()).bind(serde_json::to_string(&record)?)
                .bind(scope).bind(quote.quote_id.to_string()).bind(quote.request.id.to_string()).bind(quote.request.invoice_id.to_string())
                .bind(&quote.request.payment_hash).bind(to_i64(quote.request.gross_sats,"claimed gross")?).bind(now).execute(&mut *tx).await?;
            record
        };
        tx.commit().await?;
        Ok(Lease {
            record,
            execution_enabled: allowed,
            nostr_enabled: limits.nostr,
            observed_at: now,
        })
    }
}
async fn validate_scope(tx: &mut Transaction<'_, Sqlite>, request: &Request) -> Result<()> {
    let epoch: String =
        sqlx::query_scalar("SELECT stream_id FROM overlay_identity WHERE singleton=1")
            .fetch_one(&mut **tx)
            .await?;
    let row = sqlx::query("SELECT * FROM reward_reservations WHERE id=?")
        .bind(request.reservation.id.to_string())
        .fetch_one(&mut **tx)
        .await?;
    if epoch != request.reservation.epoch.to_string()
        || decode_reservation(&row)? != request.reservation
    {
        bail!("execution claim has wrong core reservation/epoch");
    }
    let funding = confirmed_funding(tx, request.reservation.request_id)
        .await?
        .context("execution reservation lost funding")?;
    if funding.proof_hash != request.reservation.funding_hash {
        bail!("execution funding proof changed");
    }
    Ok(())
}
pub(super) async fn load(
    tx: &mut Transaction<'_, Sqlite>,
    id: Uuid,
    member: &str,
) -> Result<Option<Record>> {
    let row =
        sqlx::query("SELECT * FROM payout_execution_claims WHERE reservation_id=? AND member=?")
            .bind(id.to_string())
            .bind(member)
            .fetch_optional(&mut **tx)
            .await?;
    row.map(|row| -> Result<_> {
        let r: Record = serde_json::from_str(&row.try_get::<String, _>("document_json")?)?;
        r.validate()?;
        let q = r.request.quote()?;
        if r.request.reservation.id != id
            || r.request.member != member
            || r.id.to_string() != row.try_get::<String, _>("id")?
            || q.quote_id.to_string() != row.try_get::<String, _>("quote_id")?
            || q.request.id.to_string() != row.try_get::<String, _>("quote_intent_id")?
            || q.request.invoice_id.to_string() != row.try_get::<String, _>("invoice_id")?
            || q.request.payment_hash != row.try_get::<String, _>("invoice_hash")?
            || q.request.gross_sats != u64::try_from(row.try_get::<i64, _>("gross_sats")?)?
            || serde_json::to_string(&q.request.scope)? != row.try_get::<String, _>("scope_json")?
            || r.created_at != row.try_get::<i64, _>("created_at")?
        {
            bail!("stored execution claim scope changed");
        }
        Ok(r)
    })
    .transpose()
}
pub(super) async fn validate_outcome(
    tx: &mut Transaction<'_, Sqlite>,
    packet: &crate::payout::outcome::Packet,
) -> Result<()> {
    let claim = load(tx, packet.header.reservation.id, &packet.row.member).await?;
    match (&packet.row.disposition, claim) {
        (crate::payout::outcome::Disposition::Unsubmitted { .. }, None) => Ok(()),
        (crate::payout::outcome::Disposition::Payment { binding_json, .. }, Some(claim)) => {
            let binding: crate::payout::provider::BindingRecord =
                serde_json::from_str(binding_json)?;
            if claim.request.reservation != packet.header.reservation
                || claim.request.quote()? != binding.quote
            {
                bail!("payment outcome differs from core execution claim");
            }
            Ok(())
        }
        _ => bail!("core execution claim contradicts terminal/no-execute report"),
    }
}
