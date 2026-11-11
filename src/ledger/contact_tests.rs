use super::*;
use crate::{contact::tests::fixture, domain::payment::SettledPayment};

async fn store() -> (tempfile::TempDir, String, LedgerStore) {
    let dir = tempfile::TempDir::new().unwrap();
    let url = format!("sqlite://{}", dir.path().join("contacts.db").display());
    let ledger = LedgerStore::connect(&url).await.unwrap();
    (dir, url, ledger)
}
fn invoice(intent: &crate::contact::Intent) -> StoredStrikeReceiveRequest {
    StoredStrikeReceiveRequest {
        receive_request_id: Uuid::new_v4(),
        address_user: "contact".into(),
        credit_pool: "contact".into(),
        amount_msat: FEE_MSAT,
        description_hash: intent.description_hash.clone(),
        payment_hash: "ab".repeat(32),
        invoice: crate::test_invoices::invoice_with_payment_hash(
            FEE_MSAT,
            &intent.description_hash,
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap(),
            [0xab; 32],
        ),
        created_provider: None,
    }
}
fn ordinary(receive: Uuid, hash: Option<String>) -> SettledPayment {
    SettledPayment {
        source: "strike".into(),
        source_id: receive.to_string(),
        payment_hash: hash,
        address_user: "herd".into(),
        credit_pool: "herd".into(),
        amount_msat: FEE_MSAT,
        settled_at: None,
        context_json: None,
    }
}
async fn prepared(ledger: &LedgerStore) -> (crate::contact::Intent, StoredStrikeReceiveRequest) {
    let intent = fixture();
    ledger.create_contact_intent(&intent).await.unwrap();
    let claim = ledger
        .claim_contact_issuance(intent.id, &"ab".repeat(32))
        .await
        .unwrap()
        .unwrap();
    let request = invoice(&intent);
    ledger
        .record_contact_invoice(intent.id, claim, &request)
        .await
        .unwrap();
    (intent, request)
}
async fn count(ledger: &LedgerStore, table: &str) -> i64 {
    sqlx::query_scalar(&format!("SELECT COUNT(*) FROM {table}"))
        .fetch_one(&ledger.pool)
        .await
        .unwrap()
}

