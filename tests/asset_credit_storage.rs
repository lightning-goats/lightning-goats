//! Real file-backed SQLite, synthetic receipts only. No network or actuator.
use lightning_goats::{
    domain::{
        credit::{Asset, AssetAmount, XmrBtcRate},
        payment::SettledPayment,
    },
    ledger::{
        CreditReceiptOutcome, LedgerStore, SettlementOutcome, XmrCreditIntent, XmrNetwork,
        XmrReceiptObservation,
    },
};
use serde_json::{Value, json};
use sqlx::{
    Row, SqlitePool,
    sqlite::{SqliteConnectOptions, SqlitePoolOptions},
};
use std::{path::Path, str::FromStr, sync::Arc, time::Duration};
use tempfile::TempDir;
use tokio::sync::Barrier;
use uuid::Uuid;

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
fn intent(target: u64, atomic: u64) -> XmrCreditIntent {
    // Synthetic rational: the resulting requested piconero is exactly `atomic`.
    let quote = XmrBtcRate::new(target * 1_000_000_000_000, atomic, "synthetic", 90)
        .unwrap()
        .quote(target, 100, 200, 20)
        .unwrap();
    assert_eq!(quote.terms().expected_atomic(), atomic);
    XmrCreditIntent {
        id: Uuid::new_v4(),
        provider: "moneropay".into(),
        network: XmrNetwork::Stagenet,
        account_scope: "project-wallet".into(),
        receive_scope: format!("private-subaddress-{}", Uuid::new_v4()),
        address_user: "nova".into(),
        credit_pool: "herd".into(),
        quote,
        max_credit_sats: target * 10,
    }
}
fn receipt(i: &XmrCreditIntent, key: &str, atomic: u64) -> XmrReceiptObservation {
    XmrReceiptObservation {
        provider: i.provider.clone(),
        network: i.network,
        account_scope: i.account_scope.clone(),
        receive_scope: i.receive_scope.clone(),
        receipt_key: key.into(),
        amount: AssetAmount::new(Asset::Xmr, atomic).unwrap(),
        first_seen_at: 150,
        unlocked: true,
        double_spend_seen: false,
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
async fn mixed_assets_feed_the_same_ledger_and_replay_after_reopen() {
    let d = db().await;
    let a = intent(425, 17);
    let b = intent(1440, 29);
    d.store.register_xmr_credit_intent(&a).await.unwrap();
    d.store.register_xmr_credit_intent(&b).await.unwrap();
    let p = btc("native-one", 300);
    let q = btc("native-two", 175);
    d.store.record_payment(&p).await.unwrap();
    assert_eq!(
        d.store
            .record_xmr_receipt(a.id, &receipt(&a, "private-tx-a", 17))
            .await
            .unwrap(),
        CreditReceiptOutcome::Recorded {
            delta_sats: 425,
            feed_credit_sats: 725
        }
    );
    d.store.record_payment(&q).await.unwrap();
    d.store
        .record_xmr_receipt(b.id, &receipt(&b, "private-tx-b", 29))
        .await
        .unwrap();
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
    for (i, k, n) in [(&a, "private-tx-a", 17), (&b, "private-tx-b", 29)] {
        assert_eq!(
            reopened
                .record_xmr_receipt(i.id, &receipt(i, k, n))
                .await
                .unwrap(),
            CreditReceiptOutcome::Duplicate
        );
    }
    let events = reopened.events_after(0, 100).await.unwrap();
    assert_eq!(
        events
            .iter()
            .filter(|e| e.event_type == "payment_received")
            .count(),
        4
    );
    assert_eq!(
        events
            .iter()
            .filter(|e| e.event_type == "feeder_confirmed")
            .count(),
        2
    );
    assert_eq!(count(&d, "credit_grants").await, 4);
    let (_, snapshot) = reopened.overlay_snapshot_message(1000).await.unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&snapshot).unwrap()["feed_credit_sats"],
        340
    );
}

