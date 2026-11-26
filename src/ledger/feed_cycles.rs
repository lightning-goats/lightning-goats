//! Funding progress is independent of the physical request and proof custody.
//! An allocation records which receipt helped close a viewer goal. The existing
//! funding reservations continue to record the eligible proofs used at dispatch.
use super::{LedgerStore, append_event_in_transaction, feed_cap, to_i64, to_u64};
use crate::feeding_price::{Observation, SOURCE};
use anyhow::{Context, Result, ensure};
use serde::Serialize;
use serde_json::json;
use sqlx::{FromRow, Sqlite, Transaction};
use uuid::Uuid;

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct FeedGoal {
    pub cycle: Option<FeedCycle>,
    /// Fully funded cycles without a confirmed physical debit, including an
    /// unresolved attempt if one exists.
    pub funded_count: u64,
    pub queued_count: u64,
    pub unallocated_sats: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct FeedCycle {
    pub id: Uuid,
    pub target_sats: u64,
    pub credited_sats: u64,
    pub remaining_sats: u64,
    pub rate_usd: Option<String>,
    pub source: String,
    pub observed_at_ms: Option<i64>,
    pub opened_at: i64,
}

#[derive(FromRow)]
struct CycleRow {
    id: String,
    target_sats: i64,
    credited_sats: i64,
    rate_usd: Option<String>,
    source: String,
    observed_at_ms: Option<i64>,
    opened_at: i64,
}

impl TryFrom<CycleRow> for FeedCycle {
    type Error = anyhow::Error;

    fn try_from(row: CycleRow) -> Result<Self> {
        let target_sats = to_u64(row.target_sats, "cycle target")?;
        let credited_sats = to_u64(row.credited_sats, "cycle credit")?;
        Ok(Self {
            id: row.id.parse()?,
            target_sats,
            credited_sats,
            remaining_sats: target_sats
                .checked_sub(credited_sats)
                .context("cycle is overallocated")?,
            rate_usd: row.rate_usd,
            source: row.source,
            observed_at_ms: row.observed_at_ms,
            opened_at: row.opened_at,
        })
    }
}

pub(super) async fn enabled(tx: &mut Transaction<'_, Sqlite>) -> Result<bool> {
    sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM feed_cycle_policy)")
        .fetch_one(&mut **tx)
        .await
        .map_err(Into::into)
}

async fn open_cycle(tx: &mut Transaction<'_, Sqlite>) -> Result<Option<FeedCycle>> {
    sqlx::query_as::<_, CycleRow>(
        "SELECT id,target_sats,credited_sats,rate_usd,source,observed_at_ms,opened_at
         FROM feed_cycles WHERE funded_at IS NULL",
    )
    .fetch_optional(&mut **tx)
    .await?
    .map(FeedCycle::try_from)
    .transpose()
}

pub(super) async fn goal(tx: &mut Transaction<'_, Sqlite>) -> Result<Option<FeedGoal>> {
    if !enabled(tx).await? {
        return Ok(None);
    }
    let (funded, unallocated, awaiting): (i64, i64, bool) = sqlx::query_as(
        "SELECT (SELECT COUNT(*) FROM feed_cycles WHERE funded_at IS NOT NULL AND confirmed_attempt IS NULL),
                (SELECT COALESCE(SUM(sats),0) FROM feed_cycle_pending),
                EXISTS(SELECT 1 FROM feed_attempts WHERE status IN ('intent_committed','unknown'))",
    )
    .fetch_one(&mut **tx)
    .await?;
    let funded_count = to_u64(funded, "funded cycle count")?;
    Ok(Some(FeedGoal {
        cycle: open_cycle(tx).await?,
        funded_count,
        queued_count: funded_count.saturating_sub(u64::from(awaiting)),
        unallocated_sats: to_u64(unallocated, "unallocated contribution")?,
    }))
}

pub struct FeedProgress {
    pub credit_sats: u64,
    pub unresolved_attempt: Option<String>,
    pub goal: Option<FeedGoal>,
}