#[tokio::test]
async fn private_fee_is_atomic_idempotent_and_never_mints_feed_or_public_work() {
    let (_dir, url, ledger) = store().await;
    let (intent, request) = prepared(&ledger).await;
    let receive = Uuid::new_v4();
    assert!(
        ledger
            .accept_contact_fee(
                intent.id,
                request.receive_request_id,
                receive,
                FEE_MSAT - 1000,
                &request.payment_hash
            )
            .await
            .is_err()
    );
    assert!(
        ledger
            .accept_contact_fee(
                intent.id,
                request.receive_request_id,
                receive,
                FEE_MSAT,
                &request.payment_hash
            )
            .await
            .unwrap()
    );
    assert!(
        !ledger
            .accept_contact_fee(
                intent.id,
                request.receive_request_id,
                receive,
                FEE_MSAT,
                &request.payment_hash
            )
            .await
            .unwrap()
    );
    assert!(
        ledger
            .accept_contact_fee(
                intent.id,
                request.receive_request_id,
                Uuid::new_v4(),
                FEE_MSAT,
                &request.payment_hash
            )
            .await
            .is_err()
    );
    assert!(
        ledger
            .record_payment(&ordinary(receive, None))
            .await
            .is_err()
    );
    assert!(
        ledger
            .record_payment(&ordinary(
                Uuid::new_v4(),
                Some(request.payment_hash.to_uppercase())
            ))
            .await
            .is_err()
    );
    assert_eq!(ledger.feed_credit_sats().await.unwrap(), 0);
    for table in [
        "settled_payments",
        "ledger_entries",
        "event_log",
        "reward_funding_lots",
        "zap_receipt_jobs",
        "message_outbox",
    ] {
        assert_eq!(count(&ledger, table).await, 0, "{table}");
    }
    let reopened = LedgerStore::connect(&url).await.unwrap();
    let status = reopened
        .contact_status(intent.id, &"ab".repeat(32))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(status.state, "paid");
    assert_eq!(status.fee_sats, 100);
    assert_eq!(status.invoice, Some(request.invoice));
    assert!(
        reopened
            .contact_status(intent.id, &"cd".repeat(32))
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(count(&reopened, "private_contact_fees").await, 1);
    assert_eq!(count(&reopened, "private_contact_delivery").await, 1);
}

#[tokio::test]
async fn failed_delivery_staging_rolls_back_fee_then_retries_once() {
    let (_dir, _url, ledger) = store().await;
    let (intent, request) = prepared(&ledger).await;
    let receive = Uuid::new_v4();
    sqlx::query("CREATE TRIGGER reject_private_queue BEFORE INSERT ON private_contact_delivery BEGIN SELECT RAISE(ABORT,'fixture queue failure'); END").execute(&ledger.pool).await.unwrap();
    assert!(
        ledger
            .accept_contact_fee(
                intent.id,
                request.receive_request_id,
                receive,
                FEE_MSAT,
                &request.payment_hash
            )
            .await
            .is_err()
    );
    assert_eq!(count(&ledger, "private_contact_fees").await, 0);
    sqlx::query("DROP TRIGGER reject_private_queue")
        .execute(&ledger.pool)
        .await
        .unwrap();
    assert!(
        ledger
            .accept_contact_fee(
                intent.id,
                request.receive_request_id,
                receive,
                FEE_MSAT,
                &request.payment_hash
            )
            .await
            .unwrap()
    );
    assert_eq!(count(&ledger, "private_contact_delivery").await, 1);
}

#[tokio::test]
async fn issuance_claim_is_durable_and_uncertain_creation_is_never_reclaimed() {
    let (_dir, url, ledger) = store().await;
    let intent = fixture();
    ledger.create_contact_intent(&intent).await.unwrap();
    ledger.create_contact_intent(&intent).await.unwrap();
    let other = LedgerStore::connect(&url).await.unwrap();
    let capability = "ab".repeat(32);
    let (a, b) = tokio::join!(
        ledger.claim_contact_issuance(intent.id, &capability),
        other.claim_contact_issuance(intent.id, &capability)
    );
    let a = a.unwrap();
    let b = b.unwrap();
    assert_ne!(a.is_some(), b.is_some());
    let claim = a.or(b).unwrap();
    ledger
        .mark_contact_issuance_uncertain(intent.id, claim)
        .await
        .unwrap();
    assert!(
        other
            .claim_contact_issuance(intent.id, &"ab".repeat(32))
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(
        other
            .contact_status(intent.id, &"ab".repeat(32))
            .await
            .unwrap()
            .unwrap()
            .state,
        "uncertain"
    );
    let request = invoice(&intent);
    assert!(
        ledger
            .record_strike_receive_request(&request)
            .await
            .is_err()
    );
    assert!(
        other
            .record_contact_invoice(intent.id, Uuid::new_v4(), &request)
            .await
            .is_err()
    );
    other
        .record_contact_invoice(intent.id, claim, &request)
        .await
        .unwrap();
    other
        .record_contact_invoice(intent.id, claim, &request)
        .await
        .unwrap();
    assert_eq!(
        other
            .contact_status(intent.id, &"ab".repeat(32))
            .await
            .unwrap()
            .unwrap()
            .invoice,
        Some(request.invoice)
    );
    let mut changed = intent.clone();
    changed.envelope_json.push(' ');
    assert!(other.create_contact_intent(&changed).await.is_err());
    assert!(sqlx::query("UPDATE private_contact_intents SET state='new',issuance_claim=NULL,receive_request_id=NULL").execute(&ledger.pool).await.is_err());
}

#[tokio::test]
async fn lost_ack_and_restart_reuse_exact_ciphertext_and_reject_stale_leases() {
    let (_dir, url, ledger) = store().await;
    let (intent, request) = prepared(&ledger).await;
    ledger
        .accept_contact_fee(
            intent.id,
            request.receive_request_id,
            Uuid::new_v4(),
            FEE_MSAT,
            &request.payment_hash,
        )
        .await
        .unwrap();
    let first = ledger.reserve_contact_delivery().await.unwrap().unwrap();
    assert!(ledger.reserve_contact_delivery().await.unwrap().is_none());
    sqlx::query("UPDATE private_contact_delivery SET lease_until=0")
        .execute(&ledger.pool)
        .await
        .unwrap();
    let other = LedgerStore::connect(&url).await.unwrap();
    let second = other.reserve_contact_delivery().await.unwrap().unwrap();
    assert_eq!(first.envelope_json, second.envelope_json);
    assert_ne!(first.lease_id, second.lease_id);
    assert!(
        ledger
            .finish_contact_delivery(&first, Some(&first.relays[0]))
            .await
            .is_err()
    );
    assert!(
        other
            .finish_contact_delivery(&second, Some("wss://unselected.example.org"))
            .await
            .is_err()
    );
    other.finish_contact_delivery(&second, None).await.unwrap();
    assert!(other.reserve_contact_delivery().await.unwrap().is_none());
    sqlx::query("UPDATE private_contact_delivery SET next_attempt=0")
        .execute(&ledger.pool)
        .await
        .unwrap();
    let third = ledger.reserve_contact_delivery().await.unwrap().unwrap();
    assert_eq!(third.envelope_json, first.envelope_json);
    ledger
        .finish_contact_delivery(&third, Some(&third.relays[0]))
        .await
        .unwrap();
    assert!(other.reserve_contact_delivery().await.unwrap().is_none());
    assert_eq!(
        other
            .contact_status(intent.id, &"ab".repeat(32))
            .await
            .unwrap()
            .unwrap()
            .state,
        "delivered"
    );
    assert_eq!(ledger.feed_credit_sats().await.unwrap(), 0);
}

#[tokio::test]
async fn existing_ordinary_settlement_cannot_be_reclassified_as_a_contact_fee() {
    let (_dir, _url, ledger) = store().await;
    let intent = fixture();
    let request = invoice(&intent);
    ledger
        .record_payment(&ordinary(
            Uuid::new_v4(),
            Some(request.payment_hash.to_uppercase()),
        ))
        .await
        .unwrap();
    ledger.create_contact_intent(&intent).await.unwrap();
    let claim = ledger
        .claim_contact_issuance(intent.id, &"ab".repeat(32))
        .await
        .unwrap()
        .unwrap();
    assert!(
        ledger
            .record_contact_invoice(intent.id, claim, &request)
            .await
            .is_err()
    );
    assert_eq!(ledger.feed_credit_sats().await.unwrap(), 100);
    assert_eq!(count(&ledger, "private_contact_fees").await, 0);
}

#[tokio::test]
async fn native_retention_verifier_refuses_changed_private_commitment_or_missing_delivery() {
    let (_dir, _url, ledger) = store().await;
    let (intent, request) = prepared(&ledger).await;
    ledger
        .accept_contact_fee(
            intent.id,
            request.receive_request_id,
            Uuid::new_v4(),
            FEE_MSAT,
            &request.payment_hash,
        )
        .await
        .unwrap();
    crate::contact_state::verify(&ledger.pool).await.unwrap();
    sqlx::query("DROP TRIGGER private_contact_intent_immutable")
        .execute(&ledger.pool)
        .await
        .unwrap();
    sqlx::query("UPDATE private_contact_intents SET reply_email_hash=?")
        .bind("ff".repeat(32))
        .execute(&ledger.pool)
        .await
        .unwrap();
    assert!(crate::contact_state::verify(&ledger.pool).await.is_err());
    sqlx::query("UPDATE private_contact_intents SET reply_email_hash=?")
        .bind(&intent.reply_email_hash)
        .execute(&ledger.pool)
        .await
        .unwrap();
    crate::contact_state::verify(&ledger.pool).await.unwrap();
    sqlx::query("DROP TRIGGER private_contact_delivery_retained")
        .execute(&ledger.pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM private_contact_delivery")
        .execute(&ledger.pool)
        .await
        .unwrap();
    assert!(crate::contact_state::verify(&ledger.pool).await.is_err());
}
