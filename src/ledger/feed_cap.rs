//! Per-payment food allocation. Receipt value and actual feeding debits remain
//! whole and distinct from auditable credit retired for operations.
use super::{
    LedgerStore, append_event_in_transaction, feed::feed_credit_in_transaction, to_i64, to_u64,
};
use anyhow::{Context, Result, bail};
use serde_json::json;
use sqlx::{Row, Sqlite, Transaction};

impl LedgerStore {
    pub async fn feeding_announcement_allowed(&self, seq: u64) -> Result<bool> {
        let row: Option<i64> = sqlx::query_scalar("SELECT f.first_event_seq FROM event_log e JOIN feeding_announcement_sources f ON f.credit_ledger_id=json_extract(e.payload_json,'$.triggering_credit_id') WHERE e.seq=? AND e.event_type='feeder_confirmed'")
            .bind(to_i64(seq,"announcement sequence")?).fetch_optional(&self.pool).await?;
        Ok(row.is_none_or(|first| u64::try_from(first).ok() == Some(seq)))
    }
    /// Startup policy, before any public settlement or feeder worker starts.
    /// It does not reinterpret or clear existing paid credit.
    pub async fn configure_feeder_credit_cap(&self, threshold: u64) -> Result<()> {
        let threshold = to_i64(threshold, "feeder cap threshold")?;
        if threshold <= 0 || threshold > i64::MAX / 3 {
            bail!("invalid three-feeding threshold");
        }
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        sqlx::query(
            "INSERT INTO feeder_credit_policy VALUES(1,?,3) ON CONFLICT(singleton) DO NOTHING",
        )
        .bind(threshold)
        .execute(&mut *tx)
        .await?;
        let saved: i64 =
            sqlx::query_scalar("SELECT threshold_sats FROM feeder_credit_policy WHERE singleton=1")
                .fetch_one(&mut *tx)
                .await?;
        if saved != threshold {
            bail!("feeder threshold changed; explicit credit-policy migration required");
        }
        tx.commit().await?;
        Ok(())
    }
}

pub(super) async fn capped_total(
    tx: &mut Transaction<'_, Sqlite>,
    before: u64,
    amount: u64,
) -> Result<u64> {
    let threshold: Option<i64> =
        sqlx::query_scalar("SELECT threshold_sats FROM feeder_credit_policy WHERE singleton=1")
            .fetch_optional(&mut **tx)
            .await?;
    let accepted = if let Some(threshold) = threshold {
        let threshold = to_u64(threshold, "cap threshold")?;
        amount.min(
            threshold
                .checked_mul(3)
                .and_then(|n| n.checked_sub(before % threshold))
                .context("feeding cap overflow")?,
        )
    } else {
        amount
    };
    let after = before
        .checked_add(accepted)
        .context("capped credit overflow")?;
    to_i64(after, "capped credit")?;
    Ok(after)
}

/// Called after the original positive credit, before the public payment event.
/// Historical test/import writers without a configured live policy retain their
/// old behavior. Production daemon and operator grant CLI always configure it.
pub(super) async fn cap_new_credit(
    tx: &mut Transaction<'_, Sqlite>,
    credit_id: i64,
    amount: u64,
) -> Result<u64> {
    let total = feed_credit_in_transaction(tx).await?;
    let before = total
        .checked_sub(amount)
        .context("new credit exceeds aggregate balance")?;
    let accepted = capped_total(tx, before, amount)
        .await?
        .checked_sub(before)
        .context("invalid capped credit")?;
    if accepted < amount {
        retire(
            tx,
            &format!("payment-cap:{credit_id}"),
            credit_id,
            amount - accepted,
            "three_feeding_cap",
        )
        .await?;
    }
    feed_credit_in_transaction(tx).await
}

