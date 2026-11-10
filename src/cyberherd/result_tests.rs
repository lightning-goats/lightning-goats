use super::*;
use crate::cyberherd::rewards::StoredPlan;
use crate::payout::results::{AuthenticatedPage, Page, Row, Status};

async fn plan(f: &Fixture) -> StoredPlan {
    let frozen = f.freeze(&policy()).await.unwrap();
    let reservation = f
        .core
        .reserve_reward(
            &f.root_policy,
            frozen.plan.epoch,
            f.id,
            &frozen.hash,
            frozen.plan.reserved_sats,
        )
        .await
        .unwrap();
    f.store
        .attach_reward_reservation(&f.owner, &approved(reservation).await)
        .await
        .unwrap();
    f.store.reward_plan(&f.owner, f.id).await.unwrap().unwrap()
}
fn page(plan: &StoredPlan) -> Page {
    let mut rows: Vec<_> = plan
        .plan
        .rows
        .iter()
        .filter(|row| row.skip.is_none())
        .map(|row| Row {
            member: row.pubkey.clone(),
            gross_sats: row.gross_sats,
            verified_at: plan
                .plan
                .created_at
                .max(plan.reservation.as_ref().unwrap().created_at),
            result: Status::Completed {
                net_sats: row.net_sats,
                fee_sats: 0,
            },
        })
        .collect();
    rows.sort_by(|a, b| a.member.cmp(&b.member));
    Page {
        reservation: plan.reservation.clone().unwrap(),
        manifest_hash: "44".repeat(32),
        after: 0,
        through: rows.len().try_into().unwrap(),
        total_rows: rows.len().try_into().unwrap(),
        rows,
    }
}
async fn authenticated_result(page: Page) -> AuthenticatedPage {
    let reservation = page.reservation.clone();
    let after = page.after;
    let (_root, client, task) = peer(Response::RewardResults {
        page: Some(Box::new(page)),
    })
    .await;
    let value = client
        .reward_results(&reservation, after)
        .await
        .unwrap()
        .unwrap();
    task.await.unwrap();
    value
}

#[tokio::test]
async fn verified_results_freeze_original_breakdown_atomically_once_across_restart_and_pages() {
    let f = Fixture::new(0).await;
    let frozen = plan(&f).await;
    let whole = page(&frozen);
    assert_eq!(whole.rows.len(), 3);
    let mut first = whole.clone();
    first.rows.truncate(2);
    first.through = 2;
    let first = authenticated_result(first).await;
    let mut second = whole.clone();
    second.rows = second.rows.split_off(2);
    second.after = 2;
    let second = authenticated_result(second).await;
    assert!(f.store.record_reward_results(&second).await.is_err());
    let db = sqlx::SqlitePool::connect(&f.url).await.unwrap();
    sqlx::query("CREATE TRIGGER fail_result BEFORE INSERT ON domain_events WHEN NEW.event_type='payout_result' BEGIN SELECT RAISE(ABORT,'synthetic result commit failure'); END")
        .execute(&db).await.unwrap();
    assert!(f.store.record_reward_results(&first).await.is_err());
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM reward_result_pages")
            .fetch_one(&db)
            .await
            .unwrap(),
        0
    );
    sqlx::query("DROP TRIGGER fail_result")
        .execute(&db)
        .await
        .unwrap();
    let (a, b) = tokio::join!(
        f.store.record_reward_results(&first),
        f.store.record_reward_results(&first)
    );
    a.unwrap();
    b.unwrap();
    let reopened = Store::connect(&f.url).await.unwrap();
    reopened.record_reward_results(&second).await.unwrap();
    reopened.record_reward_results(&first).await.unwrap();
    assert!(
        reopened
            .next_reward_result(&f.owner)
            .await
            .unwrap()
            .is_none()
    );
    let events: Vec<_> = reopened
        .events_after(&f.owner, 0, 100)
        .await
        .unwrap()
        .into_iter()
        .filter(|event| event.event_type == "payout_result")
        .collect();
    assert_eq!(events.len(), 3);
    for event in events {
        let payload: serde_json::Value = serde_json::from_str(&event.snapshot_json).unwrap();
        let member = payload["result"]["member"].as_str().unwrap();
        let original = frozen
            .plan
            .rows
            .iter()
            .find(|row| row.pubkey == member)
            .unwrap();
        assert_eq!(
            payload["breakdown"],
            serde_json::to_value(original).unwrap()
        );
        assert_eq!(event.cycle, frozen.plan.cycle);
    }
    for sql in [
        "DELETE FROM reward_result_pages",
        "UPDATE reward_result_cursors SET through=0",
    ] {
        assert!(sqlx::query(sql).execute(&db).await.is_err());
    }
    let mut changed = whole;
    changed.rows.truncate(2);
    changed.through = 2;
    changed.manifest_hash = "55".repeat(32);
    assert!(
        reopened
            .record_reward_results(&authenticated_result(changed).await)
            .await
            .is_err()
    );
}

