//! Real file-backed SQLite, synthetic receipts only.
use lightning_goats::{
    domain::payment::SettledPayment,
    ledger::{LedgerStore, SettlementOutcome},
};
use serde_json::Value;
use sqlx::{
    Row, SqlitePool,
    sqlite::{SqliteConnectOptions, SqlitePoolOptions},
};
use std::{path::Path, str::FromStr, sync::Arc, time::Duration};
use tempfile::TempDir;
use tokio::sync::Barrier;

struct Db {
    _dir: TempDir,
    url: String,
    store: LedgerStore,
    raw: SqlitePool,
}
async fn raw_pool(url: &str) -> SqlitePool {
    SqlitePoolOptions::new()
        .max_connections(2)
        .connect_with(
            SqliteConnectOptions::from_str(url)
                .unwrap()
                .create_if_missing(true)
                .foreign_keys(true)
                .busy_timeout(Duration::from_secs(5)),
        )
        .await
        .unwrap()
}
async fn db() -> Db {
    let dir = TempDir::new().unwrap();
    let url = format!("sqlite://{}", dir.path().join("credit.db").display());
    let store = LedgerStore::connect(&url).await.unwrap();
    let raw = raw_pool(&url).await;
    Db {
        _dir: dir,
        url,
        store,
        raw,
    }
}
fn btc(id: &str, sats: u64) -> SettledPayment {
    SettledPayment {
        source: "strike".into(),
        source_id: id.into(),
        payment_hash: None,
        address_user: "herd".into(),
        credit_pool: "herd".into(),
        amount_msat: sats * 1000,
        settled_at: Some(123),
        context_json: None,
    }
}
async fn count(d: &Db, table: &str) -> i64 {
    sqlx::query_scalar(&format!("SELECT COUNT(*) FROM {table}"))
        .fetch_one(&d.raw)
        .await
        .unwrap()
}

#[tokio::test]
async fn native_btc_feeds_the_ledger_and_replays_after_reopen() {
    let d = db().await;
    let p = btc("native-one", 1200);
    let q = btc("native-two", 1140);
    d.store.record_payment(&p).await.unwrap();
    d.store.record_payment(&q).await.unwrap();
    assert_eq!(d.store.feed_credit_sats().await.unwrap(), 2340);
    for _ in 0..2 {
        let id = d.store.begin_feed_attempt(1000).await.unwrap().unwrap();
        d.store.confirm_feed_attempt(id).await.unwrap();
    }
    assert_eq!(d.store.feed_credit_sats().await.unwrap(), 340);
    let reopened = LedgerStore::connect(&d.url).await.unwrap();
    assert_eq!(
        reopened.record_payment(&p).await.unwrap(),
        SettlementOutcome::Duplicate
    );
    assert_eq!(
        reopened.record_payment(&q).await.unwrap(),
        SettlementOutcome::Duplicate
    );
    let events = reopened.events_after(0, 100).await.unwrap();
    assert_eq!(
        events
            .iter()
            .filter(|e| e.event_type == "payment_received")
            .count(),
        2
    );
    assert_eq!(
        events
            .iter()
            .filter(|e| e.event_type == "feeder_confirmed")
            .count(),
        2
    );
    assert_eq!(count(&d, "credit_grants").await, 2);
    let (_, snapshot) = reopened.overlay_snapshot_message(1000).await.unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&snapshot).unwrap()["feed_credit_sats"],
        340
    );
}

#[tokio::test]
async fn concurrent_native_replays_preserve_single_receipt_event_and_grant() {
    let d = db().await;
    let barrier = Arc::new(Barrier::new(8));
    let mut jobs = Vec::new();
    for _ in 0..8 {
        let store = LedgerStore::connect(&d.url).await.unwrap();
        let b = barrier.clone();
        jobs.push(tokio::spawn(async move {
            b.wait().await;
            store.record_payment(&btc("same-btc", 500)).await.unwrap()
        }));
    }
    let mut credited = 0;
    for j in jobs {
        if matches!(j.await.unwrap(), SettlementOutcome::Credited { .. }) {
            credited += 1;
        }
    }
    assert_eq!(credited, 1);
    assert_eq!(count(&d, "credit_grants").await, 1);
    assert_eq!(count(&d, "event_log").await, 1);
}

