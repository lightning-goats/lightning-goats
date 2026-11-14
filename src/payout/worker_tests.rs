use super::*;
use crate::payout::{
    signer,
    worker::{InvoiceFetcher, Step, Worker},
};
use std::sync::atomic::{AtomicUsize, Ordering};

async fn core(
    w: &Workflow,
    mode: RuntimeMode,
) -> (Client, tokio::task::JoinHandle<anyhow::Result<()>>) {
    let (uid, gid) = self_credentials().unwrap();
    let path =
        w.f.root
            .path()
            .join(format!("{}.sock", Uuid::new_v4().simple()));
    let config = BridgeConfig {
        socket: path.clone(),
        peer_uid: uid + 1,
        group_gid: gid,
        reward_policy: Some(w.f.policy.clone()),
        payout_peer_uid: Some(uid),
        delegated_feeder_messages: false,
        payout_execution_enabled: true,
        payout_account_id: Some(w.policy.scope.provider.account_id),
    };
    let reader = crate::payout::outcome::Reader::new(
        StrikeClient::new(&w.origin, "synthetic-core-read-key".into()).unwrap(),
        w.policy.scope.provider.account_id,
    )
    .unwrap();
    let task = tokio::spawn(Server::bind(&config).await.unwrap().run_with_reader(
        w.f.ledger.clone(),
        mode,
        Some(Arc::new(reader)),
    ));
    (Client::new(path, uid).unwrap(), task)
}
fn worker(w: &Workflow, core: Client, store: Store) -> Worker {
    Worker {
        presentation: None,
        store,
        core,
        signer: signer::Client::new(
            w.f.root.path().join("unused.sock"),
            self_credentials().unwrap().0,
        )
        .unwrap(),
        sender: Sender::test_client(w.policy.scope.provider.clone(), KEY.into(), &w.origin)
            .unwrap(),
    }
}
async fn tick(worker: &Worker) -> Step {
    tokio::time::sleep(std::time::Duration::from_millis(600)).await;
    worker.tick().await.unwrap()
}

/// SQLite creates a consistent snapshot, including committed WAL contents.
/// The core database is deliberately left at its newer state.
async fn snapshot(w: &Workflow, name: &str) -> String {
    let path = w.f.root.path().join(name);
    let db = sqlx::SqlitePool::connect(&w.url).await.unwrap();
    sqlx::query("VACUUM INTO ?")
        .bind(path.to_str().unwrap())
        .execute(&db)
        .await
        .unwrap();
    db.close().await;
    format!("sqlite://{}", path.display())
}

#[tokio::test]
async fn older_payout_snapshot_without_original_quote_cannot_create_replacement_payment() {
    let w = Workflow::new(false).await;
    let old = snapshot(&w, "before-quote.db").await;
    let (client, task) = core(&w, RuntimeMode::Active).await;
    let first = worker(&w, client, w.store.clone());
    assert_eq!(tick(&first).await, Step::QuoteSaved);
    assert_eq!(tick(&first).await, Step::Submitted);
    w.mock.lock().unwrap().state = PaymentState::Completed;
    assert_eq!(tick(&first).await, Step::Observed);
    let store = Store::connect(&old, w.policy.clone()).await.unwrap();
    let restored = worker(&w, first.core, store);
    tokio::time::sleep(std::time::Duration::from_millis(600)).await;
    assert!(restored.tick().await.is_err());
    assert_eq!(restored.store.held_sats().await.unwrap(), 2);
    {
        let state = w.mock.lock().unwrap();
        assert_eq!(state.payments.len(), 1);
        assert_eq!(state.post_keys.len(), 1);
        assert_eq!(state.patch_ids.len(), 1);
    }
    tokio::time::sleep(std::time::Duration::from_millis(600)).await;
    let state = restored
        .core
        .execution_state(
            w.f.reservation.epoch,
            w.f.reservation.id,
            &w.f.reservation.owner,
            &w.f.plan.rows[0].pubkey,
        )
        .await
        .unwrap();
    assert!(state.claim().is_some());
    assert_eq!(
        state.claim().unwrap().lease().record.request.reservation,
        w.f.reservation
    );
    task.abort();
}

