//! Temporary databases, actual kernel UID/socket checks and authenticated HTTP
//! with different synthetic core-read and payout keys. No live service touched.
use super::*;
use crate::payout::outcome::{
    Disposition, Header, Packet, Reader, Row, UnsubmittedReason, next_hash,
};

fn reader(w: &Workflow) -> Reader {
    Reader::new(
        StrikeClient::new(&w.origin, "synthetic-core-read-key".into()).unwrap(),
        w.policy.scope.provider.account_id,
    )
    .unwrap()
}

#[tokio::test]
async fn retained_core_claim_rejects_rolled_back_no_execute_and_omitted_member_reports() {
    let w = Workflow::new(false).await;
    let quote = w.quote().await;
    let execute = w
        .store
        .prepare_execute(quote.id(), &w.fresh(true, false).await, now())
        .await
        .unwrap();
    // Even if PATCH never returns, its core claim is an enduring obligation.
    let claim = w.execution_lease(&execute).await;
    let mut row = Row {
        member: w.f.plan.rows[0].pubkey.clone(),
        gross_sats: 2,
        disposition: Disposition::Unsubmitted {
            reason: UnsubmittedReason::Expired,
        },
    };
    for omit in [false, true] {
        if omit {
            row.member = crate::nostr_event::test_event(9, 0, 100, "", vec![]).pubkey;
        }
        let mut header = Header {
            reservation: w.f.reservation.clone(),
            scope: w.policy.scope.provider.clone(),
            rows: 1,
            final_hash: "00".repeat(32),
        };
        let seed = header.seed().unwrap();
        header.final_hash = next_hash(&seed, &row).unwrap();
        let packet = Packet {
            header: header.clone(),
            after: 0,
            before_hash: seed,
            row: row.clone(),
        };
        assert!(
            w.f.ledger
                .reconcile_payout_outcome(&packet, &reader(&w))
                .await
                .is_err()
        );
        assert_eq!(
            w.f.ledger
                .payout_outcome_progress(&header)
                .await
                .unwrap()
                .through,
            0
        );
    }
    assert!(w.mock.lock().unwrap().get_ids.is_empty());
    assert!(w.mock.lock().unwrap().patch_ids.is_empty());
    assert_eq!(w.store.held_sats().await.unwrap(), 2);
    assert_eq!(
        w.execution_lease(&execute).await.lease().record,
        claim.lease().record
    );
}
async fn execute(w: &Workflow, state: PaymentState) -> crate::payout::provider::BoundPayment {
    let intent = w.quote().await;
    let permit = w
        .store
        .prepare_execute(intent.id(), &w.fresh(true, false).await, now())
        .await
        .unwrap();
    let response = w.execute(&permit).await.unwrap();
    let bound = w.store.bind_payment(&response, now()).await.unwrap();
    w.mock.lock().unwrap().state = state;
    w.store
        .observe_payment(&w.sender.read_payment(&bound).await.unwrap(), now())
        .await
        .unwrap();
    bound
}
async fn bridge(
    w: &Workflow,
    payout_role: bool,
) -> (Client, tokio::task::JoinHandle<anyhow::Result<()>>) {
    let (uid, gid) = self_credentials().unwrap();
    let config = BridgeConfig {
        socket: w.f.root.path().join(if payout_role {
            "outcome.sock"
        } else {
            "intake.sock"
        }),
        peer_uid: if payout_role { uid + 1 } else { uid },
        group_gid: gid,
        reward_policy: None,
        payout_peer_uid: if payout_role {
            Some(uid)
        } else {
            Some(uid + 1)
        },
        payout_execution_enabled: false,
        payout_account_id: Some(w.policy.scope.provider.account_id),
    };
    let server = Server::bind(&config).await.unwrap();
    let task = tokio::spawn(server.run_with_reader(
        w.f.ledger.clone(),
        RuntimeMode::Shadow,
        Some(Arc::new(reader(w))),
    ));
    (Client::new(config.socket, uid).unwrap(), task)
}

