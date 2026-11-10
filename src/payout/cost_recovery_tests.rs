use super::*;
use crate::payout::cost_recovery::{ApprovedDecision, Decision};
fn decision(packet: &Packet, cost: u64) -> Decision {
    let binding = packet.validate().unwrap().unwrap();
    let header = &packet.header;
    let now = now();
    Decision {
        version: 1,
        id: Uuid::new_v4(),
        epoch: header.reservation.epoch,
        reservation_id: header.reservation.id,
        owner: header.reservation.owner.clone(),
        member: packet.row.member.clone(),
        account_id: header.scope.account_id,
        manifest_hash: header.final_hash.clone(),
        payment_id: binding.payment_id,
        invoice_hash: binding.quote.request.payment_hash,
        gross_sats: packet.row.gross_sats,
        actual_cost_sats: cost,
        evidence_sha256: "99".repeat(32),
        statement_reference: "Synthetic statement".into(),
        reason: "Reviewed synthetic final debit".into(),
        reviewed_failed_payment_cost: true,
        approved_at: now,
        expires_at: now + 3600,
    }
}
#[tokio::test]
async fn failed_costs_require_core_receipt_and_survive_both_ack_loss_and_payout_restart_without_new_payment()
 {
    let w = Workflow::new(false).await;
    let bound = execute(&w, PaymentState::Failed).await;
    let header = w
        .store
        .freeze_outcomes(w.f.reservation.id, now())
        .await
        .unwrap();
    let packet = w
        .store
        .outcome_packet(header.reservation.id, 0)
        .await
        .unwrap();
    let (core, task) = bridge(&w, true).await;
    let ack = core.payout_outcome(&packet).await.unwrap();
    w.store
        .acknowledge_outcomes(header.reservation.id, &ack, now())
        .await
        .unwrap();
    let member = &packet.row.member;
    assert!(
        w.store
            .sync_failed_cost(&core, header.reservation.id, member)
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(w.store.held_sats().await.unwrap(), 2);
    let d = decision(&packet, 1);
    let approved = ApprovedDecision::fixture(d);
    let before_events = w.f.ledger.events_after(0, 100).await.unwrap();
    let receipt =
        w.f.ledger
            .reconcile_failed_cost(&approved, &reader(&w))
            .await
            .unwrap();
    let reads = w.mock.lock().unwrap().get_ids.len();
    assert_eq!(
        w.f.ledger
            .reconcile_failed_cost(&approved, &reader(&w))
            .await
            .unwrap(),
        receipt
    );
    assert_eq!(w.mock.lock().unwrap().get_ids.len(), reads);
    assert_eq!(
        w.f.ledger.events_after(0, 100).await.unwrap(),
        before_events
    );
    assert!(w.f.ledger.next_outbox_entry().await.unwrap().is_none());
    let original = w.f.ledger.payout_outcome_progress(&header).await.unwrap();
    assert_eq!(original.held_sats, 2);
    let db = sqlx::SqlitePool::connect(&w.url).await.unwrap();
    sqlx::query("CREATE TRIGGER lost_cost_ack BEFORE INSERT ON payout_cost_acks BEGIN SELECT RAISE(ABORT,'fixture'); END").execute(&db).await.unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(600)).await;
    assert!(
        w.store
            .sync_failed_cost(&core, header.reservation.id, member)
            .await
            .is_err()
    );
    assert_eq!(w.store.held_sats().await.unwrap(), 2);
    sqlx::query("DROP TRIGGER lost_cost_ack")
        .execute(&db)
        .await
        .unwrap();
    let reopened = Store::connect(&w.url, w.policy.clone()).await.unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(600)).await;
    assert_eq!(
        reopened
            .sync_failed_cost(&core, header.reservation.id, member)
            .await
            .unwrap(),
        Some(receipt.clone())
    );
    assert_eq!(reopened.held_sats().await.unwrap(), 0);
    assert_eq!(reopened.budget_usage_sats().await.unwrap(), 1);
    tokio::time::sleep(std::time::Duration::from_millis(600)).await;
    assert_eq!(
        reopened
            .sync_failed_cost(&core, header.reservation.id, member)
            .await
            .unwrap(),
        Some(receipt)
    );
    assert_eq!(reopened.budget_usage_sats().await.unwrap(), 1);
    assert!(
        sqlx::query("DELETE FROM payout_cost_acks")
            .execute(&db)
            .await
            .is_err()
    );
    {
        let mock = w.mock.lock().unwrap();
        assert_eq!(mock.post_keys.len(), 1);
        assert_eq!(mock.patch_ids.len(), 1);
        assert_eq!(mock.payments.len(), 1);
        assert_eq!(*mock.payments.values().next().unwrap(), bound.payment_id());
    }
    task.abort();
    let (intake, task) = bridge(&w, false).await;
    assert!(intake.failed_cost(&header, member).await.is_err());
    task.abort();
}
