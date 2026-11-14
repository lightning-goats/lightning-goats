use super::*;
use crate::{
    domain::payment::SettledPayment,
    payout::{
        funding::ProviderScope,
        outcome::{Disposition, Row as OutcomeRow},
        provider::{BindingRecord, PaymentState, QuoteDocument, QuoteRecord},
    },
    reward_funding::{Reservation, RewardPolicy},
};
use axum::{
    Json, Router,
    extract::{Path, State},
    http::HeaderMap,
    routing::get,
};
use uuid::Uuid;

async fn fixture() -> (tempfile::TempDir, LedgerStore, RewardPolicy, Vec<Uuid>) {
    let root = tempfile::TempDir::new().unwrap();
    let store = LedgerStore::connect(&format!(
        "sqlite://{}",
        root.path().join("ledger.db").display()
    ))
    .await
    .unwrap();
    store.initialize_reward_funding().await.unwrap();
    store
        .record_payment(&SettledPayment {
            source: "strike".into(),
            source_id: "synthetic-window-source".into(),
            payment_hash: None,
            address_user: "herd".into(),
            credit_pool: "herd".into(),
            amount_msat: 3_000_000,
            settled_at: None,
            context_json: None,
        })
        .await
        .unwrap();
    let mut feeds = Vec::new();
    for _ in 0..3 {
        let id = store.begin_feed_attempt(1000).await.unwrap().unwrap();
        store.confirm_feed_attempt(id).await.unwrap();
        feeds.push(id);
    }
    let policy = RewardPolicy {
        owner: crate::nostr_event::test_event(1, 0, 100, "", vec![]).pubkey,
        member_percent_ceiling: 10,
        max_sats_per_feed: 100,
        max_sats_per_24h: 200,
    };
    (root, store, policy, feeds)
}
async fn reserve(store: &LedgerStore, policy: &RewardPolicy, feed: Uuid, sats: u64) -> Reservation {
    store
        .reserve_reward(
            policy,
            store.overlay_stream_id().await.unwrap(),
            feed,
            &"ab".repeat(32),
            sats,
        )
        .await
        .unwrap()
}
async fn register_claim(store: &LedgerStore, policy: &RewardPolicy, packet: &Packet) {
    let Disposition::Payment { binding_json, .. } = &packet.row.disposition else {
        return;
    };
    let binding: BindingRecord = serde_json::from_str(binding_json).unwrap();
    let request = crate::payout::execution::Request {
        reservation: packet.header.reservation.clone(),
        member: packet.row.member.clone(),
        quote_json: serde_json::to_string(&binding.quote).unwrap(),
    };
    store
        .claim_payout_execution(
            &request,
            crate::payout::execution::Limits {
                policy: Some(policy),
                account: Some(packet.header.scope.account_id),
                enabled: true,
                nostr: true,
            },
        )
        .await
        .unwrap();
}
fn report(reservation: Reservation, state: PaymentState) -> Packet {
    let now = crate::cyberherd::calendar::now().unwrap();
    let scope = ProviderScope {
        environment: "strike-production".into(),
        account_id: Uuid::from_u128(1),
        credential_fingerprint: "aa".repeat(32),
    };
    let encoded = crate::test_invoices::invoice(90_000, &"11".repeat(32));
    let invoice =
        crate::bolt11::verify(&encoded, 90_000, &"22".repeat(32), &"11".repeat(32)).unwrap();
    let quote = QuoteRecord {
        request: QuoteDocument {
            id: Uuid::new_v4(),
            invoice_id: Uuid::new_v4(),
            scope: scope.clone(),
            encoded,
            payment_hash: "22".repeat(32),
            description_hash: "11".repeat(32),
            amount_sats: 90,
            fee_limit_sats: 10,
            gross_sats: 100,
            invoice_expires_at: invoice.expires_at().unwrap().as_secs(),
            created_at: now,
            nip57: false,
        },
        quote_id: Uuid::new_v4(),
        fee_sats: 5,
        total_sats: 95,
        provider_valid_until: None,
        usable_until: now + 30,
        observed_at: now,
    };
    quote.validate().unwrap();
    let binding = BindingRecord {
        quote,
        payment_id: Uuid::new_v4(),
    };
    let row = OutcomeRow {
        member: crate::nostr_event::test_event(2, 0, 100, "", vec![]).pubkey,
        gross_sats: 100,
        disposition: Disposition::Payment {
            binding_json: serde_json::to_string(&binding).unwrap(),
            state,
            fee_sats: 5,
            total_sats: 95,
        },
    };
    let mut header = Header {
        reservation,
        scope,
        rows: 1,
        final_hash: "00".repeat(32),
    };
    let seed = header.seed().unwrap();
    header.final_hash = next_hash(&seed, &row).unwrap();
    Packet {
        header,
        after: 0,
        before_hash: seed,
        row,
    }
}
async fn read_payment(
    State(state): State<PaymentState>,
    Path(id): Path<Uuid>,
    headers: HeaderMap,
) -> Json<serde_json::Value> {
    assert_eq!(
        headers.get("authorization").unwrap(),
        "Bearer synthetic-ledger-read-key"
    );
    Json(
        serde_json::json!({"paymentId":id,"state":state,"amount":{"amount":"0.00000090","currency":"BTC"},"totalAmount":{"amount":"0.00000095","currency":"BTC"}}),
    )
}
async fn provider(state: PaymentState) -> (Reader, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let app = Router::new()
        .route("/v1/payments/{id}", get(read_payment))
        .with_state(state);
    let task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (
        Reader::new(
            crate::strike::StrikeClient::new(
                &format!("http://{address}/"),
                "synthetic-ledger-read-key".into(),
            )
            .unwrap(),
            Uuid::from_u128(1),
        )
        .unwrap(),
        task,
    )
}