#[tokio::test]
async fn reduced_payment_limit_blocks_scheduler_quote_but_keeps_known_payment_reconciliation() {
    let w = Workflow::new(false).await;
    let (client, task) = core(&w, RuntimeMode::Active).await;
    let mut policy = w.policy.clone();
    policy.max_net_sats = 0;
    let held = Store::connect(&w.url, policy).await.unwrap();
    let mut runner = worker(&w, client, held.clone());
    assert_eq!(tick(&runner).await, Step::Waiting);
    assert!(w.mock.lock().unwrap().post_keys.is_empty());
    runner.store = w.store.clone();
    assert_eq!(tick(&runner).await, Step::QuoteSaved);
    runner.store = held.clone();
    assert_eq!(tick(&runner).await, Step::Waiting);
    assert!(w.mock.lock().unwrap().patch_ids.is_empty());
    runner.store = w.store.clone();
    assert_eq!(tick(&runner).await, Step::Submitted);
    w.mock.lock().unwrap().state = PaymentState::Completed;
    runner.store = held;
    assert_eq!(tick(&runner).await, Step::Observed);
    assert_eq!(w.mock.lock().unwrap().payments.len(), 1);
    task.abort();
}

#[tokio::test]
async fn older_payout_snapshot_with_original_quote_recovers_same_payment_even_after_quote_expiry() {
    let w = Workflow::new(false).await;
    let (client, task) = core(&w, RuntimeMode::Active).await;
    let first = worker(&w, client, w.store.clone());
    assert_eq!(tick(&first).await, Step::QuoteSaved);
    let quote = w.store.current_quote(w.invoice_id).await.unwrap().unwrap();
    let terms = w.store.quote_terms(quote.id()).await.unwrap().unwrap();
    let old = snapshot(&w, "after-quote.db").await;
    assert_eq!(tick(&first).await, Step::Submitted);
    w.mock.lock().unwrap().state = PaymentState::Completed;
    assert_eq!(tick(&first).await, Step::Observed);
    let wait = terms.usable_until.saturating_sub(now()) + 1;
    tokio::time::sleep(std::time::Duration::from_secs(u64::try_from(wait).unwrap())).await;
    assert!(now() >= terms.usable_until);
    let store = Store::connect(&old, w.policy.clone()).await.unwrap();
    assert_eq!(
        store.payout_status(w.invoice_id).await.unwrap(),
        Status::QuoteReady
    );
    let restored = worker(&w, first.core, store);
    assert_eq!(tick(&restored).await, Step::Submitted);
    assert_eq!(tick(&restored).await, Step::Observed);
    let Step::Outcome(done) = tick(&restored).await else {
        panic!("expected final core outcome")
    };
    assert!(done.finalized);
    assert_eq!(done.spent_sats, 2);
    assert_eq!(restored.store.held_sats().await.unwrap(), 0);
    let state = w.mock.lock().unwrap();
    assert_eq!(state.payments.len(), 1);
    assert_eq!(state.post_keys.len(), 1);
    assert_eq!(state.patch_ids.len(), 2);
    assert!(state.patch_ids.iter().all(|id| *id == terms.quote_id));
    task.abort();
}

#[tokio::test]
async fn worker_recovers_lost_submission_after_restart_without_new_quote_payment_or_message() {
    let w = Workflow::new(false).await;
    let (client, task) = core(&w, RuntimeMode::Active).await;
    let first = worker(&w, client, w.store.clone());
    assert_eq!(tick(&first).await, Step::QuoteSaved);
    w.mock.lock().unwrap().lose_execute = true;
    tokio::time::sleep(std::time::Duration::from_millis(600)).await;
    assert!(first.tick().await.is_err());
    let store = Store::connect(&w.url, w.policy.clone()).await.unwrap();
    let client = first.core;
    let restored = worker(&w, client, store);
    assert_eq!(tick(&restored).await, Step::Submitted);
    w.mock.lock().unwrap().state = PaymentState::Completed;
    assert_eq!(tick(&restored).await, Step::Observed);
    let Step::Outcome(done) = tick(&restored).await else {
        panic!("expected verified outcome")
    };
    assert!(done.finalized);
    assert_eq!(done.spent_sats, 2);
    assert_eq!(restored.store.held_sats().await.unwrap(), 0);
    assert_eq!(tick(&restored).await, Step::Idle);
    assert_eq!(w.mock.lock().unwrap().payments.len(), 1);
    assert_eq!(w.mock.lock().unwrap().quotes.len(), 1);
    assert_eq!(w.mock.lock().unwrap().patch_ids.len(), 2);
    let db = sqlx::SqlitePool::connect(&w.f.database).await.unwrap();
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM message_outbox")
            .fetch_one(&db)
            .await
            .unwrap(),
        0
    );
    assert_eq!(w.f.ledger.feed_credit_sats().await.unwrap(), 0);
    task.abort();
}