#[tokio::test]
async fn verified_completed_cost_releases_only_unused_reservation_after_core_ack_and_restart() {
    let w = Workflow::new(false).await;
    // Actual provider cost is one sat below the frozen worst-case fee budget.
    w.mock.lock().unwrap().payment_money =
        Some(json!({"totalAmount":{"amount":"0.00000001","currency":"BTC"}}));
    execute(&w, PaymentState::Completed).await;
    let h = w
        .store
        .freeze_outcomes(w.f.reservation.id, now())
        .await
        .unwrap();
    assert_eq!(w.store.held_sats().await.unwrap(), 2);
    let packet = w.store.outcome_packet(h.reservation.id, 0).await.unwrap();
    let (client, task) = bridge(&w, true).await;
    // Protected payout journal claims Completed, but core independently sees
    // Pending: neither the reservation nor a partial row can be released.
    w.mock.lock().unwrap().state = PaymentState::Pending;
    assert!(client.payout_outcome(&packet).await.is_err());
    assert_eq!(
        w.f.ledger
            .payout_outcome_progress(&h)
            .await
            .unwrap()
            .through,
        0
    );
    w.mock.lock().unwrap().state = PaymentState::Completed;
    let ack = client.payout_outcome(&packet).await.unwrap();
    assert_eq!(
        (
            ack.progress().spent_sats,
            ack.progress().held_sats,
            ack.progress().released_sats
        ),
        (1, 0, 1)
    );
    assert!(!w.f.page(0, true).await.execution_enabled);
    let gets = w.mock.lock().unwrap().get_ids.len();
    let repeated = client.payout_outcome(&packet).await.unwrap();
    assert_eq!(ack.progress(), repeated.progress());
    assert_eq!(w.mock.lock().unwrap().get_ids.len(), gets); // no second provider read
    let restored = Store::connect(&w.url, w.policy.clone()).await.unwrap();
    assert_eq!(restored.held_sats().await.unwrap(), 2); // lost ACK stays held
    restored
        .acknowledge_outcomes(h.reservation.id, &ack, now())
        .await
        .unwrap();
    restored
        .acknowledge_outcomes(h.reservation.id, &ack, now())
        .await
        .unwrap();
    assert_eq!(restored.held_sats().await.unwrap(), 0);
    let allowance =
        w.f.ledger
            .reward_allowance(&w.f.policy, h.reservation.epoch, h.reservation.request_id)
            .await
            .unwrap();
    assert_eq!(allowance.available_sats, 999);
    let db = sqlx::SqlitePool::connect(&w.url).await.unwrap();
    let recorded: i64 = sqlx::query_scalar("SELECT spent_sats FROM payout_outcome_acks")
        .fetch_one(&db)
        .await
        .unwrap();
    assert_eq!(recorded, 1);
    drop(db);
    task.abort();
}

#[tokio::test]
async fn failed_provider_result_keeps_costs_held_and_never_claims_delivery() {
    let w = Workflow::new(false).await;
    execute(&w, PaymentState::Failed).await;
    let h = w
        .store
        .freeze_outcomes(w.f.reservation.id, now())
        .await
        .unwrap();
    let packet = w.store.outcome_packet(h.reservation.id, 0).await.unwrap();
    let (client, task) = bridge(&w, true).await;
    let ack = client.payout_outcome(&packet).await.unwrap();
    assert_eq!(
        (
            ack.progress().spent_sats,
            ack.progress().held_sats,
            ack.progress().released_sats
        ),
        (0, 2, 0)
    );
    w.store
        .acknowledge_outcomes(h.reservation.id, &ack, now())
        .await
        .unwrap();
    assert_eq!(w.store.held_sats().await.unwrap(), 2);
    let db = sqlx::SqlitePool::connect(&w.f.database).await.unwrap();
    let used: i64 = sqlx::query_scalar("SELECT held_sats FROM reward_outcome_finals")
        .fetch_one(&db)
        .await
        .unwrap();
    assert_eq!(used, 2);
    assert_eq!(w.f.ledger.feed_credit_sats().await.unwrap(), 0);
    task.abort();
}

