//! The independent core journal survives loss of payout-side execution history.
use super::*;
use crate::payout::execution::{Limits, Request};

fn limits(w: &Workflow, enabled: bool, nostr: bool) -> Limits<'_> {
    Limits {
        policy: Some(&w.f.policy),
        account: Some(w.policy.scope.provider.account_id),
        enabled,
        nostr,
    }
}

#[tokio::test]
async fn original_execution_claim_is_immutable_scoped_budgeted_and_retained_across_restart() {
    let w = Workflow::new(false).await;
    let quote = w.quote().await;
    let intent = w
        .store
        .prepare_execute(quote.id(), &w.fresh(true, false).await, now())
        .await
        .unwrap();
    let request = intent.claim_request();
    assert!(
        w.f.ledger
            .claim_payout_execution(request, limits(&w, false, true))
            .await
            .is_err()
    );
    let mut wrong_account = limits(&w, true, true);
    wrong_account.account = Some(Uuid::new_v4());
    assert!(
        w.f.ledger
            .claim_payout_execution(request, wrong_account)
            .await
            .is_err()
    );
    let mut foreign = request.clone();
    foreign.reservation.epoch = Uuid::new_v4();
    assert!(
        w.f.ledger
            .claim_payout_execution(&foreign, limits(&w, true, true))
            .await
            .is_err()
    );
    let saved =
        w.f.ledger
            .claim_payout_execution(request, limits(&w, true, true))
            .await
            .unwrap();
    assert!(saved.permitted(now()));
    let mut changed = request.clone();
    let mut terms = changed.quote().unwrap();
    terms.quote_id = Uuid::new_v4();
    changed.quote_json = serde_json::to_string(&terms).unwrap();
    assert!(
        w.f.ledger
            .claim_payout_execution(&changed, limits(&w, true, true))
            .await
            .is_err()
    );
    let mut extra = request.clone();
    extra.member = crate::nostr_event::test_event(9, 0, 100, "", vec![]).pubkey;
    assert!(
        w.f.ledger
            .claim_payout_execution(&extra, limits(&w, true, true))
            .await
            .is_err()
    );
    let restored = LedgerStore::connect(&w.f.database).await.unwrap();
    let held = restored
        .claim_payout_execution(request, limits(&w, false, true))
        .await
        .unwrap();
    assert_eq!(held.record, saved.record);
    assert!(!held.permitted(now()));
    let state = restored
        .payout_execution_state(
            request.reservation.epoch,
            request.reservation.id,
            &request.reservation.owner,
            &request.member,
            limits(&w, true, true),
        )
        .await
        .unwrap();
    assert_eq!(state.claim.unwrap().record, saved.record);
    let db = sqlx::SqlitePool::connect(&w.f.database).await.unwrap();
    for sql in [
        "DELETE FROM payout_execution_claims",
        "UPDATE payout_execution_claims SET gross_sats=1",
    ] {
        assert!(sqlx::query(sql).execute(&db).await.is_err());
    }
    assert!(w.mock.lock().unwrap().patch_ids.is_empty());
}

#[tokio::test]
async fn supported_zap_execution_claim_requires_public_authority_and_intake_cannot_claim() {
    let w = Workflow::new(true).await;
    let quote = w.quote().await;
    let intent = w
        .store
        .prepare_execute(quote.id(), &w.fresh(true, true).await, now())
        .await
        .unwrap();
    assert!(
        w.f.ledger
            .claim_payout_execution(intent.claim_request(), limits(&w, true, false))
            .await
            .is_err()
    );
    let (uid, gid) = self_credentials().unwrap();
    let config = BridgeConfig {
        socket: w.f.root.path().join("intake-only.sock"),
        peer_uid: uid,
        group_gid: gid,
        reward_policy: Some(w.f.policy.clone()),
        payout_peer_uid: Some(uid + 1),
        delegated_feeder_messages: false,
        payout_execution_enabled: true,
        payout_account_id: Some(w.policy.scope.provider.account_id),
    };
    let task = tokio::spawn(
        Server::bind(&config)
            .await
            .unwrap()
            .run(w.f.ledger.clone(), RuntimeMode::Active),
    );
    let client = Client::new(config.socket, uid).unwrap();
    assert!(
        client
            .claim_execution(intent.claim_request())
            .await
            .is_err()
    );
    assert!(
        client
            .execution_state(
                w.f.reservation.epoch,
                w.f.reservation.id,
                &w.f.reservation.owner,
                &w.f.plan.rows[0].pubkey
            )
            .await
            .is_err()
    );
    let request: &Request = intent.claim_request();
    assert!(
        w.f.ledger
            .claim_payout_execution(request, limits(&w, true, true))
            .await
            .unwrap()
            .permitted(now())
    );
    assert!(w.mock.lock().unwrap().patch_ids.is_empty());
    task.abort();
}
