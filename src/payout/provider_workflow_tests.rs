//! Actual HTTP and private Unix framing around temporary core/payout stores.
//! No production key, provider, relay or physical gateway is involved.
use super::*;
use crate::payout::{
    provider::{Client as Sender, PaymentState, QuoteReply},
    store::{PayoutStatus as Status, Store},
};
use axum::routing::{patch, post};
use std::{collections::BTreeMap, sync::Mutex};

#[path = "intake_tests.rs"]
mod intake_tests;
#[path = "outcome_workflow_tests.rs"]
mod outcome_workflow_tests;
#[path = "worker_tests.rs"]
mod worker_tests;

fn now() -> i64 {
    crate::cyberherd::calendar::now().unwrap()
}
struct Mock {
    invoice: String,
    quotes: BTreeMap<Uuid, Uuid>,
    payments: BTreeMap<Uuid, Uuid>,
    post_keys: Vec<Uuid>,
    patch_ids: Vec<Uuid>,
    get_ids: Vec<Uuid>,
    lose_quote: bool,
    lose_execute: bool,
    state: PaymentState,
    bad_money: Option<serde_json::Value>,
    wrong_hash: bool,
    payment_money: Option<serde_json::Value>,
}
impl Mock {
    fn new(invoice: String) -> Self {
        Self {
            invoice,
            quotes: BTreeMap::new(),
            payments: BTreeMap::new(),
            post_keys: vec![],
            patch_ids: vec![],
            get_ids: vec![],
            lose_quote: false,
            lose_execute: false,
            state: PaymentState::Pending,
            bad_money: None,
            wrong_hash: false,
            payment_money: None,
        }
    }
}
fn payment(id: Uuid, state: PaymentState) -> serde_json::Value {
    json!({"paymentId":id,"state":state,"amount":{"amount":"0.00000001","currency":"BTC"},
        "totalAmount":{"amount":"0.00000002","currency":"BTC"}})
}
fn authorized(headers: &HeaderMap) -> bool {
    headers.get("authorization").and_then(|v| v.to_str().ok())
        == Some("Bearer synthetic-payout-read-key")
}
async fn quote_http(
    State(s): State<Arc<Mutex<Mock>>>,
    headers: HeaderMap,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    if !authorized(&headers) {
        return (StatusCode::UNAUTHORIZED, Json(json!({})));
    }
    let key: Uuid = headers
        .get("idempotency-key")
        .unwrap()
        .to_str()
        .unwrap()
        .parse()
        .unwrap();
    assert_eq!(key.get_version_num(), 4);
    let mut s = s.lock().unwrap();
    assert_eq!(body, json!({"lnInvoice":s.invoice,"sourceCurrency":"BTC"}));
    s.post_keys.push(key);
    if let Some(id) = s.quotes.get(&key) {
        return (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(
                json!({"data":{"status":422,"code":"DUPLICATE_PAYMENT_QUOTE","values":{"paymentQuoteId":id}}}),
            ),
        );
    }
    let id = Uuid::new_v4();
    s.quotes.insert(key, id);
    if s.lose_quote {
        s.lose_quote = false;
        return (
            StatusCode::BAD_GATEWAY,
            Json(
                json!({"data":{"status":502,"code":"UNKNOWN","values":{},"message":"synthetic-private"}}),
            ),
        );
    }
    let mut value = payment(id, PaymentState::Pending);
    value.as_object_mut().unwrap().remove("paymentId");
    value["paymentQuoteId"] = json!(id);
    if let Some(money) = &s.bad_money {
        for (key, value2) in money.as_object().unwrap() {
            value[key] = value2.clone();
        }
    }
    (StatusCode::OK, Json(value))
}
async fn execute_http(
    State(s): State<Arc<Mutex<Mock>>>,
    Path(id): Path<Uuid>,
    headers: HeaderMap,
) -> impl IntoResponse {
    assert!(authorized(&headers));
    let mut s = s.lock().unwrap();
    assert!(s.quotes.values().any(|v| *v == id));
    s.patch_ids.push(id);
    if let Some(pid) = s.payments.get(&id) {
        return (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(
                json!({"data":{"status":422,"code":"PAYMENT_PROCESSED","values":{"paymentId":pid}}}),
            ),
        );
    }
    let pid = Uuid::new_v4();
    s.payments.insert(id, pid);
    if s.lose_execute {
        s.lose_execute = false;
        return (
            StatusCode::BAD_GATEWAY,
            Json(
                json!({"data":{"status":502,"code":"UNKNOWN","values":{},"message":"synthetic-private"}}),
            ),
        );
    }
    (StatusCode::ACCEPTED, Json(payment(pid, s.state)))
}
async fn payment_http(
    State(s): State<Arc<Mutex<Mock>>>,
    Path(id): Path<Uuid>,
    headers: HeaderMap,
) -> impl IntoResponse {
    assert!(
        authorized(&headers)
            || headers.get("authorization").and_then(|v| v.to_str().ok())
                == Some("Bearer synthetic-core-read-key")
    );
    let mut s = s.lock().unwrap();
    assert!(s.payments.values().any(|v| *v == id));
    s.get_ids.push(id);
    let mut value = payment(id, s.state);
    if let Some(money) = &s.payment_money {
        for (key, amount) in money.as_object().unwrap() {
            value[key] = amount.clone();
        }
    }
    if s.wrong_hash {
        value["lightning"] = json!({"preImage":"11".repeat(32)});
    }
    (StatusCode::OK, Json(value))
}

