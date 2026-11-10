//! Core-owned FIFO provenance and confirmed-feed spending reservations. No
//! provider request, member lookup or physical command occurs in this module.
use anyhow::{Context, Result, bail};
use serde_json::json;
use sha2::{Digest, Sha256};
use sqlx::{Row, Sqlite, Transaction};
use uuid::Uuid;

use super::{LedgerStore, to_i64, to_u64};
use crate::reward_funding::{
    Allowance, Funding, FundingPage, FundingPart, MAX_FUNDING_PARTS, Reservation, RewardPolicy,
    StrikeFundingAnchor,
};

impl LedgerStore {
    /// Payout-only readback of a core reservation and its actual consumed
    /// sources. The execution flag is a fresh policy snapshot, not a lease or
    /// a provider payment. Old reservations remain readable while held.
    pub(crate) async fn payout_funding_page(
        &self,
        epoch: Uuid,
        reservation_id: Uuid,
        owner: &str,
        after: i64,
        policy: Option<&RewardPolicy>,
        execution_enabled: bool,
    ) -> Result<FundingPage> {
        crate::nostr_event::validate_public_key(owner)?;
        if after < 0 || epoch.is_nil() || reservation_id.is_nil() {
            bail!("invalid payout funding scope");
        }
        let mut tx = self.pool.begin().await?;
        let current: String =
            sqlx::query_scalar("SELECT stream_id FROM overlay_identity WHERE singleton=1")
                .fetch_one(&mut *tx)
                .await?;
        if current != epoch.to_string() {
            bail!("payout read requires pinned core epoch");
        }
        let row = sqlx::query("SELECT * FROM reward_reservations WHERE id=?")
            .bind(reservation_id.to_string())
            .fetch_one(&mut *tx)
            .await?;
        let reservation = decode_reservation(&row)?;
        if reservation.owner != owner || reservation.epoch != epoch {
            bail!("payout reservation scope mismatch");
        }
        let funding = confirmed_funding(&mut tx, reservation.request_id)
            .await?
            .context("reservation funding missing")?;
        let frozen: RewardPolicy = serde_json::from_str(&row.try_get::<String, _>("policy_json")?)?;
        if frozen.owner != owner
            || funding.proof_hash != reservation.funding_hash
            || reservation.reserved_sats > frozen.budget(funding.strike_sats)?
        {
            bail!("reservation violates frozen financial scope");
        }
        let threshold: i64 =
            sqlx::query_scalar("SELECT total_sats FROM feed_reward_funding WHERE request_id=?")
                .bind(reservation.request_id.to_string())
                .fetch_one(&mut *tx)
                .await?;
        // A caller cannot manufacture a starting cursor that skips a source.
        // Clients still have to start at zero and complete the page sequence.
        if after > 0 {
            let valid: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM feed_reward_funding_parts WHERE request_id=? AND ledger_id=?)")
                .bind(reservation.request_id.to_string()).bind(after).fetch_one(&mut *tx).await?;
            if !valid {
                bail!("payout funding cursor is not a consumed part");
            }
        }
        let rows = sqlx::query("SELECT p.ledger_id,p.sats,l.kind,l.proof_hash FROM feed_reward_funding_parts p JOIN reward_funding_lots l ON l.ledger_id=p.ledger_id WHERE p.request_id=? AND p.ledger_id>? ORDER BY p.ledger_id LIMIT ?")
            .bind(reservation.request_id.to_string()).bind(after).bind((MAX_FUNDING_PARTS+1) as i64).fetch_all(&mut *tx).await?;
        let more = rows.len() > MAX_FUNDING_PARTS;
        let mut parts = Vec::new();
        let mut through = after;
        for row in rows.into_iter().take(MAX_FUNDING_PARTS) {
            let ledger_id: i64 = row.try_get("ledger_id")?;
            let kind: String = row.try_get("kind")?;
            let hash: String = row.try_get("proof_hash")?;
            let actual = lot_proof(&mut tx, ledger_id).await?;
            if actual.0 != kind || actual.1 != hash {
                bail!("consumed funding provenance changed");
            }
            let anchor = if kind == "strike_btc" {
                Some(strike_anchor(&mut tx, ledger_id).await?)
            } else {
                None
            };
            parts.push(FundingPart {
                ledger_id,
                consumed_sats: to_u64(row.try_get("sats")?, "consumed sats")?,
                anchor,
            });
            through = ledger_id;
        }
        let budget_now = super::reward_outcomes::budget_time(&mut tx, false).await?;
        let used = super::reward_outcomes::budget_usage(&mut tx, budget_now).await?;
        let finalized: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM reward_outcome_finals WHERE reservation_id=?)",
        )
        .bind(reservation_id.to_string())
        .fetch_one(&mut *tx)
        .await?;
        let allowed = !finalized
            && match policy {
                Some(p) => {
                    p.owner == owner
                        && reservation.reserved_sats <= p.budget(funding.strike_sats)?
                        && used <= p.max_sats_per_24h
                }
                None => false,
            };
        let page = FundingPage {
            reservation,
            funding,
            threshold_sats: to_u64(threshold, "funded threshold")?,
            execution_enabled: execution_enabled && allowed,
            nostr_enabled: false,
            observed_at: crate::cyberherd::calendar::now()?,
            after,
            through,
            more,
            parts,
        };
        page.validate()?;
        tx.commit().await?;
        Ok(page)
    }

    pub(crate) async fn reward_allowance(
        &self,
        policy: &RewardPolicy,
        epoch: Uuid,
        request_id: Uuid,
    ) -> Result<Allowance> {
        policy.validate()?;
        let mut tx = self.pool.begin().await?;
        let current: String =
            sqlx::query_scalar("SELECT stream_id FROM overlay_identity WHERE singleton=1")
                .fetch_one(&mut *tx)
                .await?;
        if current != epoch.to_string() {
            bail!("reward planning requires pinned core epoch");
        }
        let funding = confirmed_funding(&mut tx, request_id)
            .await?
            .context("feeding has no eligible funding proof")?;
        let threshold: i64 =
            sqlx::query_scalar("SELECT total_sats FROM feed_reward_funding WHERE request_id=?")
                .bind(request_id.to_string())
                .fetch_one(&mut *tx)
                .await?;
        let budget_now = super::reward_outcomes::budget_time(&mut tx, false).await?;
        let used = super::reward_outcomes::budget_usage(&mut tx, budget_now).await?;
        let value = Allowance {
            epoch,
            request_id,
            owner: policy.owner.clone(),
            funding,
            threshold_sats: to_u64(threshold, "funded threshold")?,
            member_percent_ceiling: policy.member_percent_ceiling,
            max_sats_per_feed: policy.max_sats_per_feed,
            available_sats: policy.max_sats_per_24h.saturating_sub(used),
            policy_hash: hex::encode(Sha256::digest(serde_json::to_vec(policy)?)),
        };
        value.validate()?;
        tx.commit().await?;
        Ok(value)
    }

    /// Explicit root-policy activation. Replay old deltas only to establish
    /// remaining FIFO lots; never create a historical feed/reward opportunity.
    pub(crate) async fn initialize_reward_funding(&self) -> Result<()> {
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM reward_funding_state)")
            .fetch_one(&mut *tx)
            .await?;
        if !exists {
            let through: i64 = sqlx::query_scalar("SELECT COALESCE(MAX(id),0) FROM ledger_entries")
                .fetch_one(&mut *tx)
                .await?;
            sqlx::query("INSERT INTO reward_funding_state(singleton,bootstrap_through,last_ledger_id) VALUES(1,?,0)").bind(through).execute(&mut *tx).await?;
        }
        prepare(&mut tx).await?;
        tx.commit().await?;
        Ok(())
    }

    pub(crate) async fn reserve_reward(
        &self,
        policy: &RewardPolicy,
        epoch: Uuid,
        request_id: Uuid,
        plan_hash: &str,
        amount_sats: u64,
    ) -> Result<Reservation> {
        policy.validate()?;
        crate::nostr_event::validate_hex(plan_hash, 32)?;
        if epoch.is_nil()
            || request_id.is_nil()
            || amount_sats == 0
            || amount_sats > i64::MAX as u64
        {
            bail!("invalid reservation request");
        }
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let current: String =
            sqlx::query_scalar("SELECT stream_id FROM overlay_identity WHERE singleton=1")
                .fetch_one(&mut *tx)
                .await?;
        if current != epoch.to_string() {
            bail!("reservation requires the current pinned core epoch");
        }
        if let Some(row) = sqlx::query("SELECT * FROM reward_reservations WHERE request_id=?")
            .bind(request_id.to_string())
            .fetch_optional(&mut *tx)
            .await?
        {
            let old = decode_reservation(&row)?;
            if old.epoch != epoch
                || old.owner != policy.owner
                || old.plan_hash != plan_hash
                || old.reserved_sats != amount_sats
            {
                bail!("confirmed feeding already has a different reward reservation");
            }
            let funding = confirmed_funding(&mut tx, request_id)
                .await?
                .context("reservation lost its funding proof")?;
            if old.funding_hash != funding.proof_hash {
                bail!("reservation funding changed");
            }
            let frozen: RewardPolicy =
                serde_json::from_str(&row.try_get::<String, _>("policy_json")?)?;
            if frozen.owner != old.owner
                || old.reserved_sats > frozen.budget(funding.strike_sats)?
            {
                bail!("reservation exceeds its frozen policy");
            }
            tx.commit().await?;
            return Ok(old);
        }
        let funding = confirmed_funding(&mut tx, request_id)
            .await?
            .context("feeding has no eligible funding proof")?;
        if amount_sats > policy.budget(funding.strike_sats)? {
            bail!("reward exceeds real funding or root per-feed ceiling");
        }
        let now = super::reward_outcomes::budget_time(&mut tx, true).await?;
        // All reservations remain outstanding until a separately authenticated
        // payout outcome exists. Never age an unknown/submitted payment out by
        // its creation time: it could still spend later. Verified terminal
        // outcome accounting must precede releasing/aging this conservative cap.
        let budget_now = super::reward_outcomes::budget_time(&mut tx, false).await?;
        let used = super::reward_outcomes::budget_usage(&mut tx, budget_now).await?;
        if u128::from(used) + u128::from(amount_sats) > u128::from(policy.max_sats_per_24h) {
            bail!("root outstanding/recent reward limit reached");
        }
        let policy_json = serde_json::to_string(policy)?;
        let reservation = Reservation {
            id: Uuid::new_v4(),
            request_id,
            epoch,
            owner: policy.owner.clone(),
            plan_hash: plan_hash.into(),
            funding_hash: funding.proof_hash,
            policy_hash: hex::encode(Sha256::digest(policy_json.as_bytes())),
            reserved_sats: amount_sats,
            created_at: now,
        };
        reservation.validate()?;
        sqlx::query("INSERT INTO reward_reservations(id,request_id,epoch,owner,plan_hash,funding_hash,policy_json,reserved_sats,created_at) VALUES(?,?,?,?,?,?,?,?,?)")
            .bind(reservation.id.to_string()).bind(request_id.to_string()).bind(epoch.to_string()).bind(&policy.owner).bind(plan_hash).bind(&reservation.funding_hash).bind(policy_json).bind(to_i64(amount_sats,"reserved sats")?).bind(now).execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(reservation)
    }
}