impl LedgerStore {
    /// The maintenance entry point accepts only the reviewed previous/current
    /// schema and never creates a new file or starts runtime workers.
    pub async fn migrate_feeding_cycle_schema(database_url: &str) -> Result<Self> {
        let pool = crate::sqlite::connect_existing_durable(database_url, 1).await?;
        let version: i64 = sqlx::query_scalar("SELECT MAX(version) FROM _sqlx_migrations")
            .fetch_one(&pool)
            .await?;
        ensure!(
            (40..=41).contains(&version),
            "funding cycles require ledger schema 40 or 41"
        );
        super::MIGRATOR.run(&pool).await?;
        Ok(Self {
            pool,
            publication: Default::default(),
            retained_wallet_epoch: None,
        })
    }

    pub async fn feed_progress(&self) -> Result<FeedProgress> {
        let mut tx = self.pool.begin().await?;
        let credit_sats = super::feed::feed_credit_in_transaction(&mut tx).await?;
        let unresolved_attempt = sqlx::query_scalar(
            "SELECT id FROM feed_attempts WHERE status IN ('intent_committed','unknown')",
        )
        .fetch_optional(&mut *tx)
        .await?;
        let goal = goal(&mut tx).await?;
        tx.commit().await?;
        Ok(FeedProgress {
            credit_sats,
            unresolved_attempt,
            goal,
        })
    }

    pub async fn feed_goal(&self) -> Result<Option<FeedGoal>> {
        let mut tx = self.pool.begin().await?;
        let result = goal(&mut tx).await?;
        tx.commit().await?;
        Ok(result)
    }

    /// Preserve unspent legacy lots and any unresolved physical UUID before
    /// letting the price reader open the first new cycle.
    pub async fn initialize_feed_cycles(&self, configured_threshold: u64) -> Result<()> {
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        if enabled(&mut tx).await? {
            tx.commit().await?;
            return Ok(());
        }
        let saved: Option<i64> =
            sqlx::query_scalar("SELECT threshold_sats FROM feeder_credit_policy WHERE singleton=1")
                .fetch_optional(&mut *tx)
                .await?;
        let legacy = match saved {
            Some(threshold) => to_u64(threshold, "legacy threshold")?,
            None => configured_threshold,
        };
        ensure!(legacy > 0, "missing legacy threshold");
        let now = sqlx::query_scalar::<_, i64>("SELECT unixepoch()")
            .fetch_one(&mut *tx)
            .await?;
        super::reward_funding::initialize(&mut tx).await?;
        sqlx::query(
            "INSERT INTO feed_cycle_policy SELECT 1,?,?,COALESCE(MAX(id),0) FROM ledger_entries",
        )
        .bind(now)
        .bind(to_i64(legacy, "legacy threshold")?)
        .execute(&mut *tx)
        .await?;
        seed_legacy_cycles(&mut tx, legacy, now).await?;
        tx.commit().await?;
        Ok(())
    }

    /// New observations only affect cycles opened after this transaction.
    pub async fn refresh_feed_price(&self, observation: &Observation) -> Result<()> {
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        ensure!(
            enabled(&mut tx).await?,
            "feeding cycles are not initialized"
        );
        let now = clock_ms(&mut tx).await?;
        if !observation.is_fresh(now) {
            tx.commit().await?;
            return Ok(());
        }
        let before = goal(&mut tx).await?;
        let changed = sqlx::query(
            "INSERT INTO feed_cycle_price VALUES(1,?,?,?,?)
             ON CONFLICT(singleton) DO UPDATE SET rate_usd=excluded.rate_usd,
             observed_at_ms=excluded.observed_at_ms,target_sats=excluded.target_sats
             WHERE excluded.observed_at_ms > feed_cycle_price.observed_at_ms",
        )
        .bind(observation.rate_usd())
        .bind(SOURCE)
        .bind(observation.observed_at_ms())
        .bind(to_i64(observation.target_sats(), "priced target")?)
        .execute(&mut *tx)
        .await?
        .rows_affected();
        if changed == 0 {
            tx.commit().await?;
            return Ok(());
        }
        allocate(&mut tx, now).await?;
        let after = goal(&mut tx).await?;
        if before != after {
            append_event_in_transaction(&mut tx, "feed_goal_changed", &json!({"feed_goal":after}))
                .await?;
        }
        super::reward_funding::prepare(&mut tx).await?;
        tx.commit().await?;
        Ok(())
    }
}

