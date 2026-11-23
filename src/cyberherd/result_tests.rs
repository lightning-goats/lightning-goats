use super::*;
use crate::cyberherd::rewards::StoredPlan;
use crate::payout::results::{AuthenticatedPage, Page, Row, Status};

#[tokio::test]
async fn feeding_freezes_original_member_conversations_and_editable_zap_wording_before_later_interactions()
 {
    let f = Fixture::with_conversations(0, true).await;
    f.store
        .seed_templates(
            &f.owner,
            &crate::cyberherd::templates::Catalog::legacy().unwrap(),
        )
        .await
        .unwrap();
    f.store
        .edit_template(
            &f.owner,
            "zap-wording",
            &crate::cyberherd::store::TemplateEdit::Set {
                category: "cyber_herd_zap_request".into(),
                variant: "only".into(),
                text: "Treats for {name}: {new_amount} sats, frozen at feeding.".into(),
            },
        )
        .await
        .unwrap();
    // Opt out of original variants explicitly, retaining only the new edit.
    let source = crate::cyberherd::templates::Catalog::legacy().unwrap();
    for variant in source.category("cyber_herd_zap_request").unwrap().keys() {
        f.store
            .edit_template(
                &f.owner,
                &format!("remove-{variant}"),
                &crate::cyberherd::store::TemplateEdit::Delete {
                    category: "cyber_herd_zap_request".into(),
                    variant: variant.clone(),
                },
            )
            .await
            .unwrap();
    }
    let later:super::super::targets::InteractionContext=serde_json::from_value(json!({
        "matched":{"event_id":"88".repeat(32),"kind":1,"coordinate":null,"payee_pubkey":f.owner},
        "destinations":[{"event_id":"99".repeat(32),"kind":30311,"coordinate":format!("30311:{}:later",f.owner),"payee_pubkey":f.owner}]
    })).unwrap();
    f.store
        .process_with_context(
            &f.owner,
            &Interaction {
                source_id: "settlement:after-feeding".into(),
                source_hash: "aa".repeat(32),
                pubkey: key(2),
                profile: profile(2, f.now, Some("member2@example.com"))
                    .profile()
                    .clone(),
                action: Action::Zap { amount_sats: 34 },
                occurred_at: f.now,
            },
            Target::ProjectContent,
            FeederAvailability::Ready,
            Some(&later),
        )
        .await
        .unwrap();
    let frozen = f.freeze(&policy()).await.unwrap();
    let row = frozen
        .plan
        .rows
        .iter()
        .find(|r| r.pubkey == key(2))
        .unwrap();
    assert_eq!(row.amount_sats, 10);
    let context = &row.conversation.as_ref().unwrap().context;
    assert_eq!(context.note.as_ref().unwrap().event_id, "66".repeat(32));
    assert!(
        context
            .stream
            .as_ref()
            .unwrap()
            .coordinate
            .as_ref()
            .unwrap()
            .ends_with(":original")
    );
    assert!(
        row.zap_content
            .as_ref()
            .unwrap()
            .contains(&format!("{} sats", row.net_sats))
    );
    assert!(
        row.zap_content
            .as_ref()
            .unwrap()
            .contains("frozen at feeding")
    );
    let adjusted = row.render_zap(98).unwrap();
    assert!(adjusted.contains("98 sats, frozen at feeding."));
    assert!(!adjusted.contains("Different words"));
    let mut legacy = row.clone();
    legacy.zap_wording = None;
    let raw = serde_json::to_string(&legacy).unwrap();
    assert!(!raw.contains("zap_wording"));
    let restored: crate::cyberherd::rewards::Row = serde_json::from_str(&raw).unwrap();
    assert_eq!(
        restored.render_zap(restored.net_sats).unwrap(),
        restored.zap_content.as_ref().unwrap().as_str()
    );
    assert!(restored.render_zap(restored.net_sats + 1).is_err());
    assert_eq!(serde_json::to_string(&restored).unwrap(), raw);
    assert!(row.render_zap(0).is_err());
    f.store
        .edit_template(
            &f.owner,
            "change-after-freeze",
            &crate::cyberherd::store::TemplateEdit::Set {
                category: "cyber_herd_zap_request".into(),
                variant: "only".into(),
                text: "Different words".into(),
            },
        )
        .await
        .unwrap();
    assert_eq!(
        f.store.reward_plan(&f.owner, f.id).await.unwrap().unwrap(),
        frozen
    );
}

async fn plan(f: &Fixture) -> StoredPlan {
    plan_using(f, &policy()).await
}
async fn plan_using(f: &Fixture, policy: &PlanningPolicy) -> StoredPlan {
    let frozen = f.freeze(policy).await.unwrap();
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

#[tokio::test]
async fn inclusive_verified_results_retain_actual_net_fee_and_frozen_minimum() {
    let f = Fixture::new(0).await;
    let mut policy = policy();
    policy.fee_mode = crate::payout::inclusive_fees::Mode::DeductFromShare;
    policy.minimum_fee_sats = 0;
    policy.minimum_net_sats = 2;
    let frozen = plan_using(&f, &policy).await;
    let mut actual = page(&frozen);
    let gross = actual.rows[0].gross_sats;
    assert!(gross > 2);
    let mut invalid = actual.clone();
    invalid.rows[0].result = Status::Completed {
        net_sats: 1,
        fee_sats: 1,
    };
    assert!(
        f.store
            .record_reward_results(&authenticated_result(invalid).await)
            .await
            .is_err()
    );
    actual.rows[0].result = Status::Completed {
        net_sats: gross - 1,
        fee_sats: 1,
    };
    f.store
        .record_reward_results(&authenticated_result(actual.clone()).await)
        .await
        .unwrap();
    let events = f.store.events_after(&f.owner, 0, 100).await.unwrap();
    let event=events.iter().find(|e| e.event_type=="payout_result" &&
        serde_json::from_str::<serde_json::Value>(&e.snapshot_json).unwrap()["result"]["member"]==actual.rows[0].member).unwrap();
    let payload: serde_json::Value = serde_json::from_str(&event.snapshot_json).unwrap();
    assert_eq!(
        payload["result"],
        serde_json::to_value(&actual.rows[0]).unwrap()
    );
    assert_eq!(payload["breakdown"]["net_sats"], gross);
}