#[tokio::test]
async fn never_submitted_closure_is_permanent_and_payout_role_alone_can_release_it() {
    let w = Workflow::new(false).await;
    let member = &w.f.plan.rows[0].pubkey;
    assert!(
        w.store
            .freeze_outcomes(w.f.reservation.id, now())
            .await
            .is_err()
    );
    w.store
        .close_unsubmitted(
            w.f.reservation.id,
            member,
            UnsubmittedReason::Unavailable,
            now(),
        )
        .await
        .unwrap();
    w.store
        .close_unsubmitted(
            w.f.reservation.id,
            member,
            UnsubmittedReason::Unavailable,
            now(),
        )
        .await
        .unwrap();
    assert!(
        w.store
            .close_unsubmitted(
                w.f.reservation.id,
                member,
                UnsubmittedReason::Expired,
                now()
            )
            .await
            .is_err()
    );
    assert!(w.store.prepare_quote(w.invoice_id, now()).await.is_err());
    assert!(
        w.store
            .prepare_invoice(w.f.reservation.id, member, now())
            .await
            .is_err()
    );
    let h = w
        .store
        .freeze_outcomes(w.f.reservation.id, now())
        .await
        .unwrap();
    let packet = w.store.outcome_packet(h.reservation.id, 0).await.unwrap();
    let (intake, task) = bridge(&w, false).await;
    assert!(intake.payout_outcome(&packet).await.is_err());
    assert!(intake.payout_outcome_progress(&h).await.is_err());
    task.abort();
    let (payout, task) = bridge(&w, true).await;
    let ack = payout.payout_outcome(&packet).await.unwrap();
    assert_eq!(ack.progress().released_sats, 2);
    assert!(w.mock.lock().unwrap().get_ids.is_empty());
    w.store
        .acknowledge_outcomes(h.reservation.id, &ack, now())
        .await
        .unwrap();
    assert_eq!(w.store.held_sats().await.unwrap(), 0);
    assert!(
        w.store
            .reconcile_outcomes(&payout, h.reservation.id)
            .await
            .unwrap()
            .finalized
    );
    assert_eq!(
        w.f.ledger
            .reward_allowance(&w.f.policy, h.reservation.epoch, h.reservation.request_id)
            .await
            .unwrap()
            .available_sats,
        1000
    );
    task.abort();
}

#[tokio::test]
async fn persisted_execute_intent_forbids_no_execute_closure_even_without_http_submission() {
    let w = Workflow::new(false).await;
    let q = w.quote().await;
    w.store
        .prepare_execute(q.id(), &w.fresh(true, false).await, now())
        .await
        .unwrap();
    assert!(
        w.store
            .close_unsubmitted(
                w.f.reservation.id,
                &w.f.plan.rows[0].pubkey,
                UnsubmittedReason::OperatorCancelled,
                now()
            )
            .await
            .is_err()
    );
    assert!(
        w.store
            .freeze_outcomes(w.f.reservation.id, now())
            .await
            .is_err()
    );
    assert_eq!(w.store.held_sats().await.unwrap(), 2);
    assert!(w.mock.lock().unwrap().patch_ids.is_empty());
}