#[tokio::test]
async fn verified_result_net_member_gross_and_manifest_changes_cannot_create_treat_events() {
    let f = Fixture::new(0).await;
    let frozen = plan(&f).await;
    let original = page(&frozen);
    let mut changes = Vec::new();
    let mut changed = original.clone();
    let Status::Completed {
        ref mut net_sats, ..
    } = changed.rows[0].result
    else {
        unreachable!()
    };
    *net_sats -= 1;
    changes.push(changed);
    let mut changed = original.clone();
    changed.rows[0].gross_sats += 1;
    changed.rows[1].gross_sats -= 1;
    changes.push(changed);
    let mut changed = original.clone();
    changed.rows[0].member = key(9);
    changed.rows.sort_by(|a, b| a.member.cmp(&b.member));
    changes.push(changed);
    for changed in changes {
        assert!(
            f.store
                .record_reward_results(&authenticated_result(changed).await)
                .await
                .is_err()
        );
    }
    assert!(
        !f.store
            .events_after(&f.owner, 0, 100)
            .await
            .unwrap()
            .iter()
            .any(|e| e.event_type == "payout_result")
    );
    // A legitimate failure is retained as failure, never a Completed claim.
    let mut failed = original;
    failed.rows[0].result = Status::Failed;
    f.store
        .record_reward_results(&authenticated_result(failed).await)
        .await
        .unwrap();
    let events = f.store.events_after(&f.owner, 0, 100).await.unwrap();
    assert!(
        events
            .iter()
            .any(|e| e.event_type == "payout_result" && e.snapshot_json.contains("\"failed\""))
    );
}

#[tokio::test]
async fn result_worker_waits_without_inventing_delivery_then_saves_real_authenticated_page() {
    let f = Fixture::new(0).await;
    let frozen = plan(&f).await;
    let (_root, client, task) = peer(Response::RewardResults { page: None }).await;
    let worker = crate::cyberherd::result_worker::Worker {
        store: f.store.clone(),
        client,
        owner: f.owner.clone(),
    };
    assert_eq!(
        worker.tick().await.unwrap(),
        crate::cyberherd::result_worker::Step::Waiting
    );
    task.await.unwrap();
    assert!(
        !f.store
            .events_after(&f.owner, 0, 100)
            .await
            .unwrap()
            .iter()
            .any(|e| e.event_type == "payout_result")
    );
    let (_root, client, task) = peer(Response::RewardResults {
        page: Some(Box::new(page(&frozen))),
    })
    .await;
    let worker = crate::cyberherd::result_worker::Worker {
        store: Store::connect(&f.url).await.unwrap(),
        client,
        owner: f.owner.clone(),
    };
    assert_eq!(
        worker.tick().await.unwrap(),
        crate::cyberherd::result_worker::Step::Saved
    );
    task.await.unwrap();
    assert_eq!(
        worker.tick().await.unwrap(),
        crate::cyberherd::result_worker::Step::Idle
    );
}
