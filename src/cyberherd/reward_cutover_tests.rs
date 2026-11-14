use super::*;
use crate::cyberherd::reward_cutover::{Decision as Cutover, HistoricalFeeding};

async fn inventory(f: &Fixture) -> Cutover {
    let epoch = f.core.overlay_stream_id().await.unwrap();
    let batch = f.core.core_event_batch(Some(epoch), 0, 128).await.unwrap();
    Cutover {
        id: Uuid::new_v4(),
        epoch,
        through: batch.latest,
        backup_manifest_sha256: "ab".repeat(32),
        decided_at: f.now,
        historical: batch
            .events
            .iter()
            .filter_map(|event| match &event.body {
                crate::core_bridge::EventBody::Feeding(feed) => Some(HistoricalFeeding {
                    request_id: feed.request_id,
                    core_seq: event.seq,
                    proof_hash: crate::cyberherd::ingress::hash(feed).unwrap(),
                }),
                _ => None,
            })
            .collect(),
    }
}
async fn confirmed_virtual_feed(f: &Fixture) -> Uuid {
    f.core
        .grant_operator_credit(
            &OperatorCreditGrant {
                id: Uuid::new_v4().to_string(),
                amount_sats: 1000,
                address_user: "herd".into(),
                reason: "Synthetic cutover fixture".into(),
            },
            false,
        )
        .await
        .unwrap();
    let id = f.core.begin_feed_attempt(1000).await.unwrap().unwrap();
    f.core.confirm_feed_attempt(id).await.unwrap();
    id
}
async fn ingest_and_classify(f: &Fixture) {
    let cursor = f.store.core_cursor(&f.owner).await.unwrap();
    let batch = f
        .core
        .core_event_batch(Some(cursor.binding.epoch), cursor.received_through, 128)
        .await
        .unwrap();
    let (_root, client, task) = peer(Response::Events {
        batch: Box::new(batch),
    })
    .await;
    let page = client
        .events(Some(cursor.binding.epoch), cursor.received_through, 128)
        .await
        .unwrap();
    task.await.unwrap();
    f.store.ingest_core(&f.owner, &page).await.unwrap();
    let targets = Targets::project(
        TargetPolicy {
            project_pubkey: f.owner.clone(),
            tracked_tags: vec!["CyberHerd".into()],
        },
        &DayWindow::at("America/Denver", f.now).unwrap(),
        vec![],
        vec![],
        BTreeSet::new(),
        f.now,
    )
    .unwrap();
    while f.store.ingress_head(&f.owner).await.unwrap().is_some() {
        worker::step(
            &f.store,
            &f.owner,
            Environment {
                targets: &targets,
                discovery_available: true,
                feeder: FeederAvailability::Ready,
                profile: None,
                now: f.now,
            },
        )
        .await
        .unwrap();
    }
}

#[tokio::test]
async fn initial_cutover_excludes_frozen_and_not_yet_received_feedings_but_not_future_ones() {
    let f = Fixture::new(0).await;
    let frozen = f
        .store
        .feeding_barrier(&f.owner, f.id)
        .await
        .unwrap()
        .unwrap();
    let delayed = confirmed_virtual_feed(&f).await;
    let decision = inventory(&f).await;
    assert_eq!(decision.historical.len(), 2);
    let receipt = f
        .store
        .record_initial_reward_cutover(&f.owner, &decision)
        .await
        .unwrap();
    assert!(!receipt.activation_authorized);
    assert_eq!(receipt.historical_feedings, 2);
    assert!(
        f.store
            .next_reward_feeding(&f.owner)
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        f.store
            .next_reward_export(&f.owner, 0)
            .await
            .unwrap()
            .is_none()
    );
    assert!(f.freeze(&policy()).await.is_err());
    ingest_and_classify(&f).await;
    assert!(
        f.store
            .feeding_barrier(&f.owner, delayed)
            .await
            .unwrap()
            .is_some()
    );
    assert_eq!(
        f.store
            .feeding_barrier(&f.owner, f.id)
            .await
            .unwrap()
            .unwrap(),
        frozen
    );
    assert!(
        f.store
            .next_reward_feeding(&f.owner)
            .await
            .unwrap()
            .is_none()
    );
    let reopened = Store::connect(&f.url).await.unwrap();
    assert_eq!(
        reopened
            .record_initial_reward_cutover(&f.owner, &decision)
            .await
            .unwrap(),
        receipt
    );
    let mut changed = decision.clone();
    changed.id = Uuid::new_v4();
    assert!(
        reopened
            .record_initial_reward_cutover(&f.owner, &changed)
            .await
            .is_err()
    );
    let future = confirmed_virtual_feed(&f).await;
    ingest_and_classify(&f).await;
    assert_eq!(
        reopened.next_reward_feeding(&f.owner).await.unwrap(),
        Some(future)
    );
    assert_eq!(f.core.feed_credit_sats().await.unwrap(), 0);
}