async fn strike_anchor(
    tx: &mut Transaction<'_, Sqlite>,
    ledger_id: i64,
) -> Result<StrikeFundingAnchor> {
    let p = sqlx::query("SELECT p.source,p.source_id,p.payment_hash,p.amount_msat,p.address_user,p.credit_pool,CASE WHEN length(CAST(p.context_json AS BLOB))<=32768 THEN p.context_json ELSE NULL END AS context_json FROM credit_grants g JOIN asset_receipts a ON a.id=g.receipt_id JOIN settled_payments p ON p.id=a.btc_settlement_id WHERE g.ledger_entry_id=?")
        .bind(ledger_id).fetch_one(&mut **tx).await?;
    if p.try_get::<String, _>("source")? != "strike" {
        bail!("payout funding source is not Strike");
    }
    let context: serde_json::Value = serde_json::from_str(
        &p.try_get::<Option<String>, _>("context_json")?
            .context("Strike receive lacks request correlation")?,
    )?;
    let request_id: Uuid = context["receive_request_id"]
        .as_str()
        .context("Strike request correlation missing")?
        .parse()?;
    let request = sqlx::query("SELECT * FROM strike_receive_requests WHERE receive_request_id=?")
        .bind(request_id.to_string())
        .fetch_one(&mut **tx)
        .await?;
    let address_user: String = p.try_get("address_user")?;
    let credit_pool: String = p.try_get("credit_pool")?;
    if address_user != request.try_get::<String, _>("address_user")?
        || credit_pool != request.try_get::<String, _>("credit_pool")?
    {
        bail!("Strike request and consumed credit address/pool mismatch");
    }
    let anchor = StrikeFundingAnchor {
        receive_request_id: request_id,
        receive_id: p.try_get::<String, _>("source_id")?.parse()?,
        address_user,
        credit_pool,
        requested_msat: to_u64(request.try_get("amount_msat")?, "requested msat")?,
        settled_msat: to_u64(p.try_get("amount_msat")?, "settled msat")?,
        invoice: request.try_get("invoice")?,
        payment_hash: request.try_get("payment_hash")?,
        description_hash: request.try_get("description_hash")?,
        settled_hash: p.try_get("payment_hash")?,
    };
    anchor.validate()?;
    Ok(anchor)
}

