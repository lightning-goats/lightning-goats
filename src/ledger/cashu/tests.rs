use super::*;
use crate::cashu::receipt_authority::{ImportLimits, MintImportLimit, fixture_authenticate};
use crate::domain::payment::SettledPayment;
use lightning_goats_cashu_receipt::ReceiptData;
use std::collections::BTreeMap;
use uuid::Uuid;

async fn fixture() -> (tempfile::TempDir, String, LedgerStore, ReceiptPolicy) {
    let dir = tempfile::TempDir::new().unwrap();
    let url = format!("sqlite://{}", dir.path().join("cashu-core.db").display());
    let store = LedgerStore::connect(&url).await.unwrap();
    store.initialize_reward_funding().await.unwrap();
    store.configure_feeder_credit_cap(1000).await.unwrap();
    let policy = ReceiptPolicy {
        wallet_uid: 60001,
        wallet_instance: Uuid::new_v4(),
        wallet_epoch: Uuid::new_v4(),
        core_epoch: store.overlay_stream_id().await.unwrap(),
        import_enabled: true,
        limits: ImportLimits {
            receipts_per_24h: 10,
            unlimited_receipts_per_24h: false,
            credit_sats_per_24h: 20000,
            maximum_custody_sats: 20000,
        },
        approved: BTreeMap::from([(
            ("mint-a".into(), "v1".into()),
            MintImportLimit {
                maximum_gross_sats: 10000,
                maximum_net_sats: 10000,
            },
        )]),
    };
    (dir, url, store, policy)
}
fn wire(policy: &ReceiptPolicy, net: u64) -> WireReceipt {
    let id = Uuid::new_v4();
    WireReceipt::validate(ReceiptData {
        schema_version: 1,
        wallet_instance: policy.wallet_instance,
        wallet_epoch: policy.wallet_epoch,
        acquisition_id: id,
        transport: "cashu".into(),
        source_ref: "reusable-descriptor".into(),
        source_author: None,
        mint_id: "mint-a".into(),
        unit: "sat".into(),
        gross_sats: net + 1,
        receive_fee_sats: 1,
        net_sats: net,
        acquisition_evidence_digest: format!("{}{}", id.simple(), id.simple()),
        policy_version: "v1".into(),
        credit_pool: "herd".into(),
    })
    .unwrap()
}
fn authenticated(policy: &ReceiptPolicy, receipt: &WireReceipt) -> PeerReceipt {
    fixture_authenticate(policy, &receipt.encode().unwrap()).unwrap()
}
async fn count(store: &LedgerStore, table: &str) -> i64 {
    sqlx::query_scalar(&format!("SELECT COUNT(*) FROM {table}"))
        .fetch_one(&store.pool)
        .await
        .unwrap()
}
async fn lightning(store: &LedgerStore, amount: u64) -> String {
    let id = Uuid::new_v4().to_string();
    store
        .record_payment(&SettledPayment {
            source: "strike".into(),
            source_id: id.clone(),
            payment_hash: None,
            address_user: "dexter".into(),
            credit_pool: "herd".into(),
            amount_msat: amount * 1000,
            settled_at: Some(1700000000),
            context_json: None,
        })
        .await
        .unwrap();
    id
}