#[tokio::test]
async fn native_provenance_failure_rolls_back_the_original_payment_transaction() {
    let d = db().await;
    sqlx::query("CREATE TRIGGER injected BEFORE INSERT ON credit_grants BEGIN SELECT RAISE(ABORT,'injected'); END").execute(&d.raw).await.unwrap();
    let p = btc("native-fault", 750);
    assert!(d.store.record_payment(&p).await.is_err());
    for t in [
        "settled_payments",
        "ledger_entries",
        "event_log",
        "asset_receipts",
        "credit_allocations",
        "credit_valuations",
    ] {
        assert_eq!(count(&d, t).await, 0);
    }
    sqlx::query("DROP TRIGGER injected")
        .execute(&d.raw)
        .await
        .unwrap();
    d.store.record_payment(&p).await.unwrap();
    assert_eq!(d.store.feed_credit_sats().await.unwrap(), 750);
    assert_eq!(count(&d, "credit_grants").await, 1);
}

#[tokio::test]
async fn schema_protects_native_provenance_and_replay_history() {
    let d = db().await;
    d.store.record_payment(&btc("retained", 500)).await.unwrap();
    for sql in [
        "UPDATE credit_valuations SET target_sats=2000",
        "DELETE FROM credit_valuations",
        "UPDATE asset_receipts SET amount_atomic=1",
        "DELETE FROM asset_receipts",
        "UPDATE credit_grants SET delta_sats=1",
        "DELETE FROM credit_grants",
        "DELETE FROM credit_allocations",
        "UPDATE credit_allocations SET eligible_atomic=0",
    ] {
        assert!(sqlx::query(sql).execute(&d.raw).await.is_err(), "{sql}");
    }
}

async fn legacy_db() -> (TempDir, String, SqlitePool) {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("old-migrations");
    std::fs::create_dir(&path).unwrap();
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("migrations");
    for entry in std::fs::read_dir(source).unwrap() {
        let e = entry.unwrap();
        let name = e.file_name().to_string_lossy().into_owned();
        if name.ends_with(".sql") && name.as_str() < "0010" {
            std::fs::copy(e.path(), path.join(name)).unwrap();
        }
    }
    let url = format!("sqlite://{}", dir.path().join("old.db").display());
    let raw = raw_pool(&url).await;
    sqlx::migrate::Migrator::new(path)
        .await
        .unwrap()
        .run(&raw)
        .await
        .unwrap();
    (dir, url, raw)
}
async fn seed_legacy(raw: &SqlitePool, corrupt: bool) {
    sqlx::query("INSERT INTO settled_payments (id,source,source_id,address_user,credit_pool,amount_msat,settled_at,context_json) VALUES (7,'strike','historic','herd','herd',2340000,123,'{\"private\":true}')").execute(raw).await.unwrap();
    sqlx::query("INSERT INTO ledger_entries (id,entry_type,source_key,delta_sats,payment_source,payment_source_id,address_user,credit_pool) VALUES (19,'HERD_RECEIPT','payment:strike:historic',?,'strike','historic','herd','herd')").bind(if corrupt {2341}else{2340}).execute(raw).await.unwrap();
    sqlx::query("INSERT INTO feed_attempts (id,status,threshold_sats) VALUES ('11111111-1111-4111-8111-111111111111','confirmed',1000),('22222222-2222-4222-8222-222222222222','unknown',1000)").execute(raw).await.unwrap();
    sqlx::query("INSERT INTO ledger_entries (id,entry_type,source_key,delta_sats,feed_attempt_id) VALUES (20,'FEED_DEBIT','feed:11111111-1111-4111-8111-111111111111',-1000,'11111111-1111-4111-8111-111111111111')").execute(raw).await.unwrap();
    sqlx::query("INSERT INTO event_log (seq,event_type,payload_json) VALUES (41,'payment_received','{\"original\":\"bytes preserved\"}'),(42,'feeder_confirmed','{\"original\":\"feed\"}')").execute(raw).await.unwrap();
    sqlx::query("INSERT INTO message_outbox (event_id,signed_event_json,status,source_event_seq) VALUES ('synthetic-signed-id','{\"originalSignedBytes\":true}','failed',41)").execute(raw).await.unwrap();
    sqlx::query("UPDATE message_cursor SET last_event_seq=42")
        .execute(raw)
        .await
        .unwrap();
    sqlx::query("INSERT INTO strike_receive_requests (receive_request_id,address_user,credit_pool,description_hash,amount_msat,invoice,payment_hash) VALUES ('issued-unpaid','herd','herd',?,1000000,'synthetic-invoice',?)").bind("a".repeat(64)).bind("b".repeat(64)).execute(raw).await.unwrap();
}