#[tokio::test]
async fn cutover_refuses_missing_proofs_wrong_epoch_and_existing_frozen_reward_authority() {
    let f = Fixture::new(0).await;
    let valid = inventory(&f).await;
    let mut omitted = valid.clone();
    omitted.historical.clear();
    assert!(
        f.store
            .record_initial_reward_cutover(&f.owner, &omitted)
            .await
            .is_err()
    );
    let mut wrong = valid.clone();
    wrong.historical[0].proof_hash = "cd".repeat(32);
    assert!(
        f.store
            .record_initial_reward_cutover(&f.owner, &wrong)
            .await
            .is_err()
    );
    wrong = valid.clone();
    wrong.epoch = Uuid::new_v4();
    assert!(
        f.store
            .record_initial_reward_cutover(&f.owner, &wrong)
            .await
            .is_err()
    );
    let plan = f.freeze(&policy()).await.unwrap();
    assert!(
        f.store
            .record_initial_reward_cutover(&f.owner, &valid)
            .await
            .is_err()
    );
    assert_eq!(
        f.store.reward_plan(&f.owner, f.id).await.unwrap().unwrap(),
        plan
    );
    let pool = sqlx::SqlitePool::connect(&f.url).await.unwrap();
    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM operator_commands WHERE command_id LIKE 'initial-reward-cutover%'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(count, 0);
    pool.close().await;
}

#[tokio::test]
async fn historical_input_missing_from_inventory_cannot_advance_the_cursor_or_gain_reward_authority()
 {
    let f = Fixture::new(0).await;
    let delayed = confirmed_virtual_feed(&f).await;
    let mut cutover = inventory(&f).await;
    cutover.historical.retain(|item| item.request_id != delayed);
    // The offline native boundary must reject an incomplete core inventory.
    // Independently, intake fails closed even if a trusted caller supplied one.
    f.store
        .record_initial_reward_cutover(&f.owner, &cutover)
        .await
        .unwrap();
    let before = f.store.core_cursor(&f.owner).await.unwrap();
    let batch = f
        .core
        .core_event_batch(Some(before.binding.epoch), before.received_through, 128)
        .await
        .unwrap();
    let (_root, client, task) = peer(Response::Events {
        batch: Box::new(batch),
    })
    .await;
    let page = client
        .events(Some(before.binding.epoch), before.received_through, 128)
        .await
        .unwrap();
    task.await.unwrap();
    assert!(f.store.ingest_core(&f.owner, &page).await.is_err());
    assert_eq!(f.store.core_cursor(&f.owner).await.unwrap(), before);
    assert!(
        f.store
            .feeding_barrier(&f.owner, delayed)
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        f.store
            .reward_plan(&f.owner, delayed)
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn native_cutover_inventory_matches_authenticated_projection_and_includes_delayed_feeds() {
    let f = Fixture::new(0).await;
    let delayed = confirmed_virtual_feed(&f).await;
    let expected = inventory(&f).await;
    let cursor = f.store.core_cursor(&f.owner).await.unwrap();
    assert!(cursor.received_through < expected.through);
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(
            sqlx::sqlite::SqliteConnectOptions::new()
                .filename(f._root.path().join("core.db"))
                .read_only(true)
                .create_if_missing(false)
                .pragma("query_only", "ON"),
        )
        .await
        .unwrap();
    let observed =
        crate::state_maintenance::cutover_fixture_history(&pool, &cursor.binding, expected.through)
            .await
            .unwrap();
    assert_eq!(observed, expected.historical);
    assert!(observed.iter().any(|item| item.request_id == delayed));
    assert_eq!(f.store.core_cursor(&f.owner).await.unwrap(), cursor);
    assert!(
        f.store
            .feeding_barrier(&f.owner, delayed)
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        crate::state_maintenance::cutover_fixture_history(
            &pool,
            &cursor.binding,
            expected.through - 1
        )
        .await
        .is_err()
    );
    let mut wrong = cursor.binding.clone();
    wrong.epoch = Uuid::new_v4();
    assert!(
        crate::state_maintenance::cutover_fixture_history(&pool, &wrong, expected.through)
            .await
            .is_err()
    );
    pool.close().await;
}
