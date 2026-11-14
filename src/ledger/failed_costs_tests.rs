use super::*;
use crate::payout::cost_recovery::{ApprovedDecision, Decision};
fn decision(packet: &Packet, cost: u64) -> Decision {
    let binding = packet.validate().unwrap().unwrap();
    let now = crate::cyberherd::calendar::now().unwrap();
    Decision {
        version: 1,
        id: Uuid::new_v4(),
        epoch: packet.header.reservation.epoch,
        reservation_id: packet.header.reservation.id,
        owner: packet.header.reservation.owner.clone(),
        member: packet.row.member.clone(),
        account_id: packet.header.scope.account_id,
        manifest_hash: packet.header.final_hash.clone(),
        payment_id: binding.payment_id,
        invoice_hash: binding.quote.request.payment_hash,
        gross_sats: packet.row.gross_sats,
        actual_cost_sats: cost,
        evidence_sha256: "99".repeat(32),
        statement_reference: "Synthetic reviewed statement".into(),
        reason: "Synthetic exact failed-payment fee/refund review".into(),
        reviewed_failed_payment_cost: true,
        approved_at: now,
        expires_at: now + 3600,
    }
}
#[tokio::test]
async fn explicit_failed_cost_reclassifies_only_its_original_hold_and_rolls_cost_from_review_time()
{
    let (_root, store, policy, feeds) = fixture().await;
    let r = reserve(&store, &policy, feeds[0], 100).await;
    let p = report(r, PaymentState::Failed);
    register_claim(&store, &policy, &p).await;
    let (reader, task) = provider(PaymentState::Failed).await;
    let original = store.reconcile_payout_outcome(&p, &reader).await.unwrap();
    assert_eq!(original.held_sats, 100);
    // A review can release only this member's original hold. A separate
    // unresolved reservation remains held even after the rolling window ends.
    let other = reserve(&store, &policy, feeds[1], 100).await;
    let draft = store
        .failed_cost_draft(p.header.reservation.id, &p.row.member)
        .await
        .unwrap();
    assert!(draft["actual_cost_sats"].is_null());
    assert_eq!(draft["reviewed_failed_payment_cost"], false);
    assert!(serde_json::from_value::<Decision>(draft).is_err());
    let d = decision(&p, 3);
    assert!(store.preview_failed_cost(&d).await.unwrap().is_none());
    let receipt = store
        .reconcile_failed_cost(&ApprovedDecision::fixture(d.clone()), &reader)
        .await
        .unwrap();
    assert_eq!(receipt.released_sats(), 97);
    assert_eq!(
        store.payout_outcome_progress(&p.header).await.unwrap(),
        original
    ); // never rewrite verified status/final ACK
    let mut tx = store.pool.begin().await.unwrap();
    assert_eq!(
        budget_usage(&mut tx, receipt.verified_at).await.unwrap(),
        103
    );
    assert_eq!(
        budget_usage(&mut tx, receipt.verified_at + 86400)
            .await
            .unwrap(),
        100
    );
    tx.commit().await.unwrap();
    assert_eq!(other.reserved_sats, 100);
    assert_eq!(store.feed_credit_sats().await.unwrap(), 0);
    assert!(store.next_outbox_entry().await.unwrap().is_none());
    assert_eq!(
        store
            .reconcile_failed_cost(&ApprovedDecision::fixture(d.clone()), &reader)
            .await
            .unwrap(),
        receipt
    );
    assert!(
        sqlx::query("DELETE FROM payout_failed_costs")
            .execute(&store.pool)
            .await
            .is_err()
    );
    let mut changed = d;
    changed.actual_cost_sats = 0;
    assert!(
        store
            .reconcile_failed_cost(&ApprovedDecision::fixture(changed), &reader)
            .await
            .is_err()
    );
    task.abort();
}
#[tokio::test]
async fn failed_cost_wrong_scope_unfinalized_pending_and_expired_approval_keep_funds_held() {
    let (_root, store, policy, feeds) = fixture().await;
    let r = reserve(&store, &policy, feeds[0], 100).await;
    let p = report(r, PaymentState::Failed);
    register_claim(&store, &policy, &p).await;
    let (failed, failed_task) = provider(PaymentState::Failed).await;
    let d = decision(&p, 0);
    assert!(store.preview_failed_cost(&d).await.is_err());
    store.reconcile_payout_outcome(&p, &failed).await.unwrap();
    for mutate in 0..5 {
        let mut wrong = d.clone();
        match mutate {
            0 => wrong.epoch = Uuid::new_v4(),
            1 => wrong.payment_id = Uuid::new_v4(),
            2 => wrong.account_id = Uuid::new_v4(),
            3 => wrong.invoice_hash = "88".repeat(32),
            _ => {
                wrong.approved_at -= 7200;
                wrong.expires_at -= 7200;
            }
        }
        assert!(
            store
                .reconcile_failed_cost(&ApprovedDecision::fixture(wrong), &failed)
                .await
                .is_err()
        );
    }
    let (pending, pending_task) = provider(PaymentState::Pending).await;
    assert!(
        store
            .reconcile_failed_cost(&ApprovedDecision::fixture(d), &pending)
            .await
            .is_err()
    );
    let mut tx = store.pool.begin().await.unwrap();
    assert_eq!(
        budget_usage(
            &mut tx,
            crate::cyberherd::calendar::now().unwrap() + 86400 * 30
        )
        .await
        .unwrap(),
        100
    );
    tx.commit().await.unwrap();
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM payout_failed_costs")
            .fetch_one(&store.pool)
            .await
            .unwrap(),
        0
    );
    failed_task.abort();
    pending_task.abort();
}
#[tokio::test]
async fn failed_cost_final_insert_failure_rolls_back_budget_clock_and_remains_exactly_retryable() {
    let (_root, store, policy, feeds) = fixture().await;
    let r = reserve(&store, &policy, feeds[0], 100).await;
    let p = report(r, PaymentState::Failed);
    register_claim(&store, &policy, &p).await;
    let (reader, task) = provider(PaymentState::Failed).await;
    store.reconcile_payout_outcome(&p, &reader).await.unwrap();
    let approved = ApprovedDecision::fixture(decision(&p, 0));
    sqlx::query("CREATE TRIGGER synthetic_cost_failure BEFORE INSERT ON payout_failed_costs BEGIN SELECT RAISE(ABORT,'fixture'); END").execute(&store.pool).await.unwrap();
    assert!(
        store
            .reconcile_failed_cost(&approved, &reader)
            .await
            .is_err()
    );
    let mut tx = store.pool.begin().await.unwrap();
    assert_eq!(
        budget_usage(&mut tx, crate::cyberherd::calendar::now().unwrap())
            .await
            .unwrap(),
        100
    );
    tx.commit().await.unwrap();
    sqlx::query("DROP TRIGGER synthetic_cost_failure")
        .execute(&store.pool)
        .await
        .unwrap();
    store
        .reconcile_failed_cost(&approved, &reader)
        .await
        .unwrap();
    let mut tx = store.pool.begin().await.unwrap();
    assert_eq!(
        budget_usage(&mut tx, crate::cyberherd::calendar::now().unwrap())
            .await
            .unwrap(),
        0
    );
    tx.commit().await.unwrap();
    task.abort();
}