#[tokio::test]
async fn populated_migration_preserves_credit_pending_ids_outbox_and_old_event_bytes() {
    let (_dir, url, raw) = legacy_db().await;
    seed_legacy(&raw, false).await;
    let before_events: Vec<(i64, String, String)> =
        sqlx::query_as("SELECT seq,event_type,payload_json FROM event_log ORDER BY seq")
            .fetch_all(&raw)
            .await
            .unwrap();
    let before_entries: Vec<(i64, String, String, i64)> = sqlx::query_as(
        "SELECT id,entry_type,source_key,delta_sats FROM ledger_entries ORDER BY id",
    )
    .fetch_all(&raw)
    .await
    .unwrap();
    let store = LedgerStore::connect(&url).await.unwrap();
    assert_eq!(store.feed_credit_sats().await.unwrap(), 1340);
    assert_eq!(
        store
            .unresolved_feed_attempt()
            .await
            .unwrap()
            .unwrap()
            .id
            .to_string(),
        "22222222-2222-4222-8222-222222222222"
    );
    assert_eq!(
        store.record_payment(&btc("historic", 2340)).await.unwrap(),
        SettlementOutcome::Duplicate
    );
    let after_events: Vec<(i64, String, String)> =
        sqlx::query_as("SELECT seq,event_type,payload_json FROM event_log ORDER BY seq")
            .fetch_all(&raw)
            .await
            .unwrap();
    let after_entries: Vec<(i64, String, String, i64)> = sqlx::query_as(
        "SELECT id,entry_type,source_key,delta_sats FROM ledger_entries ORDER BY id",
    )
    .fetch_all(&raw)
    .await
    .unwrap();
    assert_eq!(before_events, after_events);
    assert_eq!(before_entries, after_entries);
    let signed: String = sqlx::query_scalar("SELECT signed_event_json FROM message_outbox")
        .fetch_one(&raw)
        .await
        .unwrap();
    assert_eq!(signed, "{\"originalSignedBytes\":true}");
    let cursor: i64 = sqlx::query_scalar("SELECT last_event_seq FROM message_cursor")
        .fetch_one(&raw)
        .await
        .unwrap();
    assert_eq!(cursor, 42);
    let grant = sqlx::query("SELECT delta_sats,ledger_entry_id,event_seq FROM credit_grants")
        .fetch_one(&raw)
        .await
        .unwrap();
    assert_eq!(grant.get::<i64, _>("delta_sats"), 2340);
    assert_eq!(grant.get::<i64, _>("ledger_entry_id"), 19);
    assert!(grant.get::<Option<i64>, _>("event_seq").is_none());
    let re = LedgerStore::connect(&url).await.unwrap();
    re.record_payment(&btc("new", 100)).await.unwrap();
    assert_eq!(re.feed_credit_sats().await.unwrap(), 1440);
    let issued: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM strike_receive_requests WHERE receive_request_id='issued-unpaid'",
    )
    .fetch_one(&raw)
    .await
    .unwrap();
    assert_eq!(issued, 1);
}

#[tokio::test]
async fn corrupt_historical_credit_fails_migration_without_partial_backfill() {
    let (_dir, url, raw) = legacy_db().await;
    seed_legacy(&raw, true).await;
    assert!(LedgerStore::connect(&url).await.is_err());
    let count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM sqlite_master WHERE name='credit_valuations'")
            .fetch_one(&raw)
            .await
            .unwrap();
    assert_eq!(count, 0);
    let entries: i64 = sqlx::query_scalar("SELECT SUM(delta_sats) FROM ledger_entries")
        .fetch_one(&raw)
        .await
        .unwrap();
    assert_eq!(entries, 1341);
}

#[tokio::test]
async fn total_credit_overflow_is_not_promoted_to_float_or_partly_committed() {
    let d = db().await;
    sqlx::query("INSERT INTO ledger_entries(entry_type,source_key,delta_sats) VALUES ('TEST_EXISTING_CREDIT','test-max',?)").bind(i64::MAX).execute(&d.raw).await.unwrap();
    assert_eq!(d.store.feed_credit_sats().await.unwrap(), i64::MAX as u64);
    assert_eq!(count(&d, "credit_grants").await, 0);
    assert!(
        d.store
            .record_payment(&btc("native-overflow", 1))
            .await
            .is_err()
    );
    assert_eq!(count(&d, "settled_payments").await, 0);
    assert_eq!(count(&d, "event_log").await, 0);
}