pub(super) async fn retire(
    tx: &mut Transaction<'_, Sqlite>,
    id: &str,
    credit_id: i64,
    sats: u64,
    reason: &str,
) -> Result<()> {
    if sats == 0 || id.is_empty() || id.len() > 256 {
        bail!("invalid credit retirement");
    }
    let before = feed_credit_in_transaction(tx).await?;
    let after = before
        .checked_sub(sats)
        .context("credit retirement exceeds remaining balance")?;
    let debit=sqlx::query("INSERT INTO ledger_entries(entry_type,source_key,delta_sats) VALUES('FEED_CREDIT_RETIRED',?,?)")
        .bind(format!("feed-credit-retirement:{id}")).bind(-to_i64(sats,"retired credit")?).execute(&mut **tx).await?.last_insert_rowid();
    let event = append_event_in_transaction(
        tx,
        "feed_credit_retired",
        &json!({"retired_sats":sats,"feed_credit_sats":after,"reason":reason}),
    )
    .await?;
    sqlx::query("INSERT INTO feed_credit_retirements(id,credit_ledger_id,debit_ledger_id,retired_sats,reason,event_seq) VALUES(?,?,?,?,?,?)")
        .bind(id).bind(credit_id).bind(debit).bind(to_i64(sats,"retired credit")?).bind(reason).bind(to_i64(event,"retirement event")?).execute(&mut **tx).await?;
    Ok(())
}

/// Apply only a retained, source-bound retirement to its exact funding lot.
/// It creates no feed/reward proof and never consumes another payment's funds.
pub(super) async fn consume_retirement(
    tx: &mut Transaction<'_, Sqlite>,
    debit: i64,
) -> Result<bool> {
    let row=sqlx::query("SELECT r.credit_ledger_id,r.retired_sats,l.delta_sats,l.entry_type,l.source_key,r.id FROM feed_credit_retirements r JOIN ledger_entries l ON l.id=r.debit_ledger_id WHERE r.debit_ledger_id=?")
        .bind(debit).fetch_optional(&mut **tx).await?;
    let Some(row) = row else {
        let kind: String = sqlx::query_scalar("SELECT entry_type FROM ledger_entries WHERE id=?")
            .bind(debit)
            .fetch_one(&mut **tx)
            .await?;
        if kind == "FEED_CREDIT_RETIRED" {
            bail!("credit retirement is missing its retained provenance");
        }
        return Ok(false);
    };
    let sats: i64 = row.try_get("retired_sats")?;
    if sats <= 0
        || row.try_get::<i64, _>("delta_sats")? != -sats
        || row.try_get::<String, _>("entry_type")? != "FEED_CREDIT_RETIRED"
        || row.try_get::<String, _>("source_key")?
            != format!("feed-credit-retirement:{}", row.try_get::<String, _>("id")?)
    {
        bail!("credit retirement provenance changed");
    }
    let changed=sqlx::query("UPDATE reward_funding_lots SET remaining_sats=remaining_sats-? WHERE ledger_id=? AND remaining_sats>=?")
        .bind(sats).bind(row.try_get::<i64,_>("credit_ledger_id")?).bind(sats).execute(&mut **tx).await?.rows_affected();
    if changed != 1 {
        bail!("retirement exceeds its original unspent funding lot");
    }
    Ok(true)
}