#[tokio::test]
async fn disabled_execution_still_recovers_known_payments_and_final_core_outcomes() {
    let w = Workflow::new(false).await;
    let (client, task) = core(&w, RuntimeMode::Active).await;
    let first = worker(&w, client, w.store.clone());
    assert_eq!(tick(&first).await, Step::QuoteSaved);
    assert_eq!(tick(&first).await, Step::Submitted);
    let mut disabled = w.policy.clone();
    disabled.execution_enabled = false;
    let store = Store::connect(&w.url, disabled).await.unwrap();
    let (client, shadow_task) = core(&w, RuntimeMode::Shadow).await;
    let recovery = worker(&w, client, store);
    w.mock.lock().unwrap().state = PaymentState::Completed;
    assert_eq!(tick(&recovery).await, Step::Observed);
    let Step::Outcome(done) = tick(&recovery).await else {
        panic!("expected recovery")
    };
    assert!(done.finalized);
    assert_eq!(w.mock.lock().unwrap().patch_ids.len(), 1);
    task.abort();
    shadow_task.abort();
}

async fn unstarted(nostr: bool) -> Workflow {
    unstarted_with_options(nostr, false).await
}
async fn unstarted_with_options(nostr: bool, inclusive: bool) -> Workflow {
    let f = Fixture::new_with_options(nostr, inclusive).await;
    let q = qualified(&f).await;
    let mut policy = journal_policy(&q);
    policy.execution_enabled = true;
    if inclusive {
        policy.fee_mode = crate::payout::inclusive_fees::Mode::DeductFromShare;
        policy.max_fee_sats = 0;
    }
    let url = format!("sqlite://{}", f.root.path().join("engine.db").display());
    let store = Store::connect(&url, policy.clone()).await.unwrap();
    store.admit(&f.plan, &q, now()).await.unwrap();
    let mock = Arc::new(Mutex::new(Mock::new(String::new())));
    mock.lock().unwrap().adaptive_fee = inclusive;
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}/", listener.local_addr().unwrap());
    let app = Router::new()
        .route("/v1/payment-quotes/lightning", post(quote_http))
        .route("/v1/payment-quotes/{id}/execute", patch(execute_http))
        .route("/v1/payments/{id}", get(payment_http))
        .with_state(Arc::clone(&mock));
    let task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let sender = Sender::test_client(q.provider().clone(), KEY.into(), &origin).unwrap();
    Workflow {
        f,
        store,
        policy,
        url,
        invoice_id: Uuid::nil(),
        sender,
        origin,
        mock,
        task,
    }
}
struct Invoices {
    mock: Arc<Mutex<Mock>>,
    calls: Arc<AtomicUsize>,
}
impl InvoiceFetcher for Invoices {
    async fn fetch(
        &self,
        request: &crate::payout::invoice::Request,
    ) -> anyhow::Result<crate::payout::invoice::ValidatedInvoice> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        assert!(request.raw_zap().is_some());
        let invoice = crate::test_invoices::invoice(1000, request.description_hash());
        self.mock.lock().unwrap().invoice = invoice.clone();
        request.validate_response(&serde_json::to_vec(&json!({"pr":invoice})).unwrap(), now())
    }
}