#[tokio::test]
async fn dust_watermarks_survive_restart_and_fragmentation() {
    let d = db().await;
    let i = intent(1, 3);
    d.store.register_xmr_credit_intent(&i).await.unwrap();
    for index in 0..3 {
        let store = LedgerStore::connect(&d.url).await.unwrap();
        let o = receipt(&i, &format!("fragment{index}"), 1);
        assert_eq!(
            store.record_xmr_receipt(i.id, &o).await.unwrap(),
            CreditReceiptOutcome::Recorded {
                delta_sats: u64::from(index == 2),
                feed_credit_sats: u64::from(index == 2)
            }
        );
        assert_eq!(
            store.record_xmr_receipt(i.id, &o).await.unwrap(),
            CreditReceiptOutcome::Duplicate
        );
        assert_eq!(
            store
                .xmr_credit_allocation(i.id)
                .await
                .unwrap()
                .eligible_atomic,
            index + 1
        );
    }
    assert_eq!(count(&d, "asset_receipts").await, 3);
    assert_eq!(count(&d, "credit_grants").await, 3);
    assert_eq!(count(&d, "ledger_entries").await, 1);
    assert_eq!(count(&d, "event_log").await, 1);
}

#[tokio::test]
async fn pending_funds_do_not_credit_and_timely_observation_honors_later_unlock() {
    let d = db().await;
    let i = intent(1000, 100);
    d.store.register_xmr_credit_intent(&i).await.unwrap();
    let mut o = receipt(&i, "first-seen-before-expiry", 100);
    o.unlocked = false;
    assert_eq!(
        d.store.record_xmr_receipt(i.id, &o).await.unwrap(),
        CreditReceiptOutcome::Pending
    );
    assert_eq!(count(&d, "asset_receipts").await, 1);
    assert_eq!(d.store.feed_credit_sats().await.unwrap(), 0);
    // Quote timestamps deliberately precede test execution: unlock is not repriced.
    let store = LedgerStore::connect(&d.url).await.unwrap();
    o.unlocked = true;
    assert_eq!(
        store.record_xmr_receipt(i.id, &o).await.unwrap(),
        CreditReceiptOutcome::Recorded {
            delta_sats: 1000,
            feed_credit_sats: 1000
        }
    );
}

#[tokio::test]
async fn late_topups_are_retained_without_blocking_timely_pending_receipts() {
    let d = db().await;
    let i = intent(1000, 100);
    d.store.register_xmr_credit_intent(&i).await.unwrap();
    let mut timely = receipt(&i, "timely", 50);
    timely.unlocked = false;
    d.store.record_xmr_receipt(i.id, &timely).await.unwrap();
    for (key, time) in [("late", 200), ("before-quote", 99)] {
        let mut late = receipt(&i, key, 50);
        late.first_seen_at = time;
        assert_eq!(
            d.store.record_xmr_receipt(i.id, &late).await.unwrap(),
            CreditReceiptOutcome::Held
        );
    }
    timely.unlocked = true;
    d.store.record_xmr_receipt(i.id, &timely).await.unwrap();
    assert_eq!(d.store.feed_credit_sats().await.unwrap(), 500);
    assert_eq!(count(&d, "asset_receipts").await, 3);
    assert!(
        d.store
            .xmr_credit_allocation(i.id)
            .await
            .unwrap()
            .hold_reason
            .is_none()
    );
}

#[tokio::test]
async fn same_transaction_in_distinct_subaddresses_is_not_a_global_duplicate() {
    let d = db().await;
    let a = intent(500, 10);
    let b = intent(700, 20);
    for i in [&a, &b] {
        d.store.register_xmr_credit_intent(i).await.unwrap();
        d.store
            .record_xmr_receipt(
                i.id,
                &receipt(i, "same-transaction", i.quote.terms().expected_atomic()),
            )
            .await
            .unwrap();
    }
    assert_eq!(d.store.feed_credit_sats().await.unwrap(), 1200);
    let mut alias = a.clone();
    alias.id = Uuid::new_v4();
    assert!(d.store.register_xmr_credit_intent(&alias).await.is_err());
    assert_eq!(count(&d, "credit_valuations").await, 2);
}