struct Workflow {
    f: Fixture,
    store: Store,
    policy: crate::payout::store::Policy,
    url: String,
    invoice_id: Uuid,
    sender: Sender,
    origin: String,
    mock: Arc<Mutex<Mock>>,
    task: tokio::task::JoinHandle<()>,
}
impl Drop for Workflow {
    fn drop(&mut self) {
        self.task.abort();
    }
}
impl Workflow {
    async fn new(nostr: bool) -> Self {
        let f = Fixture::new_with_nostr(nostr).await;
        let q = qualified(&f).await;
        let mut policy = journal_policy(&q);
        policy.execution_enabled = true;
        let url = format!("sqlite://{}", f.root.path().join("outgoing.db").display());
        let store = Store::connect(&url, policy.clone()).await.unwrap();
        let grant = store.admit(&f.plan, &q, now()).await.unwrap();
        let intent = store
            .prepare_invoice(grant.id, &f.plan.rows[0].pubkey, now())
            .await
            .unwrap();
        let signed = intent.unsigned_zap().unwrap().map(|u| {
            serde_json::to_string(&crate::nostr_event::test_event(
                1,
                u.kind,
                u.created_at,
                &u.content,
                u.tags,
            ))
            .unwrap()
        });
        let request = store
            .freeze_request(intent.id(), signed.as_deref(), now())
            .await
            .unwrap();
        let encoded = crate::test_invoices::invoice(1000, request.description_hash());
        let invoice = request
            .validate_response(&serde_json::to_vec(&json!({"pr":encoded})).unwrap(), now())
            .unwrap();
        store.freeze_invoice(&invoice, now()).await.unwrap();
        let mock = Arc::new(Mutex::new(Mock::new(encoded)));
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let app = Router::new()
            .route("/v1/payment-quotes/lightning", post(quote_http))
            .route("/v1/payment-quotes/{id}/execute", patch(execute_http))
            .route("/v1/payments/{id}", get(payment_http))
            .with_state(Arc::clone(&mock));
        let task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let origin = format!("http://{address}/");
        let sender = Sender::test_client(q.provider().clone(), KEY.into(), &origin).unwrap();
        Self {
            f,
            store,
            policy,
            url,
            invoice_id: intent.id(),
            sender,
            origin,
            mock,
            task,
        }
    }
    async fn fresh(&self, enabled: bool, nostr: bool) -> AuthenticatedFundingPage {
        let mut page = self.f.page(0, enabled).await;
        page.nostr_enabled = nostr;
        authenticate(page).await
    }
    async fn quote(&self) -> crate::payout::provider::QuoteIntent {
        let intent = self
            .store
            .prepare_quote(self.invoice_id, now())
            .await
            .unwrap();
        let QuoteReply::Ready(quote) = self.sender.create_quote(&intent).await.unwrap() else {
            panic!("expected new quote")
        };
        self.store.freeze_quote(&quote, now()).await.unwrap();
        intent
    }
}