/// Sync only positive grants after activation. Every later debit must have
/// been consumed in the same confirmation transaction, never guessed on reopen.
pub(super) async fn prepare(tx: &mut Transaction<'_, Sqlite>) -> Result<bool> {
    let state = sqlx::query(
        "SELECT bootstrap_through,last_ledger_id FROM reward_funding_state WHERE singleton=1",
    )
    .fetch_optional(&mut **tx)
    .await?;
    let Some(state) = state else {
        return Ok(false);
    };
    let bootstrap: i64 = state.try_get("bootstrap_through")?;
    let mut cursor: i64 = state.try_get("last_ledger_id")?;
    let latest: i64 = sqlx::query_scalar("SELECT COALESCE(MAX(id),0) FROM ledger_entries")
        .fetch_one(&mut **tx)
        .await?;
    if latest < cursor {
        bail!("funding cursor is ahead of retained ledger history");
    }
    loop {
        let rows = sqlx::query(
            "SELECT id,delta_sats FROM ledger_entries WHERE id>? ORDER BY id LIMIT 256",
        )
        .bind(cursor)
        .fetch_all(&mut **tx)
        .await?;
        if rows.is_empty() {
            break;
        }
        for row in rows {
            let id: i64 = row.try_get("id")?;
            let delta: i64 = row.try_get("delta_sats")?;
            if delta > 0 {
                let (kind, proof) = lot_proof(tx, id).await?;
                sqlx::query("INSERT INTO reward_funding_lots(ledger_id,kind,original_sats,remaining_sats,proof_hash) VALUES(?,?,?,?,?)")
                    .bind(id).bind(kind).bind(delta).bind(delta).bind(proof).execute(&mut **tx).await?;
            } else if delta < 0 {
                if id > bootstrap {
                    bail!("untracked post-activation debit; funding recovery required");
                }
                consume(tx, None, delta.unsigned_abs()).await?;
            }
            cursor = id;
        }
        sqlx::query("UPDATE reward_funding_state SET last_ledger_id=? WHERE singleton=1")
            .bind(cursor)
            .execute(&mut **tx)
            .await?;
    }
    audit_balance(tx).await?;
    Ok(true)
}