async fn seed_legacy_cycles(tx: &mut Transaction<'_, Sqlite>, legacy: u64, now: i64) -> Result<()> {
    let unresolved: Option<(String, i64)> = sqlx::query_as(
        "SELECT id,threshold_sats FROM feed_attempts WHERE status IN ('intent_committed','unknown')",
    )
    .fetch_optional(&mut **tx)
    .await?;
    let lots: Vec<(i64, i64)> = sqlx::query_as(
        "SELECT ledger_id,remaining_sats FROM reward_funding_lots
         WHERE remaining_sats>0 ORDER BY ledger_id",
    )
    .fetch_all(&mut **tx)
    .await?;
    let first_threshold = unresolved
        .as_ref()
        .map(|(_, threshold)| to_u64(*threshold, "unresolved legacy attempt threshold"))
        .transpose()?
        .unwrap_or(legacy);
    let mut cycle = None;
    let mut first_id = None;
    for (ledger, sats) in lots {
        let mut left = to_u64(sats, "unspent legacy credit")?;
        while left > 0 {
            if cycle.is_none() {
                let target = if first_id.is_none() {
                    first_threshold
                } else {
                    legacy
                };
                let id = Uuid::new_v4();
                sqlx::query("INSERT INTO feed_cycles(id,opened_at,target_sats,source) VALUES(?,?,?,'legacy')")
                    .bind(id.to_string())
                    .bind(now)
                    .bind(to_i64(target, "legacy cycle target")?)
                    .execute(&mut **tx)
                    .await?;
                first_id.get_or_insert(id);
                cycle = open_cycle(tx).await?;
            }
            let current = cycle.as_mut().context("missing legacy cycle")?;
            let sats = left.min(current.remaining_sats);
            add_allocation(tx, current, ledger, sats, now).await?;
            left -= sats;
            if current.remaining_sats == 0 {
                cycle = None;
            }
        }
    }
    if let Some((attempt, _)) = unresolved {
        let first = first_id.context("unresolved attempt has no legacy funding")?;
        let funded: bool =
            sqlx::query_scalar("SELECT funded_at IS NOT NULL FROM feed_cycles WHERE id=?")
                .bind(first.to_string())
                .fetch_one(&mut **tx)
                .await?;
        ensure!(funded, "unresolved attempt has insufficient legacy funding");
        bind_attempt(tx, &attempt, &first.to_string(), true).await?;
    }
    // Pending plans already have their own original source reservation. They
    // belong to the first funded cycle that does not already have an attempt.
    sqlx::query(
        "INSERT INTO feed_cycle_plans
         SELECT p.id,c.id FROM feed_financial_plans p JOIN feed_cycles c
         WHERE p.state='pending' AND c.funded_at IS NOT NULL
           AND c.seq=(SELECT min(seq) FROM feed_cycles
               WHERE funded_at IS NOT NULL AND NOT EXISTS(
                 SELECT 1 FROM feed_cycle_attempts a WHERE a.cycle_id=feed_cycles.id))
           AND p.threshold_sats=c.target_sats",
    )
    .execute(&mut **tx)
    .await?;
    let unmatched: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM feed_financial_plans p WHERE p.state='pending'
            AND NOT EXISTS(SELECT 1 FROM feed_cycle_plans b WHERE b.plan_id=p.id))",
    )
    .fetch_one(&mut **tx)
    .await?;
    ensure!(
        !unmatched,
        "pending plan has no matching grandfathered funding cycle"
    );
    Ok(())
}