#[tokio::test]
async fn lost_execute_response_recovers_only_same_quote_and_original_payment_across_restart() {
    let w = Workflow::new(false).await;
    let intent = w.quote().await;
    let permit = w
        .store
        .prepare_execute(intent.id(), &w.fresh(true, false).await, now())
        .await
        .unwrap();
    w.mock.lock().unwrap().lose_execute = true;
    let error = w.sender.execute(&permit).await.err().unwrap();
    assert!(!format!("{error:#}").contains("synthetic-private"));
    assert_eq!(
        w.store.payout_status(w.invoice_id).await.unwrap(),
        Status::ExecutionUnknown
    );
    let restored = Store::connect(&w.url, w.policy.clone()).await.unwrap();
    assert!(restored.retire_quote(intent.id(), now()).await.is_err());
    assert!(restored.prepare_quote(w.invoice_id, now()).await.is_err());
    let retry = restored
        .prepare_execute(intent.id(), &w.fresh(true, false).await, now())
        .await
        .unwrap();
    let recovered = w.sender.execute(&retry).await.unwrap();
    let bound = restored.bind_payment(&recovered, now()).await.unwrap();
    assert_eq!(w.mock.lock().unwrap().patch_ids.len(), 2);
    assert_eq!(w.mock.lock().unwrap().payments.len(), 1);
    assert!(
        restored
            .prepare_execute(intent.id(), &w.fresh(true, false).await, now())
            .await
            .is_err()
    );
    let pending = w.sender.read_payment(&bound).await.unwrap();
    restored.observe_payment(&pending, now()).await.unwrap();
    assert_eq!(
        restored.payout_status(w.invoice_id).await.unwrap(),
        Status::Pending
    );
    w.mock.lock().unwrap().state = PaymentState::Completed;
    let done = w.sender.read_payment(&bound).await.unwrap();
    restored.observe_payment(&done, now()).await.unwrap();
    restored.observe_payment(&done, now()).await.unwrap();
    assert_eq!(
        restored.payout_status(w.invoice_id).await.unwrap(),
        Status::Completed
    );
    let mut held = w.policy.clone();
    held.execution_enabled = false;
    let held = Store::connect(&w.url, held).await.unwrap();
    let original = held.bound_payment(intent.id()).await.unwrap().unwrap();
    assert_eq!(original.payment_id(), bound.payment_id());
    assert_eq!(held.held_sats().await.unwrap(), 2); // terminal core budget protocol is still separate
    let db = sqlx::SqlitePool::connect(&w.url).await.unwrap();
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM payout_execute_intents")
            .fetch_one(&db)
            .await
            .unwrap(),
        1
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM payout_payment_records")
            .fetch_one(&db)
            .await
            .unwrap(),
        2
    );
    assert_eq!(w.f.ledger.feed_credit_sats().await.unwrap(), 0);
}

#[tokio::test]
async fn lost_quote_response_cannot_execute_id_only_and_can_retire_only_before_execute_intent() {
    let w = Workflow::new(false).await;
    w.mock.lock().unwrap().lose_quote = true;
    let original = w.store.prepare_quote(w.invoice_id, now()).await.unwrap();
    assert!(w.sender.create_quote(&original).await.is_err());
    let restored = Store::connect(&w.url, w.policy.clone()).await.unwrap();
    let retry = restored.prepare_quote(w.invoice_id, now()).await.unwrap();
    assert_eq!(retry.id(), original.id());
    let QuoteReply::Duplicate(duplicate) = w.sender.create_quote(&retry).await.unwrap() else {
        panic!("expected correlation only")
    };
    restored
        .observe_duplicate_quote(&duplicate, now())
        .await
        .unwrap();
    assert!(
        restored
            .prepare_execute(retry.id(), &w.fresh(true, false).await, now())
            .await
            .is_err()
    );
    assert!(w.mock.lock().unwrap().patch_ids.is_empty());
    restored.retire_quote(retry.id(), now()).await.unwrap();
    let next = restored.prepare_quote(w.invoice_id, now()).await.unwrap();
    assert_ne!(next.id(), retry.id());
    let QuoteReply::Ready(quote) = w.sender.create_quote(&next).await.unwrap() else {
        panic!("expected verified replacement quote")
    };
    restored.freeze_quote(&quote, now()).await.unwrap();
    restored
        .prepare_execute(next.id(), &w.fresh(true, false).await, now())
        .await
        .unwrap();
    assert!(restored.retire_quote(next.id(), now()).await.is_err());
    assert!(restored.prepare_quote(w.invoice_id, now()).await.is_err());
    let m = w.mock.lock().unwrap();
    assert_eq!(m.quotes.len(), 2);
    assert!(m.payments.is_empty());
}

#[tokio::test]
async fn independent_holds_stale_core_policy_shrink_and_canary_receipts_block_execution() {
    let w = Workflow::new(true).await;
    let intent = w.quote().await;
    let mut policy = w.policy.clone();
    policy.execution_enabled = false;
    let disabled = Store::connect(&w.url, policy).await.unwrap();
    assert!(
        disabled
            .prepare_execute(intent.id(), &w.fresh(true, true).await, now())
            .await
            .is_err()
    );
    assert!(
        w.store
            .prepare_execute(intent.id(), &w.fresh(false, true).await, now())
            .await
            .is_err()
    );
    assert!(
        w.store
            .prepare_execute(intent.id(), &w.fresh(true, false).await, now())
            .await
            .is_err()
    );
    let mut stale = w.f.page(0, true).await;
    stale.nostr_enabled = true;
    stale.observed_at = now() - 6;
    assert!(
        w.store
            .prepare_execute(intent.id(), &authenticate(stale).await, now())
            .await
            .is_err()
    );
    let mut policy = w.policy.clone();
    policy.max_fee_sats = 0;
    let shrunk = Store::connect(&w.url, policy).await.unwrap();
    assert!(
        shrunk
            .prepare_execute(intent.id(), &w.fresh(true, true).await, now())
            .await
            .is_err()
    );
    let db = sqlx::SqlitePool::connect(&w.url).await.unwrap();
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM payout_execute_intents")
            .fetch_one(&db)
            .await
            .unwrap(),
        0
    );
    let permit = w
        .store
        .prepare_execute(intent.id(), &w.fresh(true, true).await, now())
        .await
        .unwrap();
    let mut expired = permit.clone();
    expired.permitted_until = now() - 1;
    assert!(w.sender.execute(&expired).await.is_err());
    assert!(w.mock.lock().unwrap().patch_ids.is_empty());
    let observation = w.sender.execute(&permit).await.unwrap();
    w.store.bind_payment(&observation, now()).await.unwrap();
    assert_eq!(w.mock.lock().unwrap().payments.len(), 1);
}