#[tokio::test]
async fn worker_signs_exact_nip57_callback_only_with_public_authority_and_never_downgrades() {
    let w = unstarted(true).await;
    let (client, canary_task) = core(&w, RuntimeMode::Canary).await;
    let held = worker(&w, client, w.store.clone());
    assert_eq!(tick(&held).await, Step::Waiting);
    assert!(
        held.store
            .saved_invoice_intent(w.f.reservation.id, &w.f.plan.rows[0].pubkey)
            .await
            .unwrap()
            .is_none()
    );
    let (client, active_task) = core(&w, RuntimeMode::Active).await;
    let mut engine = worker(&w, client, w.store.clone());
    let (uid, gid) = self_credentials().unwrap();
    let socket = w.f.root.path().join("signer.sock");
    let policy = signer::Policy {
        owner: owner(),
        client_pubkey: crate::nostr_event::test_event(4, 0, 100, "", vec![]).pubkey,
        peer_uid: uid,
        relays: w.policy.receipt_relays.clone(),
        max_net_sats: 100,
        max_new_per_hour: 4,
    };
    let journal = signer::Store::connect(
        &format!("sqlite://{}", w.f.root.path().join("signer.db").display()),
        policy,
    )
    .await
    .unwrap();
    let count = Arc::new(AtomicUsize::new(0));
    let broker = tokio::spawn(
        signer::Server::bind(&socket, gid, uid)
            .await
            .unwrap()
            .run_with(
                journal,
                signer::tests::Fake {
                    calls: Arc::clone(&count),
                    fail: false,
                    wrong: false,
                },
            ),
    );
    engine.signer = signer::Client::new(socket, uid).unwrap();
    assert_eq!(tick(&engine).await, Step::InvoiceIntent);
    assert_eq!(tick(&engine).await, Step::InvoiceSigned);
    let invoices = Invoices {
        mock: Arc::clone(&w.mock),
        calls: Arc::new(AtomicUsize::new(0)),
    };
    tokio::time::sleep(std::time::Duration::from_millis(600)).await;
    assert_eq!(
        engine.tick_with(&invoices).await.unwrap(),
        Step::InvoiceSaved
    );
    assert_eq!(count.load(Ordering::SeqCst), 1);
    assert_eq!(invoices.calls.load(Ordering::SeqCst), 1);
    assert_eq!(tick(&engine).await, Step::QuoteSaved);
    assert_eq!(tick(&engine).await, Step::Submitted);
    w.mock.lock().unwrap().state = PaymentState::Completed;
    assert_eq!(tick(&engine).await, Step::Observed);
    assert!(matches!(tick(&engine).await,Step::Outcome(p) if p.finalized));
    assert_eq!(count.load(Ordering::SeqCst), 1);
    broker.abort();
    canary_task.abort();
    active_task.abort();
}

struct FeeInvoices {
    mock: Arc<Mutex<Mock>>,
    calls: AtomicUsize,
    nostr: bool,
}
impl InvoiceFetcher for FeeInvoices {
    async fn fetch(
        &self,
        request: &crate::payout::invoice::Request,
    ) -> anyhow::Result<crate::payout::invoice::ValidatedInvoice> {
        let n = self.calls.fetch_add(1, Ordering::SeqCst);
        assert_eq!(request.raw_zap().is_some(), self.nostr);
        let encoded = crate::test_invoices::invoice_with_payment_hash(
            request.amount_sats() * 1000,
            request.description_hash(),
            std::time::Duration::from_secs(now().try_into().unwrap()),
            [u8::try_from(n + 20).unwrap(); 32],
        );
        self.mock.lock().unwrap().invoice = encoded.clone();
        request.validate_invoice(&encoded, now())
    }
}