pub(super) async fn next_funded(tx: &mut Transaction<'_, Sqlite>) -> Result<Option<(String, i64)>> {
    sqlx::query_as(
        "SELECT id,target_sats FROM feed_cycles
         WHERE funded_at IS NOT NULL AND confirmed_attempt IS NULL ORDER BY seq LIMIT 1",
    )
    .fetch_optional(&mut **tx)
    .await
    .map_err(Into::into)
}

pub(super) async fn bind_attempt(
    tx: &mut Transaction<'_, Sqlite>,
    attempt: &str,
    cycle: &str,
    migrated: bool,
) -> Result<()> {
    let matched = sqlx::query(
        "INSERT INTO feed_cycle_attempts
         SELECT a.id,c.id,? FROM feed_attempts a,feed_cycles c
         WHERE a.id=? AND c.id=? AND a.threshold_sats=c.target_sats
           AND c.funded_at IS NOT NULL AND c.confirmed_attempt IS NULL",
    )
    .bind(migrated)
    .bind(attempt)
    .bind(cycle)
    .execute(&mut **tx)
    .await?
    .rows_affected();
    ensure!(matched == 1, "attempt has no matching funded cycle");
    Ok(())
}

pub(super) async fn confirm(
    tx: &mut Transaction<'_, Sqlite>,
    attempt: Uuid,
) -> Result<Option<(String, bool)>> {
    if !enabled(tx).await? {
        return Ok(None);
    }
    let attempt = attempt.to_string();
    let (cycle, migrated): (String, bool) = sqlx::query_as(
        "SELECT cycle_id,migrated_attempt FROM feed_cycle_attempts WHERE attempt_id=?",
    )
    .bind(&attempt)
    .fetch_one(&mut **tx)
    .await?;
    let updated = sqlx::query(
        "UPDATE feed_cycles SET confirmed_attempt=? WHERE id=? AND confirmed_attempt IS NULL",
    )
    .bind(&attempt)
    .bind(&cycle)
    .execute(&mut **tx)
    .await?
    .rows_affected();
    ensure!(updated == 1, "funded cycle already confirmed");
    Ok(Some((cycle, migrated)))
}

pub(super) async fn confirmed_cycle(
    tx: &mut Transaction<'_, Sqlite>,
    attempt: Uuid,
) -> Result<Option<(String, bool)>> {
    sqlx::query_as(
        "SELECT c.id,a.migrated_attempt FROM feed_cycles c
        JOIN feed_cycle_attempts a ON a.attempt_id=c.confirmed_attempt AND a.cycle_id=c.id
        WHERE c.confirmed_attempt=?",
    )
    .bind(attempt.to_string())
    .fetch_optional(&mut **tx)
    .await
    .map_err(Into::into)
}

pub(super) async fn triggering_credit(
    tx: &mut Transaction<'_, Sqlite>,
    cycle: &str,
) -> Result<(i64, u64)> {
    let (ledger, amount): (i64, i64) = sqlx::query_as(
        "SELECT a.ledger_id,l.delta_sats FROM feed_cycle_allocations a
         JOIN ledger_entries l ON l.id=a.ledger_id WHERE a.cycle_id=? ORDER BY a.ledger_id DESC LIMIT 1",
    )
    .bind(cycle)
    .fetch_one(&mut **tx)
    .await?;
    Ok((ledger, to_u64(amount, "triggering payment")?))
}

pub(super) async fn credit_address_user(
    tx: &mut Transaction<'_, Sqlite>,
    credit: Option<(i64, u64)>,
) -> Result<Option<String>> {
    let Some((id, _)) = credit else {
        return Ok(None);
    };
    sqlx::query_scalar("SELECT address_user FROM ledger_entries WHERE id=?")
        .bind(id)
        .fetch_one(&mut **tx)
        .await
        .map_err(Into::into)
}

pub(super) async fn funded_by(tx: &mut Transaction<'_, Sqlite>, ledger: i64) -> Result<bool> {
    sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM feed_cycle_allocations a
       JOIN feed_cycles c ON c.id=a.cycle_id WHERE a.ledger_id=? AND c.funded_at IS NOT NULL
       AND NOT EXISTS(SELECT 1 FROM feed_cycle_allocations b WHERE b.cycle_id=a.cycle_id AND b.ledger_id>a.ledger_id))")
        .bind(ledger).fetch_one(&mut **tx).await.map_err(Into::into)
}