pub(super) async fn record_confirmation(
    tx: &mut Transaction<'_, Sqlite>,
    id: Uuid,
    debit_id: i64,
    threshold: u64,
) -> Result<()> {
    let (strike_sats, proof_hash) = consume(tx, Some((id, debit_id)), threshold).await?;
    sqlx::query("INSERT INTO feed_reward_funding(request_id,debit_id,total_sats,strike_sats,proof_hash) VALUES(?,?,?,?,?)")
        .bind(id.to_string()).bind(debit_id).bind(to_i64(threshold,"funding debit")?).bind(to_i64(strike_sats,"real funding")?).bind(proof_hash).execute(&mut **tx).await?;
    sqlx::query(
        "UPDATE reward_funding_state SET last_ledger_id=? WHERE singleton=1 AND last_ledger_id<?",
    )
    .bind(debit_id)
    .bind(debit_id)
    .execute(&mut **tx)
    .await?;
    audit_balance(tx).await?;
    Ok(())
}

async fn lot_proof(tx: &mut Transaction<'_, Sqlite>, id: i64) -> Result<(&'static str, String)> {
    let l=sqlx::query("SELECT entry_type,source_key,delta_sats,payment_hash,payment_source,payment_source_id,address_user,credit_pool,feed_attempt_id FROM ledger_entries WHERE id=?").bind(id).fetch_one(&mut **tx).await?;
    let delta: i64 = l.try_get("delta_sats")?;
    if delta <= 0 {
        bail!("funding lot must be a positive credit");
    }
    let native=sqlx::query("SELECT p.id,p.source,p.source_id,p.payment_hash,p.address_user,p.credit_pool,p.amount_msat,p.settled_at FROM credit_grants g JOIN asset_receipts r ON r.id=g.receipt_id JOIN settled_payments p ON p.id=r.btc_settlement_id WHERE g.ledger_entry_id=?").bind(id).fetch_optional(&mut **tx).await?;
    let mut kind = "unavailable";
    let mut identity = serde_json::Value::Null;
    if let Some(p) = native {
        super::credit::verify_native_btc(tx, p.try_get("id")?).await?;
        let source: String = p.try_get("source")?;
        let source_id: String = p.try_get("source_id")?;
        let hash: Option<String> = p.try_get("payment_hash")?;
        let user: String = p.try_get("address_user")?;
        let pool: String = p.try_get("credit_pool")?;
        if l.try_get::<Option<String>, _>("payment_source")?.as_ref() != Some(&source)
            || l.try_get::<Option<String>, _>("payment_source_id")?
                .as_ref()
                != Some(&source_id)
            || l.try_get::<Option<String>, _>("payment_hash")? != hash
            || l.try_get::<Option<String>, _>("address_user")?.as_ref() != Some(&user)
            || l.try_get::<Option<String>, _>("credit_pool")?.as_ref() != Some(&pool)
        {
            bail!("funding provenance disagrees with credited source");
        }
        if source == "strike" && pool == "herd" {
            kind = "strike_btc";
        }
        identity = json!([
            source,
            source_id,
            hash,
            user,
            pool,
            p.try_get::<i64, _>("amount_msat")?,
            p.try_get::<Option<i64>, _>("settled_at")?
        ]);
    }
    let proof = json!([
        id,
        l.try_get::<String, _>("entry_type")?,
        l.try_get::<String, _>("source_key")?,
        delta,
        l.try_get::<Option<String>, _>("payment_hash")?,
        l.try_get::<Option<String>, _>("payment_source")?,
        l.try_get::<Option<String>, _>("payment_source_id")?,
        l.try_get::<Option<String>, _>("address_user")?,
        l.try_get::<Option<String>, _>("credit_pool")?,
        l.try_get::<Option<String>, _>("feed_attempt_id")?,
        identity
    ]);
    Ok((
        kind,
        hex::encode(Sha256::digest(serde_json::to_vec(&proof)?)),
    ))
}

fn manifest(id: Uuid, debit: i64, threshold: u64) -> Result<Sha256> {
    let mut hash = Sha256::new();
    hash.update(b"lightning-goats-funding-v1");
    hash_part(&mut hash, &json!([id, debit, threshold]))?;
    Ok(hash)
}
fn hash_part(hash: &mut Sha256, value: &serde_json::Value) -> Result<()> {
    let bytes = serde_json::to_vec(value)?;
    hash.update(u32::try_from(bytes.len())?.to_be_bytes());
    hash.update(bytes);
    Ok(())
}