#[tokio::test]
async fn immutable_quote_roundtrips_full_rate_evidence_and_rejects_rebinding() {
    let d = db().await;
    let i = intent(1000, 33);
    d.store.register_xmr_credit_intent(&i).await.unwrap();
    d.store.register_xmr_credit_intent(&i).await.unwrap();
    let got = LedgerStore::connect(&d.url)
        .await
        .unwrap()
        .xmr_credit_intent(i.id)
        .await
        .unwrap();
    assert_eq!(got.quote, i.quote);
    assert_eq!(got.receive_scope, i.receive_scope);
    assert_eq!(got.max_credit_sats, i.max_credit_sats);
    for field in 0..7 {
        let mut bad = i.clone();
        match field {
            0 => bad.receive_scope = "another-address".into(),
            1 => bad.account_scope = "another-wallet".into(),
            2 => bad.network = XmrNetwork::Mainnet,
            3 => bad.address_user = "dexter".into(),
            4 => bad.max_credit_sats += 1,
            5 => bad.quote = intent(1000, 34).quote,
            _ => bad.provider = "different-provider".into(),
        }
        assert!(d.store.register_xmr_credit_intent(&bad).await.is_err());
    }
    assert_eq!(count(&d, "credit_allocations").await, 1);
}

#[tokio::test]
async fn receipt_binding_mismatch_is_rejected_before_any_observation_write() {
    let d = db().await;
    let i = intent(100, 10);
    d.store.register_xmr_credit_intent(&i).await.unwrap();
    for field in 0..4 {
        let mut o = receipt(&i, "wrong-binding", 10);
        match field {
            0 => o.receive_scope = "other".into(),
            1 => o.account_scope = "other".into(),
            2 => o.provider = "other".into(),
            _ => o.network = XmrNetwork::Mainnet,
        }
        assert!(d.store.record_xmr_receipt(i.id, &o).await.is_err());
    }
    assert_eq!(count(&d, "asset_receipts").await, 0);
    assert_eq!(d.store.feed_credit_sats().await.unwrap(), 0);
}

#[tokio::test]
async fn changed_receipts_regression_and_double_spend_hold_persistently() {
    for defect in 0..4 {
        let d = db().await;
        let i = intent(1000, 100);
        d.store.register_xmr_credit_intent(&i).await.unwrap();
        let o = receipt(&i, "private-id", 50);
        d.store.record_xmr_receipt(i.id, &o).await.unwrap();
        let mut bad = o.clone();
        match defect {
            0 => bad.amount = AssetAmount::new(Asset::Xmr, 49).unwrap(),
            1 => bad.first_seen_at += 1,
            2 => bad.unlocked = false,
            _ => bad.double_spend_seen = true,
        }
        for _ in 0..2 {
            assert_eq!(
                d.store.record_xmr_receipt(i.id, &bad).await.unwrap(),
                CreditReceiptOutcome::Held
            );
        }
        let reopened = LedgerStore::connect(&d.url).await.unwrap();
        assert_eq!(
            reopened.record_xmr_receipt(i.id, &o).await.unwrap(),
            CreditReceiptOutcome::Held
        );
        assert_eq!(
            reopened
                .record_xmr_receipt(i.id, &receipt(&i, "new-receipt", 50))
                .await
                .unwrap(),
            CreditReceiptOutcome::Held
        );
        assert_eq!(reopened.feed_credit_sats().await.unwrap(), 500);
        assert!(
            reopened
                .xmr_credit_allocation(i.id)
                .await
                .unwrap()
                .hold_reason
                .is_some()
        );
        assert_eq!(count(&d, "credit_conflicts").await, 1);
        assert_eq!(count(&d, "credit_grants").await, 1);
    }
}

#[tokio::test]
async fn first_observation_double_spend_cannot_grant() {
    let d = db().await;
    let i = intent(1000, 100);
    d.store.register_xmr_credit_intent(&i).await.unwrap();
    let mut o = receipt(&i, "bad-first", 100);
    o.double_spend_seen = true;
    assert_eq!(
        d.store.record_xmr_receipt(i.id, &o).await.unwrap(),
        CreditReceiptOutcome::Held
    );
    assert_eq!(count(&d, "asset_receipts").await, 1);
    assert_eq!(count(&d, "credit_grants").await, 0);
    assert!(
        d.store
            .xmr_credit_allocation(i.id)
            .await
            .unwrap()
            .hold_reason
            .is_some()
    );
}