#[tokio::test]
async fn wrong_currency_amount_fees_expiry_and_identity_never_create_execution_authority() {
    let w = Workflow::new(false).await;
    for bad in [
        json!({"amount":{"currency":"USD","amount":"0.00000001"}}),
        json!({"amount":{"currency":"BTC","amount":"0.000000011"}}),
        json!({"amount":{"currency":"BTC","amount":"-0.00000001"}}),
        json!({"amount":{"currency":"BTC","amount":"0.00000002"}}),
        json!({"totalAmount":{"currency":"BTC","amount":"0.00000003"}}),
        json!({"totalFee":{"currency":"BTC","amount":"0"}}),
        json!({"validUntil":"2000-01-01T00:00:00Z"}),
        json!({"conversionRate":{"sourceCurrency":"USD","targetCurrency":"BTC","amount":"1"}}),
    ] {
        w.mock.lock().unwrap().bad_money = Some(bad);
        let intent = w.store.prepare_quote(w.invoice_id, now()).await.unwrap();
        assert!(w.sender.create_quote(&intent).await.is_err());
        assert_eq!(
            w.store.payout_status(w.invoice_id).await.unwrap(),
            Status::QuoteUnknown
        );
        w.store.retire_quote(intent.id(), now()).await.unwrap();
    }
    w.mock.lock().unwrap().bad_money = None;
    let intent = w.store.prepare_quote(w.invoice_id, now()).await.unwrap();
    let mut scope = w.policy.scope.provider.clone();
    scope.account_id = Uuid::new_v4();
    let wrong = Sender::test_client(scope, KEY.into(), &w.origin);
    // Credential mismatch is checked independently; account mismatch against
    // an actual committed capability is covered by the provider's scope gate.
    assert!(Sender::new(w.policy.scope.provider.clone(), "different-key".into()).is_err());
    assert!(wrong.is_ok());
    assert!(wrong.unwrap().create_quote(&intent).await.is_err());
    assert!(w.mock.lock().unwrap().patch_ids.is_empty());
}

#[tokio::test]
async fn authoritative_internal_payment_reads_allow_no_preimage_and_freeze_terminal_conflicts() {
    let w = Workflow::new(false).await;
    let quote = w.quote().await;
    let permit = w
        .store
        .prepare_execute(quote.id(), &w.fresh(true, false).await, now())
        .await
        .unwrap();
    let execution = w.sender.execute(&permit).await.unwrap();
    let bound = w.store.bind_payment(&execution, now()).await.unwrap();
    w.mock.lock().unwrap().state = PaymentState::Completed;
    w.mock.lock().unwrap().wrong_hash = true;
    assert!(w.sender.read_payment(&bound).await.is_err());
    assert_eq!(
        w.store.payout_status(w.invoice_id).await.unwrap(),
        Status::Pending
    );
    w.mock.lock().unwrap().wrong_hash = false;
    let done = w.sender.read_payment(&bound).await.unwrap();
    w.store.observe_payment(&done, now()).await.unwrap();
    assert_eq!(
        w.store.payout_status(w.invoice_id).await.unwrap(),
        Status::Completed
    );
    w.mock.lock().unwrap().state = PaymentState::Failed;
    let contradicted = w.sender.read_payment(&bound).await.unwrap();
    assert!(w.store.observe_payment(&contradicted, now()).await.is_err());
    let reopened = Store::connect(&w.url, w.policy.clone()).await.unwrap();
    assert_eq!(
        reopened.payout_status(w.invoice_id).await.unwrap(),
        Status::Conflicted
    );
    assert!(reopened.retire_quote(quote.id(), now()).await.is_err());
    assert_eq!(reopened.held_sats().await.unwrap(), 2);
}