#[tokio::test]
async fn rolling_budget_ages_only_verified_spend_and_rejects_reused_payment_or_invoice() {
    let (_root, store, policy, feeds) = fixture().await;
    let first = reserve(&store, &policy, feeds[0], 100).await;
    let second = reserve(&store, &policy, feeds[1], 100).await;
    let packet = report(first, PaymentState::Completed);
    register_claim(&store, &policy, &packet).await;
    let (reader, task) = provider(PaymentState::Completed).await;
    let outcome = store
        .reconcile_payout_outcome(&packet, &reader)
        .await
        .unwrap();
    assert_eq!((outcome.spent_sats, outcome.released_sats), (95, 5));
    let mut tx = store.pool.begin().await.unwrap();
    assert_eq!(
        budget_usage(&mut tx, outcome.verified_at).await.unwrap(),
        195
    );
    assert_eq!(
        budget_usage(&mut tx, outcome.verified_at + 86399)
            .await
            .unwrap(),
        195
    );
    assert_eq!(
        budget_usage(&mut tx, outcome.verified_at + 86400)
            .await
            .unwrap(),
        100
    );
    tx.commit().await.unwrap();
    let mut duplicate = packet.clone();
    duplicate.header.reservation = second;
    duplicate.before_hash = duplicate.header.seed().unwrap();
    duplicate.header.final_hash = next_hash(&duplicate.before_hash, &duplicate.row).unwrap();
    assert!(
        store
            .reconcile_payout_outcome(&duplicate, &reader)
            .await
            .is_err()
    );
    assert_eq!(
        store
            .payout_outcome_progress(&duplicate.header)
            .await
            .unwrap()
            .through,
        0
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM reward_outcome_rows")
            .fetch_one(&store.pool)
            .await
            .unwrap(),
        1
    );
    let epoch = store.overlay_stream_id().await.unwrap();
    assert!(
        store
            .reserve_reward(&policy, epoch, feeds[2], &"cd".repeat(32), 6)
            .await
            .is_err()
    );
    assert_eq!(reserve(&store, &policy, feeds[2], 5).await.reserved_sats, 5);
    assert_eq!(store.feed_credit_sats().await.unwrap(), 0);
    task.abort();
}

#[tokio::test]
async fn failed_costs_never_age_out_and_wrong_final_hash_rolls_back_every_write() {
    let (_root, store, policy, feeds) = fixture().await;
    let r = reserve(&store, &policy, feeds[0], 100).await;
    let packet = report(r, PaymentState::Failed);
    register_claim(&store, &policy, &packet).await;
    let (reader, task) = provider(PaymentState::Failed).await;
    let mut bad = packet.clone();
    bad.header.final_hash = "ff".repeat(32);
    assert!(store.reconcile_payout_outcome(&bad, &reader).await.is_err());
    assert_eq!(
        store
            .payout_outcome_progress(&bad.header)
            .await
            .unwrap()
            .through,
        0
    );
    let done = store
        .reconcile_payout_outcome(&packet, &reader)
        .await
        .unwrap();
    assert_eq!(
        (done.spent_sats, done.held_sats, done.released_sats),
        (0, 100, 0)
    );
    let mut tx = store.pool.begin().await.unwrap();
    assert_eq!(
        budget_usage(&mut tx, done.verified_at + 86400 * 30)
            .await
            .unwrap(),
        100
    );
    tx.commit().await.unwrap();
    task.abort();
}

#[path = "failed_costs_tests.rs"]
mod failed_costs_tests;