#[tokio::test]
async fn one_private_receipt_one_capped_grant_and_exact_ack_after_retirement_restart_and_disable() {
    let (_dir, url, store, mut policy) = fixture().await;
    let receipt = wire(&policy, 9000);
    let imported = store
        .import_cashu_receipt(&policy, &authenticated(&policy, &receipt))
        .await
        .unwrap();
    assert_eq!(imported.net_sats, 9000);
    assert_eq!(imported.original_feed_credit_sats, 3000);
    assert_eq!(count(&store, "cashu_receipt_inbox").await, 1);
    assert_eq!(count(&store, "settled_payments").await, 0);
    assert_eq!(count(&store, "credit_grants").await, 0);
    assert_eq!(count(&store, "feed_credit_retirements").await, 1);
    assert!(store.begin_feed_attempt(1000).await.unwrap().is_none());
    assert_eq!(count(&store, "feed_attempts").await, 0);
    let ledger: i64 = sqlx::query_scalar("SELECT ledger_entry_id FROM cashu_receipt_inbox")
        .fetch_one(&store.pool)
        .await
        .unwrap();
    assert!(
        store
            .retire_existing_feed_credit("test-retirement", ledger, 3000, false)
            .await
            .is_err()
    );
    let plan: String =
        sqlx::query_scalar("SELECT id FROM feed_financial_plans WHERE state='pending'")
            .fetch_one(&store.pool)
            .await
            .unwrap();
    let plan = Uuid::parse_str(&plan).unwrap();
    let before_credit = store.feed_credit_sats().await.unwrap();
    let before_events = count(&store, "event_log").await;
    let before_jobs = count(&store, "cashu_treasury_intents").await;
    assert!(
        store
            .preview_financial_feed_plan_release(plan, 1000)
            .await
            .unwrap()
    );
    assert_eq!(store.feed_credit_sats().await.unwrap(), before_credit);
    assert_eq!(count(&store, "event_log").await, before_events);
    assert_eq!(count(&store, "cashu_treasury_intents").await, before_jobs);
    let retained: String = sqlx::query_scalar("SELECT state FROM feed_financial_plans WHERE id=?")
        .bind(plan.to_string())
        .fetch_one(&store.pool)
        .await
        .unwrap();
    assert_eq!(retained, "pending");
    assert!(store.release_financial_feed_plan(plan, 999).await.is_err());
    assert!(store.release_financial_feed_plan(plan, 1000).await.unwrap());
    assert!(!store.release_financial_feed_plan(plan, 1000).await.unwrap());
    store
        .retire_existing_feed_credit("test-retirement", ledger, 3000, false)
        .await
        .unwrap();
    drop(store);
    let store = LedgerStore::connect(&url).await.unwrap();
    policy.import_enabled = false;
    policy.approved.clear();
    let retry = store
        .import_cashu_receipt(&policy, &authenticated(&policy, &receipt))
        .await
        .unwrap();
    assert_eq!(retry, imported);
    assert_eq!(store.feed_credit_sats().await.unwrap(), 0);
    assert_eq!(count(&store, "cashu_receipt_inbox").await, 1);
    let events = store.events_after(0, 100).await.unwrap();
    let payment = events
        .iter()
        .filter(|e| e.event_type == "payment_received")
        .collect::<Vec<_>>();
    assert_eq!(payment.len(), 1);
    let public = &payment[0].payload_json;
    for private in [
        "proof",
        "secret",
        "mint-a",
        "wallet",
        "acquisition",
        "evidence",
        "reusable-descriptor",
    ] {
        assert!(!public.contains(private));
    }
    let native_payout: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM reward_funding_lots WHERE kind='strike_btc'")
            .fetch_one(&store.pool)
            .await
            .unwrap();
    assert_eq!(native_payout, 0);
}

#[tokio::test]
async fn concurrent_ack_loss_and_conflicting_replay_never_reset_credit_or_caps() {
    let (_dir, url, first, policy) = fixture().await;
    let second = LedgerStore::connect(&url).await.unwrap();
    let receipt = wire(&policy, 250);
    let a = authenticated(&policy, &receipt);
    let b = authenticated(&policy, &receipt);
    let (one, two) = tokio::join!(
        first.import_cashu_receipt(&policy, &a),
        second.import_cashu_receipt(&policy, &b)
    );
    assert_eq!(one.unwrap(), two.unwrap());
    assert_eq!(first.feed_credit_sats().await.unwrap(), 250);
    let mut changed = receipt.data().clone();
    changed.net_sats = 251;
    changed.gross_sats = 252;
    let changed = WireReceipt::validate(changed).unwrap();
    assert!(
        first
            .import_cashu_receipt(&policy, &authenticated(&policy, &changed))
            .await
            .is_err()
    );
    let mut rewrapped = receipt.data().clone();
    rewrapped.acquisition_id = Uuid::new_v4();
    let rewrapped = WireReceipt::validate(rewrapped).unwrap();
    assert!(
        first
            .import_cashu_receipt(&policy, &authenticated(&policy, &rewrapped))
            .await
            .is_err()
    );
    assert_eq!(count(&first, "cashu_receipt_inbox").await, 1);
    assert_eq!(count(&first, "ledger_entries").await, 1);
    assert_eq!(count(&first, "event_log").await, 1);
}