#[tokio::test]
async fn provider_history_hold_is_persistent_but_does_not_block_native_btc() {
    let d = db().await;
    let i = intent(1000, 100);
    d.store.register_xmr_credit_intent(&i).await.unwrap();
    d.store.hold_xmr_credit_intent(i.id).await.unwrap();
    let reopened = LedgerStore::connect(&d.url).await.unwrap();
    assert_eq!(
        reopened
            .record_xmr_receipt(i.id, &receipt(&i, "recovered", 100))
            .await
            .unwrap(),
        CreditReceiptOutcome::Held
    );
    reopened
        .record_payment(&btc("still-working", 300))
        .await
        .unwrap();
    assert_eq!(reopened.feed_credit_sats().await.unwrap(), 300);
}

#[tokio::test]
async fn concurrent_duplicate_and_distinct_receipts_use_one_serialized_watermark() {
    let d = db().await;
    let i = intent(1000, 10);
    d.store.register_xmr_credit_intent(&i).await.unwrap();
    let barrier = Arc::new(Barrier::new(12));
    let mut jobs = Vec::new();
    for n in 0..12 {
        let store = LedgerStore::connect(&d.url).await.unwrap();
        let i = i.clone();
        let barrier = barrier.clone();
        jobs.push(tokio::spawn(async move {
            let o = receipt(&i, &format!("fragment-{}", n % 6), 1);
            barrier.wait().await;
            store.record_xmr_receipt(i.id, &o).await.unwrap()
        }));
    }
    let mut recorded = 0;
    for job in jobs {
        if matches!(job.await.unwrap(), CreditReceiptOutcome::Recorded { .. }) {
            recorded += 1;
        }
    }
    assert_eq!(recorded, 6);
    assert_eq!(d.store.feed_credit_sats().await.unwrap(), 600);
    assert_eq!(
        d.store
            .xmr_credit_allocation(i.id)
            .await
            .unwrap()
            .eligible_atomic,
        6
    );
    assert_eq!(count(&d, "credit_grants").await, 6);
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
async fn injected_failures_roll_back_receipt_allocation_ledger_and_event() {
    for table in [
        "ledger_entries",
        "event_log",
        "credit_grants",
        "credit_allocations",
    ] {
        let d = db().await;
        let i = intent(1000, 10);
        d.store.register_xmr_credit_intent(&i).await.unwrap();
        let op = if table == "credit_allocations" {
            "UPDATE"
        } else {
            "INSERT"
        };
        sqlx::query(&format!("CREATE TRIGGER injected BEFORE {op} ON {table} BEGIN SELECT RAISE(ABORT,'injected'); END")).execute(&d.raw).await.unwrap();
        let o = receipt(&i, "fault-retry", 10);
        assert!(d.store.record_xmr_receipt(i.id, &o).await.is_err());
        for t in [
            "asset_receipts",
            "credit_grants",
            "ledger_entries",
            "event_log",
        ] {
            assert_eq!(count(&d, t).await, 0, "{table} -> {t}");
        }
        assert_eq!(
            d.store
                .xmr_credit_allocation(i.id)
                .await
                .unwrap()
                .eligible_atomic,
            0
        );
        sqlx::query("DROP TRIGGER injected")
            .execute(&d.raw)
            .await
            .unwrap();
        assert_eq!(
            d.store.record_xmr_receipt(i.id, &o).await.unwrap(),
            CreditReceiptOutcome::Recorded {
                delta_sats: 1000,
                feed_credit_sats: 1000
            }
        );
    }
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
async fn public_credit_events_are_allowlisted_and_never_contain_private_receipt_data() {
    let d = db().await;
    let i = intent(425, 17);
    d.store.register_xmr_credit_intent(&i).await.unwrap();
    d.store
        .record_xmr_receipt(i.id, &receipt(&i, "private-tx-output-key", 17))
        .await
        .unwrap();
    let events = d.store.events_after(0, 10).await.unwrap();
    assert_eq!(events.len(), 1);
    let v: Value = serde_json::from_str(&events[0].payload_json).unwrap();
    assert_eq!(
        v,
        json!({"amount_sats":425,"feed_credit_sats":425,"address_user":"nova","credit_pool":"herd"})
    );
    for secret in [
        &i.receive_scope,
        &i.account_scope,
        &i.id.to_string(),
        "private-tx-output-key",
        "XMR",
        "moneropay",
    ] {
        assert!(!events[0].payload_json.contains(secret));
    }
}

#[tokio::test]
async fn overpayment_limit_preserves_funds_and_holds_without_clipping_credit() {
    let d = db().await;
    let mut i = intent(1000, 100);
    i.max_credit_sats = 1500;
    d.store.register_xmr_credit_intent(&i).await.unwrap();
    d.store
        .record_xmr_receipt(i.id, &receipt(&i, "partial", 50))
        .await
        .unwrap();
    assert_eq!(
        d.store
            .record_xmr_receipt(i.id, &receipt(&i, "too-much", 200))
            .await
            .unwrap(),
        CreditReceiptOutcome::Held
    );
    assert_eq!(d.store.feed_credit_sats().await.unwrap(), 500);
    assert_eq!(count(&d, "asset_receipts").await, 2);
    assert_eq!(
        d.store
            .xmr_credit_allocation(i.id)
            .await
            .unwrap()
            .credited_sats,
        500
    );
}

#[tokio::test]
async fn corrupt_allocation_is_not_silently_repaired_or_credited_again() {
    let d = db().await;
    let i = intent(1000, 100);
    d.store.register_xmr_credit_intent(&i).await.unwrap();
    d.store
        .record_xmr_receipt(i.id, &receipt(&i, "first", 50))
        .await
        .unwrap();
    // Deliberate on-disk corruption simulation; ordinary APIs cannot do this.
    sqlx::query("UPDATE credit_allocations SET credited_sats=501 WHERE valuation_id=?")
        .bind(format!("xmr:{}", i.id))
        .execute(&d.raw)
        .await
        .unwrap();
    assert_eq!(
        d.store
            .record_xmr_receipt(i.id, &receipt(&i, "second", 50))
            .await
            .unwrap(),
        CreditReceiptOutcome::Held
    );
    assert_eq!(d.store.feed_credit_sats().await.unwrap(), 500);
    assert_eq!(count(&d, "credit_grants").await, 1);
}

#[tokio::test]
async fn schema_protects_quotes_grants_and_replay_history() {
    let d = db().await;
    let i = intent(1000, 100);
    d.store.register_xmr_credit_intent(&i).await.unwrap();
    d.store
        .record_xmr_receipt(i.id, &receipt(&i, "retained", 50))
        .await
        .unwrap();
    d.store.hold_xmr_credit_intent(i.id).await.unwrap();
    for sql in [
        "UPDATE credit_valuations SET target_sats=2000",
        "DELETE FROM credit_valuations",
        "UPDATE asset_receipts SET amount_atomic=1",
        "DELETE FROM asset_receipts",
        "UPDATE credit_grants SET delta_sats=1",
        "DELETE FROM credit_grants",
        "DELETE FROM credit_allocations",
        "UPDATE credit_allocations SET eligible_atomic=0",
        "UPDATE credit_allocations SET hold_reason=NULL",
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
async fn invalid_or_unknown_receipts_cannot_create_accounting_state() {
    let d = db().await;
    let i = intent(100, 10);
    let mut o = receipt(&i, "invalid", 10);
    assert!(d.store.record_xmr_receipt(i.id, &o).await.is_err());
    d.store.register_xmr_credit_intent(&i).await.unwrap();
    for amount in [
        AssetAmount::new(Asset::Btc, 10).unwrap(),
        AssetAmount::new(Asset::Xmr, 0).unwrap(),
    ] {
        o.amount = amount;
        assert!(d.store.record_xmr_receipt(i.id, &o).await.is_err());
    }
    assert_eq!(count(&d, "asset_receipts").await, 0);
    assert_eq!(count(&d, "event_log").await, 0);
}

#[tokio::test]
async fn native_and_xmr_writers_serialize_absolute_event_balances() {
    let d = db().await;
    let i = intent(100, 10);
    d.store.register_xmr_credit_intent(&i).await.unwrap();
    let barrier = Arc::new(Barrier::new(10));
    let mut jobs = Vec::new();
    for n in 0..10 {
        let store = LedgerStore::connect(&d.url).await.unwrap();
        let i = i.clone();
        let b = barrier.clone();
        jobs.push(tokio::spawn(async move {
            b.wait().await;
            if n % 2 == 0 {
                store
                    .record_payment(&btc(&format!("mixed-{n}"), 100))
                    .await
                    .unwrap();
            } else {
                store
                    .record_xmr_receipt(i.id, &receipt(&i, &format!("mixed-{n}"), 10))
                    .await
                    .unwrap();
            }
        }));
    }
    for j in jobs {
        j.await.unwrap();
    }
    assert_eq!(d.store.feed_credit_sats().await.unwrap(), 1000);
    for (n, e) in d
        .store
        .events_after(0, 100)
        .await
        .unwrap()
        .iter()
        .enumerate()
    {
        let v: Value = serde_json::from_str(&e.payload_json).unwrap();
        assert_eq!(v["feed_credit_sats"], ((n + 1) * 100) as u64);
    }
}

#[tokio::test]
async fn rate_rationals_beyond_sqlite_signed_range_roundtrip_as_decimal_text() {
    let d = db().await;
    let mut i = intent(1, 1);
    i.quote = XmrBtcRate::new(3_000_000_000_000, u64::MAX, "synthetic-large", 90)
        .unwrap()
        .quote(1, 100, 200, 20)
        .unwrap();
    d.store.register_xmr_credit_intent(&i).await.unwrap();
    let got = d.store.xmr_credit_intent(i.id).await.unwrap();
    assert_eq!(got.quote, i.quote);
    let kind: String = sqlx::query_scalar("SELECT typeof(rate_denominator) FROM credit_valuations")
        .fetch_one(&d.raw)
        .await
        .unwrap();
    assert_eq!(kind, "text");
}

#[tokio::test]
async fn cumulative_atomic_overflow_holds_without_changing_granted_credit() {
    let d = db().await;
    let i = intent(1, i64::MAX as u64);
    d.store.register_xmr_credit_intent(&i).await.unwrap();
    d.store
        .record_xmr_receipt(i.id, &receipt(&i, "max", i64::MAX as u64))
        .await
        .unwrap();
    assert_eq!(
        d.store
            .record_xmr_receipt(i.id, &receipt(&i, "one-more", 1))
            .await
            .unwrap(),
        CreditReceiptOutcome::Held
    );
    assert_eq!(d.store.feed_credit_sats().await.unwrap(), 1);
    assert_eq!(count(&d, "asset_receipts").await, 2);
    assert_eq!(
        d.store
            .xmr_credit_allocation(i.id)
            .await
            .unwrap()
            .eligible_atomic,
        i64::MAX as u64
    );
}

#[tokio::test]
async fn total_credit_overflow_is_not_promoted_to_float_or_partly_committed() {
    let d = db().await;
    let i = intent(1, 1);
    d.store.register_xmr_credit_intent(&i).await.unwrap();
    sqlx::query("INSERT INTO ledger_entries(entry_type,source_key,delta_sats) VALUES ('TEST_EXISTING_CREDIT','test-max',?)").bind(i64::MAX).execute(&d.raw).await.unwrap();
    assert_eq!(
        d.store
            .record_xmr_receipt(i.id, &receipt(&i, "one-more", 1))
            .await
            .unwrap(),
        CreditReceiptOutcome::Held
    );
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

#[tokio::test]
async fn new_quotes_or_valuation_queries_never_revalue_existing_credit() {
    let d = db().await;
    let first = intent(1000, 100);
    d.store.register_xmr_credit_intent(&first).await.unwrap();
    d.store
        .record_xmr_receipt(first.id, &receipt(&first, "first-payment", 50))
        .await
        .unwrap();
    let later = intent(1000, 200);
    d.store.register_xmr_credit_intent(&later).await.unwrap();
    let restored = d.store.xmr_credit_intent(first.id).await.unwrap();
    assert_eq!(restored.quote, first.quote);
    assert_eq!(d.store.feed_credit_sats().await.unwrap(), 500);
    assert_eq!(count(&d, "event_log").await, 1);
}
