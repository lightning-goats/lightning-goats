//! File-backed, synthetic quote lifecycle tests. No market, wallet or physical I/O.
use anyhow::{Result, bail};
use futures_util::future::BoxFuture;
use lightning_goats::{
    domain::{
        credit::{Asset, AssetAmount, XmrBtcRate},
        payment::SettledPayment,
    },
    ledger::{CreditReceiptOutcome, LedgerStore, XmrNetwork, XmrReceiptObservation},
    quotes::{
        QuoteClock, QuoteContext, QuotePolicy, QuoteRequest, QuoteService, RateEvidence,
        RateProvider, format_xmr,
    },
};
use sqlx::{Row, SqlitePool};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicI64, AtomicUsize, Ordering},
};
use tokio::sync::Notify;
use uuid::Uuid;

struct Clock(AtomicI64);
impl QuoteClock for Clock {
    fn now(&self) -> Result<i64> {
        Ok(self.0.load(Ordering::SeqCst))
    }
}
struct Oracle {
    clock: Arc<Clock>,
    rate: Mutex<(u64, u64, i64)>,
    calls: AtomicUsize,
    fail: bool,
    gate: Option<Arc<Notify>>,
}
impl RateProvider for Oracle {
    fn source_id(&self) -> &str {
        "fixture-xmrbtc-v1"
    }
    fn fetch(&self) -> BoxFuture<'_, Result<RateEvidence>> {
        Box::pin(async move {
            self.calls.fetch_add(1, Ordering::SeqCst);
            if let Some(gate) = &self.gate {
                gate.notified().await;
            }
            if self.fail {
                bail!("synthetic oracle outage");
            }
            let (n, d, time) = *self.rate.lock().unwrap();
            RateEvidence::new(
                XmrBtcRate::new(n, d, self.source_id(), time)?,
                self.clock.now()?,
                "{\"fixture\":\"private-rate-evidence\"}".into(),
            )
        })
    }
}
fn policy() -> QuotePolicy {
    QuotePolicy {
        min_target_sats: 1,
        max_target_sats: 10_000,
        max_credit_sats: 20_000,
        max_piconero: 1_000_000_000_000,
        lifetime_seconds: 120,
        max_rate_age_seconds: 300,
        min_sats_per_xmr: 100,
        max_sats_per_xmr: 1_000_000,
        admission_window_seconds: 60,
        max_requests_per_window: 20,
        max_requests_per_client_window: 5,
        max_pending_requests: 4,
        max_retained_requests: 100,
    }
}
fn context() -> QuoteContext {
    QuoteContext {
        provider: "moneropay".into(),
        account_scope: "test-private-account".into(),
        network: XmrNetwork::Stagenet,
    }
}
fn request() -> QuoteRequest {
    QuoteRequest {
        id: Uuid::new_v4(),
        client_bucket: "a".repeat(64),
        address_user: "nova".into(),
        target_sats: 1000,
    }
}
struct Env {
    _dir: tempfile::TempDir,
    url: String,
    store: LedgerStore,
    pool: SqlitePool,
    clock: Arc<Clock>,
    oracle: Arc<Oracle>,
}
impl Env {
    async fn new() -> Self {
        let dir = tempfile::TempDir::new().unwrap();
        let url = format!("sqlite://{}", dir.path().join("quotes.db").display());
        let store = LedgerStore::connect(&url).await.unwrap();
        let pool = SqlitePool::connect(&url).await.unwrap();
        let clock = Arc::new(Clock(AtomicI64::new(1000)));
        let oracle = Arc::new(Oracle {
            clock: clock.clone(),
            rate: Mutex::new((250_000, 1, 900)),
            calls: AtomicUsize::new(0),
            fail: false,
            gate: None,
        });
        Self {
            _dir: dir,
            url,
            store,
            pool,
            clock,
            oracle,
        }
    }
    fn service(&self, p: QuotePolicy) -> QuoteService {
        QuoteService::new(
            self.store.clone(),
            context(),
            p,
            self.oracle.clone(),
            self.clock.clone(),
        )
        .unwrap()
    }
    async fn count(&self, table: &str) -> i64 {
        sqlx::query_scalar(&format!("SELECT COUNT(*) FROM {table}"))
            .fetch_one(&self.pool)
            .await
            .unwrap()
    }
    async fn wait_calls(&self, n: usize) {
        tokio::time::timeout(std::time::Duration::from_secs(2), async {
            while self.oracle.calls.load(Ordering::SeqCst) < n {
                tokio::task::yield_now().await
            }
        })
        .await
        .unwrap();
    }
}
#[tokio::test]
async fn quote_creation_is_not_credit_and_terms_replay_byte_identically_after_restart() {
    let e = Env::new().await;
    let r = request();
    let q = e.service(policy()).create(&r).await.unwrap();
    let bytes = q.terms_json().unwrap();
    assert_eq!(q.quote().terms().expected_atomic(), 4_000_000_000);
    assert_eq!(e.store.feed_credit_sats().await.unwrap(), 0);
    assert_eq!(e.count("credit_valuations").await, 0);
    e.clock.0.store(1100, Ordering::SeqCst);
    *e.oracle.rate.lock().unwrap() = (500_000, 1, 1050);
    let reopened = LedgerStore::connect(&e.url).await.unwrap();
    let svc = QuoteService::new(
        reopened,
        context(),
        policy(),
        e.oracle.clone(),
        e.clock.clone(),
    )
    .unwrap();
    assert_eq!(svc.create(&r).await.unwrap().terms_json().unwrap(), bytes);
    assert_eq!(svc.lookup(&r).await.unwrap().terms_json().unwrap(), bytes);
    assert_eq!(e.oracle.calls.load(Ordering::SeqCst), 1);
}
#[tokio::test]
async fn changed_request_identity_cannot_reprice_or_read_another_clients_quote() {
    let e = Env::new().await;
    let r = request();
    let svc = e.service(policy());
    svc.create(&r).await.unwrap();
    for field in 0..3 {
        let mut other = r.clone();
        match field {
            0 => other.target_sats += 1,
            1 => other.client_bucket = "b".repeat(64),
            _ => other.address_user = "herd".into(),
        };
        assert!(svc.create(&other).await.is_err());
        assert!(svc.lookup(&other).await.is_err());
    }
    let mut ctx = context();
    ctx.network = XmrNetwork::Mainnet;
    let wrong = QuoteService::new(
        e.store.clone(),
        ctx,
        policy(),
        e.oracle.clone(),
        e.clock.clone(),
    )
    .unwrap();
    assert!(wrong.lookup(&r).await.is_err());
    assert_eq!(e.oracle.calls.load(Ordering::SeqCst), 1);
}
#[tokio::test]
async fn invalid_targets_users_and_client_buckets_never_contact_oracle() {
    let e = Env::new().await;
    let svc = e.service(policy());
    for i in 0..5 {
        let mut r = request();
        match i {
            0 => r.target_sats = 0,
            1 => r.target_sats = 10001,
            2 => r.address_user = "attacker".into(),
            3 => r.client_bucket = "untrusted".into(),
            _ => r.id = Uuid::nil(),
        };
        assert!(svc.create(&r).await.is_err());
    }
    assert_eq!(e.count("xmr_quote_requests").await, 0);
    assert_eq!(e.oracle.calls.load(Ordering::SeqCst), 0);
}
#[tokio::test]
async fn expired_quote_is_still_retrievable_but_cannot_receive_new_binding() {
    let e = Env::new().await;
    let svc = e.service(policy());
    let r = request();
    let first = svc.create(&r).await.unwrap();
    e.clock.0.store(1120, Ordering::SeqCst);
    let retry = svc.create(&r).await.unwrap();
    assert!(retry.expired_at(1120));
    assert_eq!(first.terms_json().unwrap(), retry.terms_json().unwrap());
    assert!(svc.bind(&r, "test-private-subaddress").await.is_err());
    assert_eq!(e.count("credit_valuations").await, 0);
}
#[tokio::test]
async fn binding_is_atomic_immutable_and_replayable_after_expiry() {
    let e = Env::new().await;
    let svc = e.service(policy());
    let r = request();
    svc.create(&r).await.unwrap();
    let intent = svc.bind(&r, "subaddress-A").await.unwrap();
    assert_eq!(intent.quote.terms().expected_atomic(), 4_000_000_000);
    e.clock.0.store(1200, Ordering::SeqCst);
    svc.bind(&r, "subaddress-A").await.unwrap();
    assert!(svc.bind(&r, "subaddress-B").await.is_err());
    assert_eq!(e.count("credit_valuations").await, 1);
    assert_eq!(e.count("xmr_quote_bindings").await, 1);
}
#[tokio::test]
async fn binding_failure_rolls_back_credit_intent_registration() {
    let e = Env::new().await;
    let svc = e.service(policy());
    let r = request();
    svc.create(&r).await.unwrap();
    sqlx::query("CREATE TRIGGER fail_bind BEFORE INSERT ON xmr_quote_bindings BEGIN SELECT RAISE(ABORT,'injected'); END").execute(&e.pool).await.unwrap();
    assert!(svc.bind(&r, "subaddress-A").await.is_err());
    assert_eq!(e.count("credit_valuations").await, 0);
    assert_eq!(e.count("xmr_quote_bindings").await, 0);
    sqlx::query("DROP TRIGGER fail_bind")
        .execute(&e.pool)
        .await
        .unwrap();
    svc.bind(&r, "subaddress-A").await.unwrap();
}
#[tokio::test]
async fn same_receive_scope_cannot_be_rebound_to_a_new_quote() {
    let e = Env::new().await;
    let svc = e.service(policy());
    let a = request();
    let b = request();
    svc.create(&a).await.unwrap();
    svc.create(&b).await.unwrap();
    svc.bind(&a, "same-subaddress").await.unwrap();
    assert!(svc.bind(&b, "same-subaddress").await.is_err());
    assert_eq!(e.count("xmr_quote_bindings").await, 1);
}
#[tokio::test]
async fn timely_partials_unlock_after_expiry_and_late_topup_has_no_credit() {
    let e = Env::new().await;
    let svc = e.service(policy());
    let r = request();
    svc.create(&r).await.unwrap();
    let intent = svc.bind(&r, "private-subaddress").await.unwrap();
    let mut receipt = XmrReceiptObservation {
        provider: intent.provider,
        network: intent.network,
        account_scope: intent.account_scope,
        receive_scope: intent.receive_scope,
        receipt_key: "private-tx-1".into(),
        amount: AssetAmount::new(Asset::Xmr, 2_000_000_000).unwrap(),
        first_seen_at: 1100,
        unlocked: false,
        double_spend_seen: false,
    };
    assert_eq!(
        e.store.record_xmr_receipt(r.id, &receipt).await.unwrap(),
        CreditReceiptOutcome::Pending
    );
    e.clock.0.store(1300, Ordering::SeqCst);
    receipt.unlocked = true;
    e.store.record_xmr_receipt(r.id, &receipt).await.unwrap();
    assert_eq!(e.store.feed_credit_sats().await.unwrap(), 500);
    receipt.receipt_key = "late-tx".into();
    receipt.first_seen_at = 1200;
    assert_eq!(
        e.store.record_xmr_receipt(r.id, &receipt).await.unwrap(),
        CreditReceiptOutcome::Held
    );
    assert_eq!(e.store.feed_credit_sats().await.unwrap(), 500);
}
#[tokio::test]
async fn failed_rate_request_is_retained_and_native_btc_remains_independent() {
    let mut e = Env::new().await;
    Arc::get_mut(&mut e.oracle).unwrap().fail = true;
    let svc = e.service(policy());
    let r = request();
    assert!(svc.create(&r).await.is_err());
    assert!(svc.create(&r).await.is_err());
    assert_eq!(e.oracle.calls.load(Ordering::SeqCst), 1);
    e.store
        .record_payment(&SettledPayment {
            source: "strike".into(),
            source_id: "native-during-outage".into(),
            payment_hash: None,
            address_user: "herd".into(),
            credit_pool: "herd".into(),
            amount_msat: 300_000,
            settled_at: None,
            context_json: None,
        })
        .await
        .unwrap();
    assert_eq!(e.store.feed_credit_sats().await.unwrap(), 300);
}
#[tokio::test]
async fn stale_future_and_out_of_policy_prices_never_issue_a_quote() {
    for rate in [
        (250_000, 1, 600),
        (250_000, 1, 1001),
        (1, 1, 900),
        (2_000_000, 1, 900),
    ] {
        let e = Env::new().await;
        *e.oracle.rate.lock().unwrap() = rate;
        assert!(e.service(policy()).create(&request()).await.is_err());
        assert_eq!(e.count("xmr_rate_watermarks").await, 0);
    }
}
#[tokio::test]
async fn rate_age_boundary_is_checked_without_flooring_the_rational() {
    let e = Env::new().await;
    *e.oracle.rate.lock().unwrap() = (250_000, 1, 700);
    e.service(policy()).create(&request()).await.unwrap();
    let mut p = policy();
    p.min_sats_per_xmr = 250_000;
    *e.oracle.rate.lock().unwrap() = (499_999, 2, 901);
    assert!(e.service(p).create(&request()).await.is_err());
}
#[tokio::test]
async fn quoted_atomic_limit_is_enforced_before_persistence() {
    let e = Env::new().await;
    let mut p = policy();
    p.max_piconero = 3_999_999_999;
    assert!(e.service(p).create(&request()).await.is_err());
    assert_eq!(e.count("credit_valuations").await, 0);
}
#[tokio::test]
async fn per_client_and_global_admission_survive_reopen() {
    let e = Env::new().await;
    let mut p = policy();
    p.max_requests_per_client_window = 1;
    p.max_requests_per_window = 2;
    p.max_pending_requests = 2;
    e.service(p.clone()).create(&request()).await.unwrap();
    assert!(e.service(p.clone()).create(&request()).await.is_err());
    let mut second = request();
    second.client_bucket = "b".repeat(64);
    e.service(p.clone()).create(&second).await.unwrap();
    let mut third = request();
    third.client_bucket = "c".repeat(64);
    assert!(e.service(p.clone()).create(&third).await.is_err());
    e.clock.0.store(1060, Ordering::SeqCst);
    let store = LedgerStore::connect(&e.url).await.unwrap();
    let svc = QuoteService::new(store, context(), p, e.oracle.clone(), e.clock.clone()).unwrap();
    svc.create(&third).await.unwrap();
    assert_eq!(e.oracle.calls.load(Ordering::SeqCst), 3);
}
#[tokio::test]
async fn retained_request_capacity_never_deletes_old_quote_history() {
    let e = Env::new().await;
    let mut p = policy();
    p.max_retained_requests = 1;
    p.max_requests_per_window = 1;
    p.max_requests_per_client_window = 1;
    p.max_pending_requests = 1;
    let r = request();
    let svc = e.service(p);
    let original = svc.create(&r).await.unwrap().terms_json().unwrap();
    e.clock.0.store(1100, Ordering::SeqCst);
    assert!(svc.create(&request()).await.is_err());
    assert_eq!(
        svc.create(&r).await.unwrap().terms_json().unwrap(),
        original
    );
    assert_eq!(e.count("xmr_quote_requests").await, 1);
}
#[tokio::test]
async fn concurrent_same_id_reserves_one_oracle_call() {
    let mut e = Env::new().await;
    let gate = Arc::new(Notify::new());
    Arc::get_mut(&mut e.oracle).unwrap().gate = Some(gate.clone());
    let svc = e.service(policy());
    let r = request();
    let worker = svc.clone();
    let req = r.clone();
    let first = tokio::spawn(async move { worker.create(&req).await });
    e.wait_calls(1).await;
    for _ in 0..4 {
        assert!(e.service(policy()).create(&r).await.is_err());
    }
    gate.notify_one();
    first.await.unwrap().unwrap();
    assert_eq!(
        svc.create(&r).await.unwrap().quote().terms().target_sats(),
        1000
    );
    assert_eq!(e.oracle.calls.load(Ordering::SeqCst), 1);
}
#[tokio::test]
async fn shared_pending_budget_caps_separate_service_instances() {
    let mut e = Env::new().await;
    let gate = Arc::new(Notify::new());
    Arc::get_mut(&mut e.oracle).unwrap().gate = Some(gate.clone());
    let mut p = policy();
    p.max_pending_requests = 1;
    let svc = e.service(p.clone());
    let r = request();
    let worker = tokio::spawn(async move { svc.create(&r).await });
    e.wait_calls(1).await;
    assert!(e.service(p).create(&request()).await.is_err());
    gate.notify_one();
    worker.await.unwrap().unwrap();
    assert_eq!(e.oracle.calls.load(Ordering::SeqCst), 1);
}
#[tokio::test]
async fn canceled_request_releases_capacity_after_deadline_but_never_reuses_id() {
    let mut e = Env::new().await;
    let gate = Arc::new(Notify::new());
    Arc::get_mut(&mut e.oracle).unwrap().gate = Some(gate.clone());
    let mut p = policy();
    p.max_pending_requests = 1;
    let svc = e.service(p.clone());
    let r = request();
    let old = r.clone();
    let worker = tokio::spawn(async move { svc.create(&old).await });
    e.wait_calls(1).await;
    worker.abort();
    let _ = worker.await;
    e.clock.0.store(1031, Ordering::SeqCst);
    assert!(e.service(p.clone()).create(&r).await.is_err());
    let svc = e.service(p);
    let newer = tokio::spawn(async move { svc.create(&request()).await });
    e.wait_calls(2).await;
    gate.notify_one();
    newer.await.unwrap().unwrap();
    assert_eq!(e.count("xmr_quote_requests").await, 2);
}
#[tokio::test]
async fn delayed_oracle_cannot_commit_after_reservation_deadline() {
    let mut e = Env::new().await;
    let gate = Arc::new(Notify::new());
    Arc::get_mut(&mut e.oracle).unwrap().gate = Some(gate.clone());
    let svc = e.service(policy());
    let r = request();
    let worker = tokio::spawn(async move { svc.create(&r).await });
    e.wait_calls(1).await;
    e.clock.0.store(1030, Ordering::SeqCst);
    gate.notify_one();
    assert!(worker.await.unwrap().is_err());
    assert_eq!(e.count("xmr_rate_watermarks").await, 0);
}
#[tokio::test]
async fn conflicting_or_older_rate_evidence_is_rejected_after_restart() {
    let e = Env::new().await;
    e.service(policy()).create(&request()).await.unwrap();
    for value in [(250_001, 1, 900), (250_000, 1, 899)] {
        *e.oracle.rate.lock().unwrap() = value;
        assert!(e.service(policy()).create(&request()).await.is_err());
    }
    *e.oracle.rate.lock().unwrap() = (500_000, 2, 900);
    e.service(policy()).create(&request()).await.unwrap();
    *e.oracle.rate.lock().unwrap() = (250_001, 1, 960);
    e.service(policy()).create(&request()).await.unwrap();
}
#[tokio::test]
async fn clock_rollback_cannot_reset_quotas_or_extend_quote_binding() {
    let e = Env::new().await;
    let r = request();
    e.service(policy()).create(&r).await.unwrap();
    e.clock.0.store(999, Ordering::SeqCst);
    assert!(e.service(policy()).create(&request()).await.is_err());
    assert!(e.service(policy()).bind(&r, "addr").await.is_err());
    assert_eq!(e.oracle.calls.load(Ordering::SeqCst), 1);
}
#[tokio::test]
async fn quote_commit_failure_does_not_leave_a_new_rate_watermark() {
    let e = Env::new().await;
    sqlx::query("CREATE TRIGGER fail_quote BEFORE UPDATE ON xmr_quote_requests WHEN NEW.status='ready' BEGIN SELECT RAISE(ABORT,'injected'); END").execute(&e.pool).await.unwrap();
    assert!(e.service(policy()).create(&request()).await.is_err());
    assert_eq!(e.count("xmr_rate_watermarks").await, 0);
    let status: String = sqlx::query_scalar("SELECT status FROM xmr_quote_requests")
        .fetch_one(&e.pool)
        .await
        .unwrap();
    assert_eq!(status, "failed");
}
#[tokio::test]
async fn schema_prohibits_repricing_and_deletion_of_quote_bindings() {
    let e = Env::new().await;
    let svc = e.service(policy());
    let r = request();
    svc.create(&r).await.unwrap();
    svc.bind(&r, "addr").await.unwrap();
    for statement in [
        "UPDATE xmr_quote_requests SET document_json='{}'",
        "DELETE FROM xmr_quote_requests",
        "UPDATE xmr_quote_bindings SET receive_scope='replacement'",
        "DELETE FROM xmr_quote_bindings",
        "DELETE FROM xmr_quote_clock",
        "DELETE FROM xmr_rate_watermarks",
    ] {
        assert!(
            sqlx::query(statement).execute(&e.pool).await.is_err(),
            "{statement}"
        );
    }
}
#[tokio::test]
async fn policy_changes_do_not_rewrite_existing_quotes_or_credit_limits() {
    let e = Env::new().await;
    let r = request();
    let first = e.service(policy()).create(&r).await.unwrap();
    let mut p = policy();
    p.max_target_sats = 10;
    p.max_credit_sats = 100;
    p.lifetime_seconds = 1;
    let svc = e.service(p);
    let retried = svc.create(&r).await.unwrap();
    assert_eq!(first.terms_json().unwrap(), retried.terms_json().unwrap());
    assert_eq!(svc.bind(&r, "addr").await.unwrap().max_credit_sats, 20_000);
}
#[tokio::test]
async fn payer_terms_do_not_expose_private_rate_evidence_or_wallet_identifiers() {
    let e = Env::new().await;
    let r = request();
    let q = e.service(policy()).create(&r).await.unwrap();
    let text = q.terms_json().unwrap();
    for private in [
        "private-rate-evidence",
        "fixture-xmrbtc-v1",
        "test-private-account",
        r.client_bucket.as_str(),
        "raw_json",
        "numerator",
        "denominator",
    ] {
        assert!(!text.contains(private));
    }
    let data: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(data["amount"], "0.004");
    assert_eq!(data["amount_atomic"], "4000000000");
    assert_eq!(data["target_credit_sats"], 1000);
    let row = sqlx::query("SELECT document_json FROM xmr_quote_requests")
        .fetch_one(&e.pool)
        .await
        .unwrap();
    assert!(
        row.get::<String, _>("document_json")
            .contains("private-rate-evidence")
    );
}
#[test]
fn native_decimal_formatting_is_exact_and_never_scientific() {
    for (n, text) in [
        (0, "0"),
        (1, "0.000000000001"),
        (1_000, "0.000000001"),
        (4_000_000_000, "0.004"),
        (1_234_567_890_123, "1.234567890123"),
    ] {
        assert_eq!(format_xmr(n), text);
    }
    assert_eq!(format_xmr(u64::MAX), "18446744.073709551615");
}
#[test]
fn unsafe_policy_combinations_are_rejected() {
    for i in 0..8 {
        let mut p = policy();
        match i {
            0 => p.lifetime_seconds = 0,
            1 => p.max_rate_age_seconds = 86_401,
            2 => p.min_sats_per_xmr = p.max_sats_per_xmr + 1,
            3 => p.max_credit_sats = p.max_target_sats - 1,
            4 => p.max_requests_per_client_window = 21,
            5 => p.max_pending_requests = 21,
            6 => p.max_retained_requests = 19,
            _ => p.max_piconero = 0,
        };
        assert!(p.validate().is_err());
    }
}