async fn consume(
    tx: &mut Transaction<'_, Sqlite>,
    feed: Option<(Uuid, i64)>,
    amount: u64,
) -> Result<(u64, String)> {
    let mut left = amount;
    let mut strike = 0u64;
    let mut hash = manifest(
        feed.map_or(Uuid::nil(), |v| v.0),
        feed.map_or(0, |v| v.1),
        amount,
    )?;
    while left > 0 {
        let rows=sqlx::query("SELECT ledger_id,kind,remaining_sats,proof_hash FROM reward_funding_lots WHERE remaining_sats>0 ORDER BY ledger_id LIMIT 256").fetch_all(&mut **tx).await?;
        if rows.is_empty() {
            bail!("funding lots cannot cover confirmed debit");
        }
        for row in rows {
            if left == 0 {
                break;
            }
            let ledger: i64 = row.try_get("ledger_id")?;
            let remaining = to_u64(row.try_get("remaining_sats")?, "remaining funding")?;
            let kind: String = row.try_get("kind")?;
            let proof: String = row.try_get("proof_hash")?;
            let actual = lot_proof(tx, ledger).await?;
            if actual.0 != kind || actual.1 != proof {
                bail!("funding source changed since observation");
            }
            let take = left.min(remaining);
            sqlx::query(
                "UPDATE reward_funding_lots SET remaining_sats=remaining_sats-? WHERE ledger_id=?",
            )
            .bind(to_i64(take, "funding consumed")?)
            .bind(ledger)
            .execute(&mut **tx)
            .await?;
            if let Some((id, _)) = feed {
                sqlx::query("INSERT INTO feed_reward_funding_parts(request_id,ledger_id,sats) VALUES(?,?,?)")
                    .bind(id.to_string()).bind(ledger).bind(to_i64(take,"funding part")?).execute(&mut **tx).await?;
            }
            hash_part(&mut hash, &json!([ledger, take, kind, proof]))?;
            if kind == "strike_btc" {
                strike = strike.checked_add(take).context("real funding overflow")?;
            }
            left -= take;
        }
    }
    Ok((strike, hex::encode(hash.finalize())))
}

async fn audit_balance(tx: &mut Transaction<'_, Sqlite>) -> Result<()> {
    let lots: i64 =
        sqlx::query_scalar("SELECT COALESCE(SUM(remaining_sats),0) FROM reward_funding_lots")
            .fetch_one(&mut **tx)
            .await?;
    let ledger = super::feed::feed_credit_in_transaction(tx).await?;
    if to_u64(lots, "funding remaining total")? != ledger {
        bail!("funding lots disagree with durable feed credit");
    }
    Ok(())
}

pub(super) async fn confirmed_funding(
    tx: &mut Transaction<'_, Sqlite>,
    id: Uuid,
) -> Result<Option<Funding>> {
    let row=sqlx::query("SELECT r.*,f.status,f.threshold_sats,l.delta_sats,l.entry_type,l.source_key,l.feed_attempt_id FROM feed_reward_funding r JOIN feed_attempts f ON f.id=r.request_id JOIN ledger_entries l ON l.id=r.debit_id WHERE r.request_id=?")
        .bind(id.to_string()).fetch_optional(&mut **tx).await?;
    let Some(row) = row else {
        return Ok(None);
    };
    let total = to_u64(row.try_get("total_sats")?, "funded feed amount")?;
    if row.try_get::<String, _>("status")? != "confirmed"
        || row.try_get::<i64, _>("threshold_sats")? != to_i64(total, "funded threshold")?
        || row.try_get::<i64, _>("delta_sats")? != -to_i64(total, "confirmed debit")?
        || row.try_get::<String, _>("entry_type")? != "FEED_DEBIT"
        || row.try_get::<String, _>("source_key")? != format!("feed:{id}")
        || row
            .try_get::<Option<String>, _>("feed_attempt_id")?
            .as_deref()
            != Some(id.to_string().as_str())
    {
        bail!("reward proof lost its confirmed debit");
    }
    let mut hash = manifest(id, row.try_get("debit_id")?, total)?;
    let mut cursor = 0;
    let mut sum = 0u64;
    let mut strike = 0u64;
    loop {
        let parts=sqlx::query("SELECT p.ledger_id,p.sats,l.kind,l.proof_hash,l.original_sats,e.delta_sats FROM feed_reward_funding_parts p JOIN reward_funding_lots l ON l.ledger_id=p.ledger_id JOIN ledger_entries e ON e.id=l.ledger_id WHERE p.request_id=? AND p.ledger_id>? ORDER BY p.ledger_id LIMIT 256")
            .bind(id.to_string()).bind(cursor).fetch_all(&mut **tx).await?;
        if parts.is_empty() {
            break;
        }
        for part in parts {
            let ledger: i64 = part.try_get("ledger_id")?;
            let sats = to_u64(part.try_get("sats")?, "funding part")?;
            if part.try_get::<i64, _>("original_sats")? != part.try_get::<i64, _>("delta_sats")?
                || sats > to_u64(part.try_get("original_sats")?, "original funding")?
            {
                bail!("funding part lost its positive credit");
            }
            let kind: String = part.try_get("kind")?;
            let proof: String = part.try_get("proof_hash")?;
            hash_part(&mut hash, &json!([ledger, sats, kind, proof]))?;
            sum = sum.checked_add(sats).context("funding sum overflow")?;
            if kind == "strike_btc" {
                strike = strike
                    .checked_add(sats)
                    .context("real funding sum overflow")?;
            }
            cursor = ledger;
        }
    }
    let funding = Funding {
        strike_sats: strike,
        proof_hash: hex::encode(hash.finalize()),
    };
    if sum != total
        || strike != to_u64(row.try_get("strike_sats")?, "confirmed real funding")?
        || funding.proof_hash != row.try_get::<String, _>("proof_hash")?
    {
        bail!("confirmed funding manifest is inconsistent");
    }
    funding.validate(total)?;
    Ok(Some(funding))
}