#[derive(Debug, serde::Serialize)]
pub struct CreditRetirementResult {
    pub applied: bool,
    pub duplicate: bool,
    pub retired_sats: u64,
    pub feed_credit_sats: u64,
}
impl LedgerStore {
    /// Root-only offline entry point. Validate the existing non-admin owner's
    /// files separately; ordinary runtime recovery continues to reject root.
    /// Never creates a journal or migrates schema. Always close SQLite before
    /// returning, including failed previews, so runtime-owned files stay usable.
    #[cfg(unix)]
    pub async fn retire_credit_offline(
        database_url: &str,
        id: &str,
        credit_id: i64,
        expected: u64,
        dry_run: bool,
    ) -> Result<CreditRetirementResult> {
        use std::os::unix::fs::MetadataExt as _;
        if crate::private_ipc::uid()? != 0 {
            bail!("offline credit retirement requires root");
        }
        let options = crate::sqlite::durable_options(database_url)?;
        let path = options.get_filename();
        let parent = path.parent().context("missing retirement journal parent")?;
        let owner = std::fs::symlink_metadata(parent)?;
        if !path.is_absolute()
            || path.canonicalize()? != path
            || !owner.is_dir()
            || owner.uid() == 0
            || owner.gid() == 0
            || owner.mode() & 0o077 != 0
        {
            bail!("retirement requires the existing private non-admin journal owner");
        }
        for ancestor in parent.ancestors().skip(1) {
            let meta = std::fs::symlink_metadata(ancestor)?;
            if !meta.is_dir() || meta.uid() != 0 || meta.mode() & 0o022 != 0 {
                bail!("retirement journal ancestors must remain protected");
            }
        }
        let check = || -> Result<()> {
            for suffix in ["", "-wal", "-shm"] {
                let file = std::path::PathBuf::from(format!("{}{suffix}", path.display()));
                let meta = match std::fs::symlink_metadata(&file) {
                    Ok(meta) => meta,
                    Err(e) if !suffix.is_empty() && e.kind() == std::io::ErrorKind::NotFound => {
                        continue;
                    }
                    Err(e) => return Err(e.into()),
                };
                if !meta.is_file()
                    || meta.nlink() != 1
                    || meta.uid() != owner.uid()
                    || meta.gid() != owner.gid()
                    || meta.mode() & 0o077 != 0
                {
                    bail!("retirement journal file ownership/type changed");
                }
            }
            Ok(())
        };
        check()?;
        let pool = crate::sqlite::connect_existing_durable(database_url, 1).await?;
        let store = Self { pool };
        let result = async {
            crate::sqlite::require_current_schema(&store.pool, &super::MIGRATOR).await?;
            store
                .retire_existing_feed_credit(id, credit_id, expected, dry_run)
                .await
        }
        .await;
        store.pool.close().await;
        check()?;
        result
    }
    /// Offline/operator-only correction. Retire one exact original lot, never
    /// another donor's credit and never historical confirmed food.
    pub async fn retire_existing_feed_credit(
        &self,
        id: &str,
        credit_id: i64,
        expected: u64,
        dry_run: bool,
    ) -> Result<CreditRetirementResult> {
        if expected == 0
            || id.is_empty()
            || id.len() > 256
            || !id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-_:".contains(&b))
        {
            bail!("invalid exact credit-retirement request");
        }
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let old = sqlx::query(
            "SELECT credit_ledger_id,retired_sats,reason FROM feed_credit_retirements WHERE id=?",
        )
        .bind(id)
        .fetch_optional(&mut *tx)
        .await?;
        if let Some(row) = old {
            if row.try_get::<i64, _>("credit_ledger_id")? != credit_id
                || to_u64(row.try_get("retired_sats")?, "retired amount")? != expected
                || row.try_get::<String, _>("reason")? != "operator_backlog_correction"
            {
                bail!("retirement retry changed frozen identity/amount");
            }
            let credit = feed_credit_in_transaction(&mut tx).await?;
            tx.commit().await?;
            return Ok(CreditRetirementResult {
                applied: false,
                duplicate: true,
                retired_sats: expected,
                feed_credit_sats: credit,
            });
        }
        let pending:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM feed_attempts WHERE status IN ('intent_committed','unknown'))")
            .fetch_one(&mut *tx).await?;
        if pending {
            bail!("cannot retire credit with an unresolved physical attempt");
        }
        if !super::reward_funding::prepare(&mut tx).await? {
            bail!("original funding history must be initialized and verified first");
        }
        let remaining: i64 =
            sqlx::query_scalar("SELECT remaining_sats FROM reward_funding_lots WHERE ledger_id=?")
                .bind(credit_id)
                .fetch_one(&mut *tx)
                .await?;
        if to_u64(remaining, "original unspent lot")? != expected {
            bail!("original unspent credit changed; review exact amount again");
        }
        let after = feed_credit_in_transaction(&mut tx)
            .await?
            .checked_sub(expected)
            .context("retirement exceeds aggregate credit")?;
        if dry_run {
            tx.rollback().await?;
            return Ok(CreditRetirementResult {
                applied: false,
                duplicate: false,
                retired_sats: expected,
                feed_credit_sats: after,
            });
        }
        retire(
            &mut tx,
            id,
            credit_id,
            expected,
            "operator_backlog_correction",
        )
        .await?;
        super::reward_funding::prepare(&mut tx).await?;
        tx.commit().await?;
        Ok(CreditRetirementResult {
            applied: true,
            duplicate: false,
            retired_sats: expected,
            feed_credit_sats: after,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ledger::OperatorCreditGrant;
    use tempfile::TempDir;
    async fn open() -> (TempDir, String, LedgerStore) {
        let root = TempDir::new().unwrap();
        let url = format!("sqlite://{}", root.path().join("cap.db").display());
        let store = LedgerStore::connect(&url).await.unwrap();
        store.initialize_reward_funding().await.unwrap();
        (root, url, store)
    }
    async fn grant(store: &LedgerStore, id: &str, sats: u64) {
        store
            .grant_operator_credit(
                &OperatorCreditGrant {
                    id: id.into(),
                    amount_sats: sats,
                    address_user: "herd".into(),
                    reason: "Synthetic cap regression".into(),
                },
                false,
            )
            .await
            .unwrap();
    }
    async fn feed(store: &LedgerStore) -> bool {
        let Some(id) = store.begin_feed_attempt(1000).await.unwrap() else {
            return false;
        };
        store.confirm_feed_attempt(id).await.unwrap();
        true
    }
    #[tokio::test]
    #[ignore = "required isolated root credit-retirement fixture"]
    async fn root_retirement_preserves_runtime_owner_and_exact_paid_history() {
        use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _};
        assert_eq!(crate::private_ipc::uid().unwrap(), 0);
        let root = tempfile::Builder::new()
            .prefix("lg-retirement-")
            .tempdir_in("/run")
            .unwrap();
        std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let directory = root.path().join("runtime");
        std::fs::create_dir(&directory).unwrap();
        std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o700)).unwrap();
        let path = directory.join("core.db");
        let url = format!("sqlite://{}", path.display());
        let store = LedgerStore::connect(&url).await.unwrap();
        store.initialize_reward_funding().await.unwrap();
        grant(&store, "saved", 70).await;
        grant(&store, "original-donation", 34031).await;
        for _ in 0..10 {
            assert!(feed(&store).await);
        }
        let credit: i64 = sqlx::query_scalar(
            "SELECT id FROM ledger_entries WHERE source_key='operator-credit:original-donation'",
        )
        .fetch_one(&store.pool)
        .await
        .unwrap();
        store.pool.close().await;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        std::os::unix::fs::chown(&path, Some(50119), Some(50119)).unwrap();
        std::os::unix::fs::chown(&directory, Some(50119), Some(50119)).unwrap();
        assert!(LedgerStore::connect_existing_recovery(&url).await.is_err());
        let preview = LedgerStore::retire_credit_offline(&url, "reviewed", credit, 24101, true)
            .await
            .unwrap();
        assert!(!preview.applied);
        assert_eq!(preview.feed_credit_sats, 0);
        let applied = LedgerStore::retire_credit_offline(&url, "reviewed", credit, 24101, false)
            .await
            .unwrap();
        assert!(applied.applied);
        assert_eq!(applied.feed_credit_sats, 0);
        let retry = LedgerStore::retire_credit_offline(&url, "reviewed", credit, 24101, false)
            .await
            .unwrap();
        assert!(retry.duplicate);
        assert_eq!(std::fs::metadata(&path).unwrap().uid(), 50119);
        assert_eq!(std::fs::metadata(&path).unwrap().gid(), 50119);
        let pool = crate::sqlite::connect_existing_durable(&url, 1)
            .await
            .unwrap();
        let counts:(i64,i64,i64)=sqlx::query_as("SELECT (SELECT COUNT(*) FROM feed_attempts WHERE status='confirmed'),(SELECT COUNT(*) FROM feed_credit_retirements),(SELECT SUM(delta_sats) FROM ledger_entries WHERE delta_sats>0)").fetch_one(&pool).await.unwrap();
        pool.close().await;
        assert_eq!(counts, (10, 1, 34101));
        println!("ROOT_CREDIT_RETIREMENT_FIXTURE_VERIFIED");
    }
    #[tokio::test]
    async fn donation_34031_with_saved_70_queues_three_and_emits_one_feeding_notice() {
        let (_root, url, store) = open().await;
        store.configure_feeder_credit_cap(1000).await.unwrap();
        grant(&store, "saved", 70).await;
        grant(&store, "donation", 34031).await;
        assert_eq!(store.feed_credit_sats().await.unwrap(), 3000);
        let reopened = LedgerStore::connect(&url).await.unwrap();
        for _ in 0..3 {
            assert!(feed(&reopened).await);
        }
        assert!(!feed(&reopened).await);
        assert_eq!(reopened.feed_credit_sats().await.unwrap(), 0);
        let renderer = crate::presentation::MessageRenderer::embedded().unwrap();
        let events = reopened.events_after(0, 100).await.unwrap();
        let feeds: Vec<_> = events
            .iter()
            .filter(|e| e.event_type == "feeder_confirmed")
            .collect();
        assert_eq!(feeds.len(), 3);
        let mut messages = 0;
        for event in feeds {
            let rendered = renderer.render(event, 1000).unwrap();
            if let Some(text) = rendered.nostr_content {
                assert!(text.contains("34031") || text.contains("34,031"));
                messages += 1;
            }
        }
        assert_eq!(messages, 1);
        reopened.initialize_reward_funding().await.unwrap();
        let db = sqlx::SqlitePool::connect(&url).await.unwrap();
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT SUM(remaining_sats) FROM reward_funding_lots")
                .fetch_one(&db)
                .await
                .unwrap(),
            0
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT SUM(retired_sats) FROM feed_credit_retirements")
                .fetch_one(&db)
                .await
                .unwrap(),
            31101
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT SUM(total_sats) FROM feed_reward_funding")
                .fetch_one(&db)
                .await
                .unwrap(),
            3000
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT delta_sats FROM ledger_entries WHERE source_key='operator-credit:donation'"
            )
            .fetch_one(&db)
            .await
            .unwrap(),
            34031
        );
        grant(&reopened, "later", 1000).await;
        assert_eq!(reopened.feed_credit_sats().await.unwrap(), 1000);
        assert!(feed(&reopened).await);
    }
    #[tokio::test]
    async fn native_settlement_and_cap_retirement_are_atomic_and_replay_safe() {
        let (_root, url, store) = open().await;
        store.configure_feeder_credit_cap(1000).await.unwrap();
        grant(&store, "saved", 70).await;
        let payment = crate::domain::payment::SettledPayment {
            source: "strike".into(),
            source_id: uuid::Uuid::new_v4().to_string(),
            payment_hash: None,
            address_user: "herd".into(),
            credit_pool: "herd".into(),
            amount_msat: 34031000,
            settled_at: None,
            context_json: None,
        };
        let db = sqlx::SqlitePool::connect(&url).await.unwrap();
        sqlx::query("CREATE TRIGGER fail_retirement BEFORE INSERT ON feed_credit_retirements BEGIN SELECT RAISE(ABORT,'synthetic late cap failure'); END")
            .execute(&db).await.unwrap();
        assert!(store.record_payment(&payment).await.is_err());
        assert_eq!(store.feed_credit_sats().await.unwrap(), 70);
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM settled_payments")
                .fetch_one(&db)
                .await
                .unwrap(),
            0
        );
        sqlx::query("DROP TRIGGER fail_retirement")
            .execute(&db)
            .await
            .unwrap();
        store.record_payment(&payment).await.unwrap();
        assert!(matches!(
            store.record_payment(&payment).await.unwrap(),
            crate::ledger::SettlementOutcome::Duplicate
        ));
        assert_eq!(store.feed_credit_sats().await.unwrap(), 3000);
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT amount_msat FROM settled_payments")
                .fetch_one(&db)
                .await
                .unwrap(),
            34031000
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM feed_credit_retirements")
                .fetch_one(&db)
                .await
                .unwrap(),
            1
        );
        for _ in 0..3 {
            assert!(feed(&store).await);
        }
        assert!(!feed(&store).await);
        assert_eq!(store.feed_credit_sats().await.unwrap(), 0);
    }

    #[tokio::test]
    async fn smaller_payments_keep_progress_and_concurrent_receipts_keep_separate_caps() {
        let (_root, _url, store) = open().await;
        store.configure_feeder_credit_cap(1000).await.unwrap();
        grant(&store, "small", 2340).await;
        assert!(feed(&store).await);
        assert!(feed(&store).await);
        assert_eq!(store.feed_credit_sats().await.unwrap(), 340);
        grant(&store, "big-a", 34031).await;
        grant(&store, "big-b", 50000).await;
        assert_eq!(store.feed_credit_sats().await.unwrap(), 6000);
        for _ in 0..6 {
            assert!(feed(&store).await);
        }
        assert!(!feed(&store).await);
        assert_eq!(store.feed_credit_sats().await.unwrap(), 0);
    }
    #[tokio::test]
    async fn exact_legacy_backlog_retirement_preserves_later_credit_and_actual_ten_feeds() {
        let (_root, url, store) = open().await;
        // Deliberately reproduce the old uncapped history before live policy.
        grant(&store, "saved", 70).await;
        grant(&store, "donation", 34031).await;
        for _ in 0..10 {
            assert!(feed(&store).await);
        }
        grant(&store, "later", 123).await;
        let db = sqlx::SqlitePool::connect(&url).await.unwrap();
        let credit: i64 = sqlx::query_scalar(
            "SELECT id FROM ledger_entries WHERE source_key='operator-credit:donation'",
        )
        .fetch_one(&db)
        .await
        .unwrap();
        assert!(
            store
                .retire_existing_feed_credit("repair", credit, 24000, true)
                .await
                .is_err()
        );
        let preview = store
            .retire_existing_feed_credit("repair", credit, 24101, true)
            .await
            .unwrap();
        assert!(!preview.applied);
        assert_eq!(preview.feed_credit_sats, 123);
        assert_eq!(store.feed_credit_sats().await.unwrap(), 24224);
        let applied = store
            .retire_existing_feed_credit("repair", credit, 24101, false)
            .await
            .unwrap();
        assert!(applied.applied);
        assert_eq!(applied.feed_credit_sats, 123);
        let reopened = LedgerStore::connect(&url).await.unwrap();
        assert!(
            reopened
                .retire_existing_feed_credit("repair", credit, 24101, false)
                .await
                .unwrap()
                .duplicate
        );
        reopened.initialize_reward_funding().await.unwrap();
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM feed_attempts WHERE status='confirmed'"
            )
            .fetch_one(&db)
            .await
            .unwrap(),
            10
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT SUM(remaining_sats) FROM reward_funding_lots")
                .fetch_one(&db)
                .await
                .unwrap(),
            123
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM feed_credit_retirements")
                .fetch_one(&db)
                .await
                .unwrap(),
            1
        );
    }
}