#[tokio::test]
async fn partial_hash_chain_survives_restart_and_failed_final_commit_never_releases_early() {
    let w = Workflow::new(false).await;
    // Private protocol fixture with two journal attestation rows. Core does not
    // re-run roster planning; the protected payout role owns that attestation.
    let mut members = vec![
        crate::nostr_event::test_event(2, 0, 100, "", vec![]).pubkey,
        crate::nostr_event::test_event(3, 0, 100, "", vec![]).pubkey,
    ];
    members.sort();
    let rows: Vec<Row> = members
        .into_iter()
        .map(|member| Row {
            member,
            gross_sats: 1,
            disposition: Disposition::Unsubmitted {
                reason: UnsubmittedReason::Expired,
            },
        })
        .collect();
    let mut h = Header {
        reservation: w.f.reservation.clone(),
        scope: w.policy.scope.provider.clone(),
        rows: 2,
        final_hash: "00".repeat(32),
    };
    let seed = h.seed().unwrap();
    let first_hash = next_hash(&seed, &rows[0]).unwrap();
    h.final_hash = next_hash(&first_hash, &rows[1]).unwrap();
    let first = Packet {
        header: h.clone(),
        after: 0,
        before_hash: seed,
        row: rows[0].clone(),
    };
    let second = Packet {
        header: h.clone(),
        after: 1,
        before_hash: first_hash,
        row: rows[1].clone(),
    };
    assert!(
        w.f.ledger
            .reconcile_payout_outcome(&second, &reader(&w))
            .await
            .is_err()
    );
    let p =
        w.f.ledger
            .reconcile_payout_outcome(&first, &reader(&w))
            .await
            .unwrap();
    assert!(!p.finalized);
    assert_eq!(p.released_sats, 0);
    assert_eq!(
        w.f.ledger
            .reward_allowance(&w.f.policy, h.reservation.epoch, h.reservation.request_id)
            .await
            .unwrap()
            .available_sats,
        998
    );
    let restored = LedgerStore::connect(&w.f.database).await.unwrap();
    let mut changed = first.clone();
    changed.row.gross_sats = 2;
    assert!(
        restored
            .reconcile_payout_outcome(&changed, &reader(&w))
            .await
            .is_err()
    );
    let db = sqlx::SqlitePool::connect(&w.f.database).await.unwrap();
    sqlx::query("CREATE TRIGGER fail_outcome BEFORE INSERT ON reward_outcome_finals BEGIN SELECT RAISE(ABORT,'synthetic final failure'); END").execute(&db).await.unwrap();
    assert!(
        restored
            .reconcile_payout_outcome(&second, &reader(&w))
            .await
            .is_err()
    );
    assert_eq!(
        restored.payout_outcome_progress(&h).await.unwrap().through,
        1
    );
    sqlx::query("DROP TRIGGER fail_outcome")
        .execute(&db)
        .await
        .unwrap();
    let done = restored
        .reconcile_payout_outcome(&second, &reader(&w))
        .await
        .unwrap();
    assert!(done.finalized);
    assert_eq!(done.released_sats, 2);
    for query in [
        "DELETE FROM reward_outcome_finals",
        "UPDATE reward_outcome_rows SET gross_sats=2",
        "DELETE FROM reward_outcome_manifests",
        "DELETE FROM reward_budget_clock",
    ] {
        assert!(sqlx::query(query).execute(&db).await.is_err());
    }
    sqlx::query("UPDATE reward_budget_clock SET observed_at=unixepoch()+86400 WHERE singleton=1")
        .execute(&db)
        .await
        .unwrap();
    assert!(
        restored
            .reward_allowance(&w.f.policy, h.reservation.epoch, h.reservation.request_id)
            .await
            .is_err()
    );
    // A clock anomaly does not hide already committed acknowledgements.
    assert_eq!(restored.payout_outcome_progress(&h).await.unwrap(), done);
}

#[tokio::test]
async fn foreign_scope_preimage_and_terminal_cost_conflicts_cannot_release_budget() {
    let w = Workflow::new(false).await;
    execute(&w, PaymentState::Completed).await;
    let h = w
        .store
        .freeze_outcomes(w.f.reservation.id, now())
        .await
        .unwrap();
    let packet = w.store.outcome_packet(h.reservation.id, 0).await.unwrap();
    let reads = w.mock.lock().unwrap().get_ids.len();
    let wrong_reader = Reader::new(
        StrikeClient::new(&w.origin, "synthetic-core-read-key".into()).unwrap(),
        Uuid::new_v4(),
    )
    .unwrap();
    assert!(
        w.f.ledger
            .reconcile_payout_outcome(&packet, &wrong_reader)
            .await
            .is_err()
    );
    let mut foreign = packet.clone();
    foreign.header.reservation.plan_hash = "ab".repeat(32);
    assert!(
        w.f.ledger
            .reconcile_payout_outcome(&foreign, &reader(&w))
            .await
            .is_err()
    );
    assert_eq!(w.mock.lock().unwrap().get_ids.len(), reads);
    w.mock.lock().unwrap().wrong_hash = true;
    assert!(
        w.f.ledger
            .reconcile_payout_outcome(&packet, &reader(&w))
            .await
            .is_err()
    );
    w.mock.lock().unwrap().wrong_hash = false;
    w.mock.lock().unwrap().payment_money =
        Some(json!({"totalAmount":{"amount":"0.00000001","currency":"BTC"}}));
    assert!(
        w.f.ledger
            .reconcile_payout_outcome(&packet, &reader(&w))
            .await
            .is_err()
    );
    assert_eq!(
        w.f.ledger
            .payout_outcome_progress(&h)
            .await
            .unwrap()
            .through,
        0
    );
    assert_eq!(w.store.held_sats().await.unwrap(), 2);
    w.mock.lock().unwrap().payment_money = None;
    assert!(
        w.f.ledger
            .reconcile_payout_outcome(&packet, &reader(&w))
            .await
            .unwrap()
            .finalized
    );
}