#[tokio::test]
async fn inclusive_worker_deducts_actual_fee_promotes_once_and_recovers_original_execution() {
    for nostr in [false, true] {
        let w = unstarted_with_options(nostr, true).await;
        let (client, active) = core(&w, RuntimeMode::Active).await;
        let mut engine = worker(&w, client, w.store.clone());
        let count = Arc::new(AtomicUsize::new(0));
        let broker = if nostr {
            let (uid, gid) = self_credentials().unwrap();
            let socket = w.f.root.path().join("inclusive-signer.sock");
            let policy = signer::Policy {
                owner: owner(),
                client_pubkey: crate::nostr_event::test_event(4, 0, 100, "", vec![]).pubkey,
                peer_uid: uid,
                relays: w.policy.receipt_relays.clone(),
                max_net_sats: 100,
                max_new_per_hour: 4,
            };
            let journal = signer::Store::connect(
                &format!(
                    "sqlite://{}",
                    w.f.root.path().join("inclusive-signer.db").display()
                ),
                policy,
            )
            .await
            .unwrap();
            let task = tokio::spawn(
                signer::Server::bind(&socket, gid, uid)
                    .await
                    .unwrap()
                    .run_with(
                        journal,
                        signer::tests::Fake {
                            calls: Arc::clone(&count),
                            fail: false,
                            wrong: false,
                        },
                    ),
            );
            engine.signer = signer::Client::new(socket, uid).unwrap();
            Some(task)
        } else {
            None
        };
        let invoices = FeeInvoices {
            mock: Arc::clone(&w.mock),
            calls: AtomicUsize::new(0),
            nostr,
        };
        assert_eq!(tick(&engine).await, Step::InvoiceIntent);
        assert_eq!(tick(&engine).await, Step::InvoiceSigned);
        tokio::time::sleep(std::time::Duration::from_millis(600)).await;
        assert_eq!(
            engine.tick_with(&invoices).await.unwrap(),
            Step::InvoiceSaved
        );
        assert_eq!(tick(&engine).await, Step::FeeObserved);
        assert!(w.mock.lock().unwrap().patch_ids.is_empty());
        assert_eq!(tick(&engine).await, Step::InvoiceIntent);
        engine.store = Store::connect(&w.url, w.policy.clone()).await.unwrap();
        assert_eq!(tick(&engine).await, Step::InvoiceSigned);
        tokio::time::sleep(std::time::Duration::from_millis(600)).await;
        assert_eq!(
            engine.tick_with(&invoices).await.unwrap(),
            Step::InvoiceSaved
        );
        assert_eq!(tick(&engine).await, Step::FeeObserved);
        let member = &w.f.plan.rows[0].pubkey;
        assert!(
            engine
                .store
                .saved_invoice_intent(w.f.reservation.id, member)
                .await
                .unwrap()
                .is_none()
        );
        let (_, last) = engine
            .store
            .latest_fee_probe(w.f.reservation.id, member)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(last.0.net_sats, 1);
        assert_eq!(tick(&engine).await, Step::QuoteSaved);
        assert_eq!(
            engine
                .store
                .saved_invoice_intent(w.f.reservation.id, member)
                .await
                .unwrap()
                .unwrap()
                .id(),
            last.id()
        );
        let promoted = engine
            .store
            .promote_fee_probe(w.f.reservation.id, member, now())
            .await
            .unwrap();
        let original = engine
            .store
            .current_quote(last.id())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(promoted.id(), original.id());
        w.mock.lock().unwrap().lose_execute = true;
        tokio::time::sleep(std::time::Duration::from_millis(600)).await;
        assert!(engine.tick().await.is_err());
        // Prove the simulated lost reply followed actual provider submission,
        // rather than treating an unrelated IPC/rate failure as that evidence.
        assert_eq!(w.mock.lock().unwrap().patch_ids.len(), 1);
        engine.store = Store::connect(&w.url, w.policy.clone()).await.unwrap();
        assert_eq!(tick(&engine).await, Step::Submitted);
        w.mock.lock().unwrap().state = PaymentState::Completed;
        assert_eq!(tick(&engine).await, Step::Observed);
        let Step::Outcome(done) = tick(&engine).await else {
            panic!("missing authoritative outcome")
        };
        assert!(done.finalized);
        assert_eq!(done.spent_sats, 2);
        assert_eq!(engine.store.held_sats().await.unwrap(), 0);
        assert_eq!(tick(&engine).await, Step::Idle);
        assert_eq!(invoices.calls.load(Ordering::SeqCst), 2);
        assert_eq!(count.load(Ordering::SeqCst), if nostr { 2 } else { 0 });
        {
            let mock = w.mock.lock().unwrap();
            assert_eq!(mock.quotes.len(), 2);
            assert_eq!(mock.payments.len(), 1);
            assert_eq!(mock.patch_ids.len(), 2);
            assert_eq!(mock.patch_ids[0], mock.patch_ids[1]);
        }
        assert_eq!(w.f.ledger.feed_credit_sats().await.unwrap(), 0);
        let db = sqlx::SqlitePool::connect(&w.url).await.unwrap();
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM payout_fee_promotions")
                .fetch_one(&db)
                .await
                .unwrap(),
            1
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM payout_execute_intents")
                .fetch_one(&db)
                .await
                .unwrap(),
            1
        );
        active.abort();
        if let Some(broker) = broker {
            broker.abort();
        }
    }
}