#[tokio::test]
async fn saved_quote_remains_available_with_a_completely_unavailable_oracle() {
    let e = Env::new().await;
    let r = request();
    let saved = e
        .service(policy())
        .create(&r)
        .await
        .unwrap()
        .terms_json()
        .unwrap();
    let down = Arc::new(Oracle {
        clock: e.clock.clone(),
        rate: Mutex::new((1, 1, 900)),
        calls: AtomicUsize::new(0),
        fail: true,
        gate: None,
    });
    let service = QuoteService::new(
        LedgerStore::connect(&e.url).await.unwrap(),
        context(),
        policy(),
        down.clone(),
        e.clock.clone(),
    )
    .unwrap();
    assert_eq!(
        service.create(&r).await.unwrap().terms_json().unwrap(),
        saved
    );
    assert_eq!(
        service.lookup(&r).await.unwrap().terms_json().unwrap(),
        saved
    );
    assert_eq!(down.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn rate_watermark_is_shared_by_new_connections_and_service_instances() {
    let e = Env::new().await;
    e.service(policy()).create(&request()).await.unwrap();
    *e.oracle.rate.lock().unwrap() = (250_001, 1, 900);
    let reopened = LedgerStore::connect(&e.url).await.unwrap();
    let service = QuoteService::new(
        reopened,
        context(),
        policy(),
        e.oracle.clone(),
        e.clock.clone(),
    )
    .unwrap();
    assert!(service.create(&request()).await.is_err());
    let n: String = sqlx::query_scalar("SELECT numerator FROM xmr_rate_watermarks")
        .fetch_one(&e.pool)
        .await
        .unwrap();
    assert_eq!(n, "250000");
}

#[tokio::test]
async fn parallel_binding_attempts_cannot_create_two_payable_addresses() {
    let e = Env::new().await;
    let r = request();
    let a = e.service(policy());
    let b = e.service(policy());
    a.create(&r).await.unwrap();
    let (one, two) = tokio::join!(a.bind(&r, "addr-A"), b.bind(&r, "addr-B"));
    assert_ne!(one.is_ok(), two.is_ok());
    assert_eq!(e.count("xmr_quote_bindings").await, 1);
    assert_eq!(e.count("credit_valuations").await, 1);
}