async fn clock_ms(tx: &mut Transaction<'_, Sqlite>) -> Result<i64> {
    sqlx::query_scalar("SELECT CAST(unixepoch('subsec')*1000 AS INTEGER)")
        .fetch_one(&mut **tx)
        .await
        .map_err(Into::into)
}

async fn open_priced(tx: &mut Transaction<'_, Sqlite>, now: i64) -> Result<Option<FeedCycle>> {
    let id = Uuid::new_v4().to_string();
    let opened = sqlx::query(
        "INSERT INTO feed_cycles(id,opened_at,target_sats,rate_usd,source,observed_at_ms)
         SELECT ?,?,target_sats,rate_usd,source,observed_at_ms FROM feed_cycle_price
         WHERE observed_at_ms BETWEEN ? AND ?",
    )
    .bind(&id)
    .bind(now.div_euclid(1000))
    .bind(now.saturating_sub(300_000))
    .bind(now.saturating_add(30_000))
    .execute(&mut **tx)
    .await?
    .rows_affected();
    if opened == 0 {
        return Ok(None);
    }
    open_cycle(tx).await
}

pub(super) async fn add_credit(
    tx: &mut Transaction<'_, Sqlite>,
    ledger_id: i64,
    sats: u64,
) -> Result<()> {
    sqlx::query("INSERT INTO feed_cycle_pending VALUES(?,?)")
        .bind(ledger_id)
        .bind(to_i64(sats, "pending contribution")?)
        .execute(&mut **tx)
        .await?;
    let now = clock_ms(tx).await?;
    allocate(tx, now).await
}

async fn allocate(tx: &mut Transaction<'_, Sqlite>, now: i64) -> Result<()> {
    let mut cycle = open_cycle(tx).await?;
    loop {
        let pending: Option<(i64, i64)> = sqlx::query_as(
            "SELECT ledger_id,sats FROM feed_cycle_pending ORDER BY ledger_id LIMIT 1",
        )
        .fetch_optional(&mut **tx)
        .await?;
        if let Some((ledger_id, sats)) = pending {
            let count: i64 =
                sqlx::query_scalar("SELECT COUNT(*) FROM feed_cycle_allocations WHERE ledger_id=?")
                    .bind(ledger_id)
                    .fetch_one(&mut **tx)
                    .await?;
            let sats = to_u64(sats, "pending contribution")?;
            if count >= 3 {
                feed_cap::retire(
                    tx,
                    &format!("payment-cap:{ledger_id}"),
                    ledger_id,
                    sats,
                    "three_feeding_cap",
                )
                .await?;
                consume_pending(tx, ledger_id, 0).await?;
                continue;
            }
        }
        if cycle.is_none() {
            cycle = open_priced(tx, now).await?;
        }
        let (Some(current), Some((ledger_id, pending_sats))) = (&mut cycle, pending) else {
            return Ok(());
        };
        let pending_sats = to_u64(pending_sats, "pending contribution")?;
        let allocated = pending_sats.min(current.remaining_sats);
        add_allocation(tx, current, ledger_id, allocated, now.div_euclid(1000)).await?;
        consume_pending(tx, ledger_id, pending_sats - allocated).await?;
        if current.remaining_sats == 0 {
            cycle = None;
        }
    }
}

