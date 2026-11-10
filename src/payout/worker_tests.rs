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
    let f = Fixture::new_with_nostr(nostr).await;
    let q = qualified(&f).await;
    let mut policy = journal_policy(&q);
    policy.execution_enabled = true;
    let url = format!("sqlite://{}", f.root.path().join("engine.db").display());
    let store = Store::connect(&url, policy.clone()).await.unwrap();
    store.admit(&f.plan, &q, now()).await.unwrap();
    let mock = Arc::new(Mutex::new(Mock::new(String::new())));
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