#[tokio::test]
async fn held_earlier_cashu_does_not_block_later_native_sources_or_reward_provenance() {
    let (_dir, _url, store, policy) = fixture().await;
    let receipt = wire(&policy, 600);
    store
        .import_cashu_receipt(&policy, &authenticated(&policy, &receipt))
        .await
        .unwrap();
    lightning(&store, 400).await;
    assert_eq!(store.feed_credit_sats().await.unwrap(), 1000);
    assert!(store.begin_feed_attempt(1000).await.unwrap().is_none());
    assert_eq!(count(&store, "feed_attempts").await, 0);
    let original: String =
        sqlx::query_scalar("SELECT id FROM feed_financial_plans WHERE state='pending'")
            .fetch_one(&store.pool)
            .await
            .unwrap();
    let original_parts: Vec<(i64, i64)> = sqlx::query_as(
        "SELECT ledger_id,sats FROM feed_financial_parts WHERE plan_id=? ORDER BY ledger_id",
    )
    .bind(&original)
    .fetch_all(&store.pool)
    .await
    .unwrap();
    assert_eq!(original_parts.iter().map(|p| p.1).sum::<i64>(), 1000);
    for (ledger, sats) in &original_parts {
        assert!(
            store
                .retire_existing_feed_credit("reserved-source", *ledger, *sats as u64, true)
                .await
                .is_err()
        );
    }
    let reopened = LedgerStore::connect(&_url).await.unwrap();
    assert!(reopened.begin_feed_attempt(1000).await.unwrap().is_none());
    assert_eq!(count(&reopened, "feed_financial_plans").await, 1);
    assert_eq!(count(&store, "cashu_treasury_intents").await, 0);
    lightning(&store, 600).await;
    sqlx::raw_sql("CREATE TRIGGER refuse_physical_parts BEFORE INSERT ON feed_source_parts BEGIN SELECT RAISE(ABORT,'fixture dispatch boundary'); END;")
        .execute(&store.pool).await.unwrap();
    assert!(store.begin_feed_attempt(1000).await.is_err());
    assert_eq!(count(&store, "feed_attempts").await, 0);
    let retained: String =
        sqlx::query_scalar("SELECT id FROM feed_financial_plans WHERE state='pending'")
            .fetch_one(&store.pool)
            .await
            .unwrap();
    assert_eq!(retained, original);
    sqlx::raw_sql("DROP TRIGGER refuse_physical_parts")
        .execute(&store.pool)
        .await
        .unwrap();
    let request = store.begin_feed_attempt(1000).await.unwrap().unwrap();
    let disposition: String = sqlx::query_scalar(
        "SELECT disposition FROM feed_financial_plans WHERE id=? AND state='released'",
    )
    .bind(&original)
    .fetch_one(&store.pool)
    .await
    .unwrap();
    assert_eq!(disposition, "settled_supersession");
    let replacement: String = sqlx::query_scalar(
        "SELECT replacement_id FROM feed_financial_supersessions WHERE original_id=?",
    )
    .bind(&original)
    .fetch_one(&store.pool)
    .await
    .unwrap();
    let linked_request: String = sqlx::query_scalar(
        "SELECT request_id FROM feed_financial_plans WHERE id=? AND state='dispatched'",
    )
    .bind(&replacement)
    .fetch_one(&store.pool)
    .await
    .unwrap();
    assert_eq!(linked_request, request.to_string());
    assert!(
        store
            .release_financial_feed_plan(Uuid::parse_str(&replacement).unwrap(), 1000)
            .await
            .is_err()
    );

    let retained_parts: Vec<(i64, i64)> = sqlx::query_as(
        "SELECT ledger_id,sats FROM feed_financial_parts WHERE plan_id=? ORDER BY ledger_id",
    )
    .bind(&original)
    .fetch_all(&store.pool)
    .await
    .unwrap();
    assert_eq!(retained_parts, original_parts);
    let financial_parts: Vec<(i64,i64)> = sqlx::query_as("SELECT p.ledger_id,p.sats FROM feed_financial_parts p JOIN feed_financial_plans f ON f.id=p.plan_id WHERE f.request_id=? ORDER BY p.ledger_id")
        .bind(request.to_string()).fetch_all(&store.pool).await.unwrap();
    let physical_parts: Vec<(i64, i64)> = sqlx::query_as(
        "SELECT ledger_id,sats FROM feed_source_parts WHERE request_id=? ORDER BY ledger_id",
    )
    .bind(request.to_string())
    .fetch_all(&store.pool)
    .await
    .unwrap();
    assert_eq!(financial_parts, physical_parts);
    let frozen:i64=sqlx::query_scalar("SELECT COALESCE(SUM(p.sats),0) FROM feed_source_parts p JOIN cashu_receipt_inbox c ON c.ledger_entry_id=p.ledger_id WHERE request_id=?")
        .bind(request.to_string()).fetch_one(&store.pool).await.unwrap();
    assert_eq!(frozen, 0);
    // Later arrivals cannot replace the dispatch's immutable sources/attribution.
    let later = lightning(&store, 100).await;
    store.confirm_feed_attempt(request).await.unwrap();
    assert_eq!(store.feed_credit_sats().await.unwrap(), 700);
    let funding = super::super::inspect_confirmed_reward_funding(
        &mut store.pool.begin().await.unwrap(),
        request,
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(funding.strike_sats, 1000);
    let cashu_remaining:i64=sqlx::query_scalar("SELECT l.remaining_sats FROM reward_funding_lots l JOIN cashu_receipt_inbox c ON c.ledger_entry_id=l.ledger_id")
        .fetch_one(&store.pool).await.unwrap();
    assert_eq!(cashu_remaining, 600);
    let selected_later:i64=sqlx::query_scalar("SELECT COUNT(*) FROM feed_reward_funding_parts p JOIN ledger_entries l ON l.id=p.ledger_id WHERE p.request_id=? AND l.payment_source_id=?")
        .bind(request.to_string()).bind(later).fetch_one(&store.pool).await.unwrap();
    assert_eq!(selected_later, 0);
    assert!(store.begin_feed_attempt(1000).await.unwrap().is_none());
}

#[tokio::test]
async fn late_failure_budget_clock_and_epoch_fences_preserve_native_paid_history() {
    let (_dir, _url, store, mut policy) = fixture().await;
    lightning(&store, 600).await;
    let receipt = wire(&policy, 250);
    sqlx::query("CREATE TRIGGER fail_cashu BEFORE INSERT ON cashu_receipt_inbox BEGIN SELECT RAISE(ABORT,'synthetic late failure'); END").execute(&store.pool).await.unwrap();
    assert!(
        store
            .import_cashu_receipt(&policy, &authenticated(&policy, &receipt))
            .await
            .is_err()
    );
    assert_eq!(store.feed_credit_sats().await.unwrap(), 600);
    assert_eq!(count(&store, "event_log").await, 1);
    assert_eq!(count(&store, "cashu_import_clock").await, 0);
    sqlx::query("DROP TRIGGER fail_cashu")
        .execute(&store.pool)
        .await
        .unwrap();
    policy.limits.maximum_custody_sats = 249;
    assert!(
        store
            .import_cashu_receipt(&policy, &authenticated(&policy, &receipt))
            .await
            .is_err()
    );
    policy.limits.maximum_custody_sats = 250;
    store
        .import_cashu_receipt(&policy, &authenticated(&policy, &receipt))
        .await
        .unwrap();
    let next = wire(&policy, 1);
    assert!(
        store
            .import_cashu_receipt(&policy, &authenticated(&policy, &next))
            .await
            .is_err()
    );
    policy.limits.maximum_custody_sats = 20000;
    sqlx::query("UPDATE cashu_import_clock SET last_seen=last_seen+3600")
        .execute(&store.pool)
        .await
        .unwrap();
    assert!(
        store
            .import_cashu_receipt(&policy, &authenticated(&policy, &next))
            .await
            .is_err()
    );
    store.reset_overlay_stream().await.unwrap();
    assert!(
        store
            .import_cashu_receipt(&policy, &authenticated(&policy, &receipt))
            .await
            .is_err()
    );
    assert_eq!(store.feed_credit_sats().await.unwrap(), 850);
    assert_eq!(count(&store, "settled_payments").await, 1);
    assert_eq!(count(&store, "cashu_receipt_inbox").await, 1);
}

#[tokio::test]
async fn all_three_rails_share_credit_but_ecash_does_not_invent_strike_or_membership_evidence() {
    let (_dir, _url, store, policy) = fixture().await;
    lightning(&store, 600).await;
    let mut nutzap = wire(&policy, 250).data().clone();
    nutzap.transport = "nutzap".into();
    nutzap.source_ref = "19".repeat(32);
    nutzap.source_author =
        Some("79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798".into());
    let nutzap = WireReceipt::validate(nutzap).unwrap();
    let cashu = wire(&policy, 150);
    for receipt in [&nutzap, &cashu] {
        let auth = authenticated(&policy, receipt);
        store.import_cashu_receipt(&policy, &auth).await.unwrap();
    }
    assert_eq!(store.feed_credit_sats().await.unwrap(), 1000);
    assert_eq!(count(&store, "settled_payments").await, 1);
    assert_eq!(count(&store, "cashu_receipt_inbox").await, 2);
    assert_eq!(count(&store, "zap_receipt_jobs").await, 0);
    assert_eq!(count(&store, "event_log").await, 3);
    assert!(store.begin_feed_attempt(1000).await.unwrap().is_none());
    assert_eq!(count(&store, "feed_attempts").await, 0);
    let page = store
        .core_event_batch(Some(policy.core_epoch), 0, 10)
        .await
        .unwrap();
    assert_eq!(
        page.events
            .iter()
            .filter(|e| matches!(e.body, crate::core_bridge::EventBody::Payment(_)))
            .count(),
        1
    );
    assert_eq!(
        page.events
            .iter()
            .filter(|e| matches!(e.body, crate::core_bridge::EventBody::Other))
            .count(),
        2
    );
    let snapshot = store.overlay_snapshot_message(1000).await.unwrap();
    let value: serde_json::Value = serde_json::from_str(&snapshot.1).unwrap();
    assert_eq!(value["feed_credit_sats"], 1000);
    assert_eq!(value["unresolved_feed_attempt"], serde_json::Value::Null);
    // Replaying both transports creates neither another credit nor another cap.
    for receipt in [&nutzap, &cashu] {
        store
            .import_cashu_receipt(&policy, &authenticated(&policy, receipt))
            .await
            .unwrap();
    }
    assert_eq!(store.feed_credit_sats().await.unwrap(), 1000);
    assert_eq!(count(&store, "event_log").await, 3);
}

#[tokio::test]
async fn core_rate_budgets_serialize_distinct_acquisitions_and_zero_defaults_deny_new_work() {
    let (_dir, url, first, mut policy) = fixture().await;
    let second = LedgerStore::connect(&url).await.unwrap();
    policy.limits.receipts_per_24h = 1;
    let one = wire(&policy, 200);
    let two = wire(&policy, 200);
    let a = authenticated(&policy, &one);
    let b = authenticated(&policy, &two);
    let (a, b) = tokio::join!(
        first.import_cashu_receipt(&policy, &a),
        second.import_cashu_receipt(&policy, &b)
    );
    assert_eq!(usize::from(a.is_ok()) + usize::from(b.is_ok()), 1);
    assert_eq!(first.feed_credit_sats().await.unwrap(), 200);
    assert_eq!(count(&first, "cashu_receipt_inbox").await, 1);
    policy.limits.receipts_per_24h = 10;
    policy.limits.credit_sats_per_24h = 399;
    let next = wire(&policy, 200);
    assert!(
        first
            .import_cashu_receipt(&policy, &authenticated(&policy, &next))
            .await
            .is_err()
    );
    policy.limits = ImportLimits::default();
    assert!(
        first
            .import_cashu_receipt(&policy, &authenticated(&policy, &next))
            .await
            .is_err()
    );
    assert_eq!(first.feed_credit_sats().await.unwrap(), 200);
}

#[tokio::test]
async fn explicit_unlimited_receipt_count_preserves_exposure_limits_and_exact_replays() {
    let (_dir, _url, store, mut policy) = fixture().await;
    policy.limits.receipts_per_24h = 1;
    policy.limits.credit_sats_per_24h = 600;
    policy.limits.maximum_custody_sats = 400;
    let first = wire(&policy, 200);
    let second = wire(&policy, 200);
    let ack = store
        .import_cashu_receipt(&policy, &authenticated(&policy, &first))
        .await
        .unwrap();
    assert!(
        store
            .import_cashu_receipt(&policy, &authenticated(&policy, &second))
            .await
            .is_err()
    );
    policy.limits.receipts_per_24h = 0;
    policy.limits.unlimited_receipts_per_24h = true;
    store
        .import_cashu_receipt(&policy, &authenticated(&policy, &second))
        .await
        .unwrap();
    let third = wire(&policy, 200);
    assert!(
        store
            .import_cashu_receipt(&policy, &authenticated(&policy, &third))
            .await
            .is_err()
    );
    let replay = store
        .import_cashu_receipt(&policy, &authenticated(&policy, &first))
        .await
        .unwrap();
    assert_eq!(replay, ack);
    assert_eq!(store.feed_credit_sats().await.unwrap(), 400);
    assert_eq!(count(&store, "cashu_receipt_inbox").await, 2);
}

#[tokio::test]
async fn receipt_credit_and_replay_accept_the_original_advisory_feeding_snapshot() {
    use crate::{config::RuntimeMode, gateway::FeederSafety};

    let (_dir, url, store, policy) = fixture().await;
    store
        .update_feeding_presentation(1000, RuntimeMode::Shadow, None)
        .await
        .unwrap();
    let wire = wire(&policy, 200);
    let receipt = authenticated(&policy, &wire);
    let original = store.import_cashu_receipt(&policy, &receipt).await.unwrap();
    let event = store.events_after(0, 1).await.unwrap().remove(0);
    let payload: serde_json::Value = serde_json::from_str(&event.payload_json).unwrap();
    assert_eq!(payload["feeding"]["reason"], "mode_disabled");
    drop(store);

    let reopened = LedgerStore::connect(&url).await.unwrap();
    reopened
        .update_feeding_presentation(
            1000,
            RuntimeMode::Canary,
            Some(FeederSafety {
                remote_enabled: true,
                override_enabled: false,
            }),
        )
        .await
        .unwrap();
    let replay = reopened
        .import_cashu_receipt(&policy, &receipt)
        .await
        .unwrap();
    assert_eq!(replay, original);
    assert_eq!(
        reopened.events_after(0, 1).await.unwrap()[0].payload_json,
        event.payload_json
    );
    assert_eq!(reopened.feed_credit_sats().await.unwrap(), 200);
    assert_eq!(count(&reopened, "event_log").await, 1);
    assert_eq!(count(&reopened, "cashu_receipt_inbox").await, 1);

    let mut edited = payload;
    edited["amount_sats"] = serde_json::json!(201);
    sqlx::query("UPDATE event_log SET payload_json=?")
        .bind(edited.to_string())
        .execute(&reopened.pool)
        .await
        .unwrap();
    assert!(
        reopened
            .import_cashu_receipt(&policy, &receipt)
            .await
            .is_err()
    );
    assert_eq!(reopened.feed_credit_sats().await.unwrap(), 200);
}

include!("financial_jobs_tests.rs");

include!("sweep_tests.rs");