async fn add_allocation(
    tx: &mut Transaction<'_, Sqlite>,
    cycle: &mut FeedCycle,
    ledger_id: i64,
    sats: u64,
    now: i64,
) -> Result<()> {
    ensure!(
        sats > 0 && sats <= cycle.remaining_sats,
        "invalid cycle allocation"
    );
    cycle.credited_sats += sats;
    cycle.remaining_sats -= sats;
    sqlx::query("INSERT INTO feed_cycle_allocations VALUES(?,?,?)")
        .bind(cycle.id.to_string())
        .bind(ledger_id)
        .bind(to_i64(sats, "allocated contribution")?)
        .execute(&mut **tx)
        .await?;
    sqlx::query("UPDATE feed_cycles SET credited_sats=?,funded_at=? WHERE id=?")
        .bind(to_i64(cycle.credited_sats, "cycle credit")?)
        .bind((cycle.remaining_sats == 0).then_some(now))
        .bind(cycle.id.to_string())
        .execute(&mut **tx)
        .await?;
    Ok(())
}

async fn consume_pending(
    tx: &mut Transaction<'_, Sqlite>,
    ledger_id: i64,
    left: u64,
) -> Result<()> {
    if left == 0 {
        sqlx::query("DELETE FROM feed_cycle_pending WHERE ledger_id=?")
            .bind(ledger_id)
            .execute(&mut **tx)
            .await?;
    } else {
        sqlx::query("UPDATE feed_cycle_pending SET sats=? WHERE ledger_id=?")
            .bind(to_i64(left, "remaining contribution")?)
            .bind(ledger_id)
            .execute(&mut **tx)
            .await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::payment::SettledPayment;

    fn price(rate: &str, seconds: i64) -> Result<Observation> {
        let now = chrono::DateTime::<chrono::Utc>::from(std::time::SystemTime::now()).timestamp();
        let time = chrono::DateTime::from_timestamp(now + seconds, 0).context("fixture time")?;
        let cache = json!({"market":{"source":SOURCE,"observed_at":time.to_rfc3339(),"price_usd_exact":rate}});
        Observation::from_cache(&serde_json::to_vec(&cache)?, now * 1000)
    }

    async fn receipt(ledger: &LedgerStore, id: &str, sats: u64) -> Result<()> {
        ledger
            .record_payment(&SettledPayment {
                source: "fixture".into(),
                source_id: id.into(),
                payment_hash: None,
                address_user: "dexter".into(),
                credit_pool: "herd".into(),
                amount_msat: sats * 1000,
                settled_at: None,
                context_json: None,
            })
            .await?;
        Ok(())
    }

    #[tokio::test]
    async fn locks_goal_across_refresh_restart_and_caps_a_single_receipt() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let url = format!("sqlite://{}", dir.path().join("feed.db").display());
        let ledger = LedgerStore::connect(&url).await?;
        ledger.initialize_feed_cycles(1000).await?;
        let quote = price("80000", -10)?;
        tokio::try_join!(
            ledger.refresh_feed_price(&quote),
            ledger.refresh_feed_price(&quote)
        )?;
        receipt(&ledger, "first", 760).await?;
        let goal = ledger.feed_goal().await?.context("priced mode")?;
        let cycle = goal.cycle.as_ref().context("open cycle")?;
        assert_eq!((cycle.target_sats, cycle.remaining_sats), (1250, 490));
        ledger.refresh_feed_price(&price("100000", 0)?).await?;
        assert_eq!(ledger.feed_goal().await?, Some(goal.clone()));
        ledger.pool.close().await;
        let ledger = LedgerStore::connect(&url).await?;
        ledger.initialize_feed_cycles(1000).await?;
        assert_eq!(ledger.feed_goal().await?, Some(goal));

        receipt(&ledger, "large", 3000).await?;
        receipt(&ledger, "large", 3000).await?;
        let assigned: Vec<i64> = sqlx::query_scalar(
            "SELECT a.sats FROM feed_cycle_allocations a JOIN feed_cycles c ON c.id=a.cycle_id
             JOIN ledger_entries l ON l.id=a.ledger_id WHERE l.payment_source_id='large' ORDER BY c.seq",
        )
        .fetch_all(&ledger.pool)
        .await?;
        assert_eq!(assigned, [490, 1000, 1000]);
        let retired: Vec<i64> =
            sqlx::query_scalar("SELECT retired_sats FROM feed_credit_retirements")
                .fetch_all(&ledger.pool)
                .await?;
        assert_eq!(retired, [510]);

        let goal = ledger.feed_goal().await?.context("priced mode")?;
        assert_eq!(goal.funded_count, 3);
        assert_eq!(goal.cycle.context("next cycle")?.remaining_sats, 1000);
        let refused = ledger
            .begin_feed_attempt(1000)
            .await?
            .context("funded cycle")?;
        ledger
            .mark_feed_unknown(refused, "fixture lost acknowledgement")
            .await?;
        assert!(ledger.begin_feed_attempt(1000).await.is_err());
        ledger.reconcile_unknown_as_not_fed(refused).await?;
        assert_eq!(
            ledger
                .feed_goal()
                .await?
                .context("priced mode")?
                .funded_count,
            3
        );
        let mut attempts = Vec::new();
        for _ in 0..3 {
            let id = ledger
                .begin_feed_attempt(1000)
                .await?
                .context("funded cycle")?;
            assert_ne!(id, refused);
            assert!(!attempts.contains(&id));
            attempts.push(id);
            ledger.confirm_feed_attempt(id).await?;
            assert!(ledger.confirm_feed_attempt(id).await.is_err());
        }
        let debits: Vec<i64> = sqlx::query_scalar(
            "SELECT delta_sats FROM ledger_entries WHERE entry_type='FEED_DEBIT' ORDER BY id",
        )
        .fetch_all(&ledger.pool)
        .await?;
        assert_eq!(debits, [-1250, -1000, -1000]);
        let announcements: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM feeding_announcement_sources")
                .fetch_one(&ledger.pool)
                .await?;
        assert_eq!(announcements, 1);
        assert_eq!(ledger.feed_credit_sats().await?, 0);
        Ok(())
    }

    #[tokio::test]
    async fn grandfathered_attempt_and_partial_progress_survive_price_outage() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let url = format!("sqlite://{}", dir.path().join("feed.db").display());
        let ledger = LedgerStore::connect(&url).await?;
        ledger.configure_feeder_credit_cap(1000).await?;
        receipt(&ledger, "legacy", 2340).await?;
        let attempt = ledger
            .begin_feed_attempt(1000)
            .await?
            .context("legacy attempt")?;
        ledger.initialize_feed_cycles(1000).await?;
        let goal = ledger.feed_goal().await?.context("priced mode")?;
        assert_eq!(goal.funded_count, 2);
        let cycle = goal.cycle.as_ref().context("legacy partial")?;
        assert_eq!(cycle.source, "legacy");
        assert_eq!((cycle.credited_sats, cycle.remaining_sats), (340, 660));
        ledger.initialize_feed_cycles(1000).await?;
        assert_eq!(ledger.feed_goal().await?, Some(goal));

        ledger.mark_interrupted_feed_intents_unknown().await?;
        assert!(ledger.begin_feed_attempt(1000).await.is_err());
        ledger.reconcile_unknown_as_fed(attempt).await?;
        receipt(&ledger, "during-outage", 3000).await?;
        receipt(&ledger, "later-donor", 20).await?;
        let goal = ledger.feed_goal().await?.context("priced mode")?;
        assert_eq!(goal.funded_count, 2);
        assert!(goal.cycle.is_none());
        assert_eq!(goal.unallocated_sats, 2360);
        ledger.pool.close().await;
        let ledger = LedgerStore::connect(&url).await?;
        assert_eq!(ledger.feed_goal().await?, Some(goal));

        ledger.refresh_feed_price(&price("100000", 0)?).await?;
        let goal = ledger.feed_goal().await?.context("priced mode")?;
        assert_eq!(goal.funded_count, 4);
        assert_eq!(goal.unallocated_sats, 0);
        assert_eq!(goal.cycle.context("new priced goal")?.credited_sats, 20);
        let retired: i64 = sqlx::query_scalar("SELECT retired_sats FROM feed_credit_retirements")
            .fetch_one(&ledger.pool)
            .await?;
        assert_eq!(retired, 340);
        assert_eq!(ledger.feed_credit_sats().await?, 4020);
        Ok(())
    }
}