pub(super) fn decode_reservation(row: &sqlx::sqlite::SqliteRow) -> Result<Reservation> {
    let r = Reservation {
        id: row.try_get::<String, _>("id")?.parse()?,
        request_id: row.try_get::<String, _>("request_id")?.parse()?,
        epoch: row.try_get::<String, _>("epoch")?.parse()?,
        owner: row.try_get("owner")?,
        plan_hash: row.try_get("plan_hash")?,
        funding_hash: row.try_get("funding_hash")?,
        policy_hash: hex::encode(Sha256::digest(
            row.try_get::<String, _>("policy_json")?.as_bytes(),
        )),
        reserved_sats: to_u64(row.try_get("reserved_sats")?, "reserved sats")?,
        created_at: row.try_get("created_at")?,
    };
    r.validate()?;
    Ok(r)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        domain::payment::SettledPayment, ledger::OperatorCreditGrant, nostr_event::test_event,
    };

    async fn store() -> (tempfile::TempDir, String, LedgerStore) {
        let root = tempfile::TempDir::new().unwrap();
        let url = format!("sqlite://{}", root.path().join("core.db").display());
        let store = LedgerStore::connect(&url).await.unwrap();
        store.overlay_stream_id().await.unwrap();
        (root, url, store)
    }
    fn policy() -> RewardPolicy {
        RewardPolicy {
            owner: test_event(1, 1, 100, "", vec![]).pubkey,
            member_percent_ceiling: 10,
            max_sats_per_feed: 100,
            max_sats_per_24h: 1000,
        }
    }
    async fn paid(store: &LedgerStore, id: &str, sats: u64) {
        store
            .record_payment(&SettledPayment {
                source: "strike".into(),
                source_id: id.into(),
                payment_hash: None,
                address_user: "herd".into(),
                credit_pool: "herd".into(),
                amount_msat: sats * 1000,
                settled_at: None,
                context_json: None,
            })
            .await
            .unwrap();
    }
    async fn virtual_credit(store: &LedgerStore, id: &str, sats: u64) {
        store
            .grant_operator_credit(
                &OperatorCreditGrant {
                    id: id.into(),
                    amount_sats: sats,
                    address_user: "herd".into(),
                    reason: "Synthetic reward funding fixture".into(),
                },
                false,
            )
            .await
            .unwrap();
    }
    async fn feed(store: &LedgerStore, sats: u64) -> Uuid {
        let id = store.begin_feed_attempt(sats).await.unwrap().unwrap();
        store.confirm_feed_attempt(id).await.unwrap();
        id
    }
    async fn funding(store: &LedgerStore, id: Uuid) -> Option<Funding> {
        let mut tx = store.pool.begin().await.unwrap();
        let value = confirmed_funding(&mut tx, id).await.unwrap();
        tx.commit().await.unwrap();
        value
    }
    async fn reserve(
        store: &LedgerStore,
        policy: &RewardPolicy,
        id: Uuid,
        amount: u64,
    ) -> Result<Reservation> {
        let epoch = store.overlay_stream_id().await?;
        store
            .reserve_reward(policy, epoch, id, &"ab".repeat(32), amount)
            .await
    }

    #[tokio::test]
    async fn disabled_tracking_and_activation_never_backpay_historical_feedings() {
        let (_root, _url, store) = store().await;
        paid(&store, "old", 1000).await;
        virtual_credit(&store, "old-virtual", 300).await;
        let old = feed(&store, 1000).await;
        let before = store.latest_event_seq().await.unwrap();
        assert!(funding(&store, old).await.is_none());
        assert!(reserve(&store, &policy(), old, 1).await.is_err());
        store.initialize_reward_funding().await.unwrap();
        store.initialize_reward_funding().await.unwrap();
        assert_eq!(store.latest_event_seq().await.unwrap(), before);
        assert_eq!(store.feed_credit_sats().await.unwrap(), 300);
        assert!(funding(&store, old).await.is_none());
        paid(&store, "new", 700).await;
        let new = feed(&store, 1000).await;
        assert_eq!(funding(&store, new).await.unwrap().strike_sats, 700);
        assert!(reserve(&store, &policy(), old, 1).await.is_err());
        assert_eq!(
            reserve(&store, &policy(), new, 70)
                .await
                .unwrap()
                .reserved_sats,
            70
        );
        assert_eq!(store.feed_credit_sats().await.unwrap(), 0);
    }

    #[tokio::test]
    async fn fifo_mixed_credit_exports_only_consumed_strike_funds_and_survives_reopen() {
        let (_root, url, store) = store().await;
        store.initialize_reward_funding().await.unwrap();
        virtual_credit(&store, "first", 300).await;
        paid(&store, "second", 1000).await;
        let first = feed(&store, 1000).await;
        let proof = funding(&store, first).await.unwrap();
        assert_eq!(proof.strike_sats, 700);
        assert!(reserve(&store, &policy(), first, 71).await.is_err());
        let reserved = reserve(&store, &policy(), first, 70).await.unwrap();
        assert_eq!(
            reserve(&store, &policy(), first, 70).await.unwrap(),
            reserved
        );
        assert!(reserve(&store, &policy(), first, 69).await.is_err());
        drop(store);
        let store = LedgerStore::connect(&url).await.unwrap();
        store.initialize_reward_funding().await.unwrap();
        virtual_credit(&store, "third", 700).await;
        let second = feed(&store, 1000).await;
        assert_eq!(funding(&store, second).await.unwrap().strike_sats, 300);
        assert_eq!(funding(&store, first).await.unwrap(), proof);
        assert_eq!(
            reserve(&store, &policy(), first, 70).await.unwrap(),
            reserved
        );
        assert_eq!(
            reserve(&store, &policy(), second, 30)
                .await
                .unwrap()
                .reserved_sats,
            30
        );
        let exported = store.core_event_batch(None, 0, 128).await.unwrap();
        let values: Vec<_> = exported
            .events
            .iter()
            .filter_map(|event| match &event.body {
                crate::core_bridge::EventBody::Feeding(f) => {
                    f.funding.as_ref().map(|v| v.strike_sats)
                }
                _ => None,
            })
            .collect();
        assert_eq!(values, vec![700, 300]);
    }

    #[tokio::test]
    async fn only_confirmed_feeding_can_reserve_and_failed_funding_commit_preserves_uuid_and_credit()
     {
        let (_root, _url, store) = store().await;
        store.initialize_reward_funding().await.unwrap();
        paid(&store, "real", 1000).await;
        let id = store.begin_feed_attempt(1000).await.unwrap().unwrap();
        assert!(reserve(&store, &policy(), id, 1).await.is_err());
        let before = store.latest_event_seq().await.unwrap();
        sqlx::query("CREATE TRIGGER fail_funding BEFORE INSERT ON feed_reward_funding BEGIN SELECT RAISE(ABORT,'synthetic funding failure'); END").execute(&store.pool).await.unwrap();
        assert!(store.confirm_feed_attempt(id).await.is_err());
        assert_eq!(store.feed_credit_sats().await.unwrap(), 1000);
        assert_eq!(store.latest_event_seq().await.unwrap(), before);
        assert_eq!(
            store.unresolved_feed_attempt().await.unwrap().unwrap().id,
            id
        );
        assert!(funding(&store, id).await.is_none());
        let parts: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM feed_reward_funding_parts")
            .fetch_one(&store.pool)
            .await
            .unwrap();
        assert_eq!(parts, 0);
        sqlx::query("DROP TRIGGER fail_funding")
            .execute(&store.pool)
            .await
            .unwrap();
        store
            .mark_feed_unknown(id, "synthetic completion write failure")
            .await
            .unwrap();
        assert!(reserve(&store, &policy(), id, 1).await.is_err());
        store.reconcile_unknown_as_fed(id).await.unwrap();
        assert_eq!(funding(&store, id).await.unwrap().strike_sats, 1000);
        assert_eq!(store.feed_credit_sats().await.unwrap(), 0);
        assert!(store.confirm_feed_attempt(id).await.is_err());
        assert_eq!(
            reserve(&store, &policy(), id, 100)
                .await
                .unwrap()
                .reserved_sats,
            100
        );
        for query in [
            "DELETE FROM reward_funding_state",
            "DELETE FROM reward_funding_lots",
            "UPDATE reward_funding_lots SET remaining_sats=remaining_sats+1",
            "UPDATE feed_reward_funding SET strike_sats=0",
            "DELETE FROM feed_reward_funding_parts",
            "UPDATE reward_reservations SET reserved_sats=1",
            "DELETE FROM reward_reservations",
        ] {
            assert!(
                sqlx::query(query).execute(&store.pool).await.is_err(),
                "{query}"
            );
        }
    }

    #[tokio::test]
    async fn operator_credit_other_provider_other_pool_and_uncorrelated_rows_never_authorize_strike_rewards()
     {
        let (_root, _url, store) = store().await;
        store.initialize_reward_funding().await.unwrap();
        virtual_credit(&store, "virtual", 1000).await;
        let virtual_feed = feed(&store, 1000).await;
        assert_eq!(funding(&store, virtual_feed).await.unwrap().strike_sats, 0);
        assert!(reserve(&store, &policy(), virtual_feed, 1).await.is_err());
        for (id, source, pool) in [
            ("other-provider", "other", "herd"),
            ("other-pool", "strike", "other"),
        ] {
            store
                .record_payment(&SettledPayment {
                    source: source.into(),
                    source_id: id.into(),
                    payment_hash: None,
                    address_user: "herd".into(),
                    credit_pool: pool.into(),
                    amount_msat: 1_000_000,
                    settled_at: None,
                    context_json: None,
                })
                .await
                .unwrap();
            let id = feed(&store, 1000).await;
            assert_eq!(funding(&store, id).await.unwrap().strike_sats, 0);
        }
        sqlx::query("INSERT INTO ledger_entries(entry_type,source_key,delta_sats,payment_source,payment_source_id,address_user,credit_pool) VALUES('HERD_RECEIPT','payment:strike:forged',1000,'strike','forged','herd','herd')").execute(&store.pool).await.unwrap();
        let id = feed(&store, 1000).await;
        assert_eq!(funding(&store, id).await.unwrap().strike_sats, 0);
        assert!(reserve(&store, &policy(), id, 1).await.is_err());
    }

    #[tokio::test]
    async fn reservations_serialize_one_owner_feed_budget_and_global_rolling_limits() {
        let (_root, _url, store) = store().await;
        store.initialize_reward_funding().await.unwrap();
        paid(&store, "real", 3000).await;
        let id = feed(&store, 1000).await;
        let epoch = store.overlay_stream_id().await.unwrap();
        let policy = policy();
        let hash1 = "ab".repeat(32);
        let hash2 = "cd".repeat(32);
        let (one, two) = tokio::join!(
            store.reserve_reward(&policy, epoch, id, &hash1, 100),
            store.reserve_reward(&policy, epoch, id, &hash2, 100)
        );
        assert_ne!(one.is_ok(), two.is_ok());
        let existing = one.or(two).unwrap();
        let other_owner = RewardPolicy {
            owner: test_event(2, 1, 100, "", vec![]).pubkey,
            ..policy.clone()
        };
        assert!(
            store
                .reserve_reward(&other_owner, epoch, id, &existing.plan_hash, 100)
                .await
                .is_err()
        );
        let next = feed(&store, 1000).await;
        let limited = RewardPolicy {
            max_sats_per_24h: 100,
            ..policy.clone()
        };
        assert!(reserve(&store, &limited, next, 1).await.is_err());
        for disabled in [
            RewardPolicy {
                member_percent_ceiling: 0,
                ..policy.clone()
            },
            RewardPolicy {
                max_sats_per_feed: 0,
                ..policy.clone()
            },
            RewardPolicy {
                max_sats_per_24h: 0,
                ..policy.clone()
            },
        ] {
            assert!(reserve(&store, &disabled, next, 1).await.is_err());
        }
        assert!(
            store
                .reserve_reward(&policy, Uuid::new_v4(), next, &hash1, 1)
                .await
                .is_err()
        );
        assert_eq!(store.feed_credit_sats().await.unwrap(), 1000);
    }

    #[tokio::test]
    async fn activation_rollback_corrupt_sources_and_untracked_debits_fail_without_manufacturing_history()
     {
        let (_root, _url, store) = store().await;
        paid(&store, "real", 1000).await;
        sqlx::query("CREATE TRIGGER fail_lot BEFORE INSERT ON reward_funding_lots BEGIN SELECT RAISE(ABORT,'synthetic bootstrap failure'); END").execute(&store.pool).await.unwrap();
        assert!(store.initialize_reward_funding().await.is_err());
        let initialized: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM reward_funding_state")
            .fetch_one(&store.pool)
            .await
            .unwrap();
        assert_eq!(initialized, 0);
        sqlx::query("DROP TRIGGER fail_lot")
            .execute(&store.pool)
            .await
            .unwrap();
        store.initialize_reward_funding().await.unwrap();
        sqlx::query("UPDATE ledger_entries SET payment_source_id='changed'")
            .execute(&store.pool)
            .await
            .unwrap();
        let id = store.begin_feed_attempt(1000).await.unwrap().unwrap();
        assert!(store.confirm_feed_attempt(id).await.is_err());
        assert_eq!(store.feed_credit_sats().await.unwrap(), 1000);
        assert_eq!(
            store.unresolved_feed_attempt().await.unwrap().unwrap().id,
            id
        );
        sqlx::query("UPDATE ledger_entries SET payment_source_id='real'")
            .execute(&store.pool)
            .await
            .unwrap();
        store.confirm_feed_attempt(id).await.unwrap();
        paid(&store, "second", 1000).await;
        sqlx::query("INSERT INTO ledger_entries(entry_type,source_key,delta_sats) VALUES('FEED_DEBIT','untracked',-1)").execute(&store.pool).await.unwrap();
        assert!(store.initialize_reward_funding().await.is_err());
        assert_eq!(store.feed_credit_sats().await.unwrap(), 999);
    }

    #[tokio::test]
    async fn old_outstanding_reservation_cannot_age_out_and_release_an_unknown_payment_budget() {
        let (_root, _url, store) = store().await;
        store.initialize_reward_funding().await.unwrap();
        paid(&store, "real", 2000).await;
        let first = feed(&store, 1000).await;
        let next = feed(&store, 1000).await;
        let epoch = store.overlay_stream_id().await.unwrap();
        let proof = funding(&store, first).await.unwrap();
        let limited = RewardPolicy {
            max_sats_per_24h: 100,
            ..policy()
        };
        // Synthetic restored history: this reservation was made two days ago,
        // but no authoritative terminal provider outcome has resolved it.
        sqlx::query("INSERT INTO reward_reservations(id,request_id,epoch,owner,plan_hash,funding_hash,policy_json,reserved_sats,created_at) VALUES(?,?,?,?,?,?,?,100,unixepoch()-172800)")
            .bind(Uuid::new_v4().to_string()).bind(first.to_string()).bind(epoch.to_string()).bind(&limited.owner).bind("ab".repeat(32)).bind(proof.proof_hash).bind(serde_json::to_string(&limited).unwrap()).execute(&store.pool).await.unwrap();
        assert!(reserve(&store, &limited, next, 1).await.is_err());
        assert_eq!(store.feed_credit_sats().await.unwrap(), 0);
    }
}
