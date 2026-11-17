#[tokio::test]
async fn operator_settlement_preview_rolls_back_and_apply_replays_original() {
    let (_dir,url,store,registration)=fixture().await;
    let receipt=wire(&registration,500);
    store.import_cashu_receipt(&registration,&authenticated(&registration,&receipt)).await.unwrap();
    let policy=feed_treasury_policy();
    let request=crate::cashu::treasury_sweep::Request {id:Uuid::new_v4(),mint_id:"mint-a".into(),maximum_input_sats:200};
    let before=store.latest_event_seq().await.unwrap();
    let preview=store.plan_operator_cashu_settlement(&registration,&policy,&request,false).await.unwrap().unwrap();
    assert_eq!(preview.input_sats,200);
    for table in ["cashu_sweep_requests","cashu_sweep_jobs","cashu_treasury_intents","cashu_treasury_sources"] {
        assert_eq!(count(&store,table).await,0);
    }
    assert_eq!(store.latest_event_seq().await.unwrap(),before);
    let original=store.plan_operator_cashu_settlement(&registration,&policy,&request,true).await.unwrap().unwrap();
    assert_eq!(original.input_sats,preview.input_sats);
    assert_eq!(original.deposit_sats,preview.deposit_sats);
    let reopened=LedgerStore::connect(&url).await.unwrap();
    assert!(reopened.plan_operator_cashu_settlement(&registration,&policy,&request,true).await.unwrap()==Some(original.clone()));
    assert!(reopened.plan_operator_cashu_settlement(&registration,&policy,&request,false).await.unwrap()==Some(original));
    let changed=crate::cashu::treasury_sweep::Request {maximum_input_sats:201,..request.clone()};
    assert!(reopened.plan_operator_cashu_settlement(&registration,&policy,&changed,true).await.is_err());
    let unapproved=crate::cashu::treasury_sweep::Request {id:Uuid::new_v4(),mint_id:"unapproved".into(),..request.clone()};
    assert!(reopened.plan_operator_cashu_settlement(&registration,&policy,&unapproved,true).await.is_err());
    let excessive=crate::cashu::treasury_sweep::Request {id:Uuid::new_v4(),maximum_input_sats:u64::MAX,..request};
    assert!(reopened.plan_operator_cashu_settlement(&registration,&policy,&excessive,true).await.is_err());
    assert_eq!(count(&store,"cashu_sweep_requests").await,1);
    assert_eq!(count(&store,"cashu_treasury_intents").await,1);
    assert_eq!(store.feed_credit_sats().await.unwrap(),500);
    assert_eq!(count(&store,"feed_attempts").await,0);
}

#[tokio::test]
async fn age_sweep_reserves_below_threshold_custody_without_new_credit_or_food() {
    let (_dir, _url, store, registration) = fixture().await;
    let receipt = wire(&registration, 500);
    store
        .import_cashu_receipt(&registration, &authenticated(&registration, &receipt))
        .await
        .unwrap();
    let policy = feed_treasury_policy();
    let mut sweep = crate::cashu::treasury_sweep::Policy {
        enabled: true,
        maximum_holding_age_seconds: 3600,
        ..Default::default()
    };
    assert!(
        store
            .plan_cashu_sweep(&registration, &policy, &sweep)
            .await
            .unwrap()
            .is_none()
    );
    let before_events = count(&store, "event_log").await;
    sweep.maximum_holding_age_seconds = 1;
    tokio::time::sleep(std::time::Duration::from_millis(1100)).await;
    let intent = store
        .plan_cashu_sweep(&registration, &policy, &sweep)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        (
            intent.input_sats,
            intent.deposit_sats,
            intent.maximum_fee_sats
        ),
        (500, 490, 10)
    );
    assert_eq!(store.feed_credit_sats().await.unwrap(), 500);
    assert_eq!(count(&store, "feed_attempts").await, 0);
    assert_eq!(count(&store, "event_log").await, before_events);
    assert_eq!(count(&store, "settled_payments").await, 0);
    let trigger: String = sqlx::query_scalar("SELECT reason FROM cashu_sweep_jobs")
        .fetch_one(&store.pool)
        .await
        .unwrap();
    assert_eq!(trigger, "age");
    assert!(
        store
            .plan_cashu_sweep(&registration, &policy, &sweep)
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(count(&store, "cashu_treasury_intents").await, 1);
}

#[tokio::test]
async fn balance_sweep_includes_retired_excess_but_never_restores_retired_credit() {
    let (_dir, _url, store, registration) = fixture().await;
    let receipt = wire(&registration, 9000);
    store
        .import_cashu_receipt(&registration, &authenticated(&registration, &receipt))
        .await
        .unwrap();
    assert_eq!(store.feed_credit_sats().await.unwrap(), 3000);
    let before_events = count(&store, "event_log").await;
    let policy = feed_treasury_policy();
    let mut sweep = crate::cashu::treasury_sweep::Policy {
        enabled: true,
        balance_trigger_sats: 10000,
        ..Default::default()
    };
    assert!(
        store
            .plan_cashu_sweep(&registration, &policy, &sweep)
            .await
            .unwrap()
            .is_none()
    );
    sweep.balance_trigger_sats = 8000;
    let intent = store
        .plan_cashu_sweep(&registration, &policy, &sweep)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(intent.input_sats, 9000);
    assert_eq!(store.feed_credit_sats().await.unwrap(), 3000);
    assert_eq!(count(&store, "feed_credit_retirements").await, 1);
    assert_eq!(count(&store, "feed_attempts").await, 0);
    assert_eq!(count(&store, "event_log").await, before_events);
    assert!(
        store
            .plan_cashu_sweep(&registration, &policy, &sweep)
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn operator_sweep_is_atomic_once_per_uuid_and_conflict_safe_after_restart() {
    let (_dir, url, store, registration) = fixture().await;
    let receipt = wire(&registration, 500);
    store
        .import_cashu_receipt(&registration, &authenticated(&registration, &receipt))
        .await
        .unwrap();
    let mut policy = feed_treasury_policy();
    let mut sweep = crate::cashu::treasury_sweep::Policy {
        enabled: true,
        requests: vec![crate::cashu::treasury_sweep::Request {
            id: Uuid::new_v4(),
            mint_id: "mint-a".into(),
            maximum_input_sats: 200,
        }],
        ..Default::default()
    };
    policy.enabled = false;
    assert!(
        store
            .plan_cashu_sweep(&registration, &policy, &sweep)
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(count(&store, "cashu_sweep_requests").await, 0);
    policy.enabled = true;
    sqlx::raw_sql("CREATE TRIGGER refuse_sweep_link BEFORE INSERT ON cashu_sweep_jobs BEGIN SELECT RAISE(ABORT,'fixture sweep linkage'); END;")
        .execute(&store.pool).await.unwrap();
    assert!(
        store
            .plan_cashu_sweep(&registration, &policy, &sweep)
            .await
            .is_err()
    );
    assert_eq!(count(&store, "cashu_treasury_intents").await, 0);
    assert_eq!(count(&store, "cashu_treasury_sources").await, 0);
    assert_eq!(count(&store, "cashu_sweep_requests").await, 0);
    sqlx::raw_sql("DROP TRIGGER refuse_sweep_link")
        .execute(&store.pool)
        .await
        .unwrap();
    let original = store
        .plan_cashu_sweep(&registration, &policy, &sweep)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(original.input_sats, 200);
    let reopened = LedgerStore::connect(&url).await.unwrap();
    assert!(
        reopened
            .plan_cashu_sweep(&registration, &policy, &sweep)
            .await
            .unwrap()
            .is_none()
    );
    sweep.requests[0].maximum_input_sats = 201;
    assert!(
        reopened
            .plan_cashu_sweep(&registration, &policy, &sweep)
            .await
            .is_err()
    );
    assert_eq!(count(&store, "cashu_treasury_intents").await, 1);
    assert_eq!(count(&store, "cashu_sweep_requests").await, 1);
    assert!(store.cashu_treasury_intent(original.id).await.unwrap() == Some(original));
    assert_eq!(store.feed_credit_sats().await.unwrap(), 500);
    assert_eq!(count(&store, "feed_attempts").await, 0);
}

#[tokio::test]
async fn sweep_racing_feed_job_uses_one_shared_custody_reservation() {
    let (_dir, url, store, registration) = fixture().await;
    lightning(&store, 600).await;
    let receipt = wire(&registration, 400);
    store
        .import_cashu_receipt(&registration, &authenticated(&registration, &receipt))
        .await
        .unwrap();
    assert!(store.begin_feed_attempt(1000).await.unwrap().is_none());
    let other = LedgerStore::connect(&url).await.unwrap();
    let policy = feed_treasury_policy();
    let sweep = crate::cashu::treasury_sweep::Policy {
        enabled: true,
        balance_trigger_sats: 1,
        ..Default::default()
    };
    let (feed, swept) = tokio::join!(
        store.plan_cashu_feed_treasury(&registration, &policy),
        other.plan_cashu_sweep(&registration, &policy, &sweep),
    );
    let feed = feed.unwrap();
    let swept = swept.unwrap();
    assert_eq!(
        usize::from(feed.is_some()) + usize::from(swept.is_some()),
        1
    );
    assert_eq!(count(&store, "cashu_treasury_intents").await, 1);
    let held: i64 = sqlx::query_scalar("SELECT SUM(input_sats) FROM cashu_treasury_sources")
        .fetch_one(&store.pool)
        .await
        .unwrap();
    assert_eq!(held, 400);
    assert_eq!(store.feed_credit_sats().await.unwrap(), 1000);
    assert_eq!(count(&store, "feed_attempts").await, 0);
}

#[test]
fn sweep_policy_requires_explicit_bounded_triggers_and_unique_operator_ids() {
    let policy = feed_treasury_policy();
    let mut empty = crate::cashu::treasury_sweep::Policy::default();
    empty.validate(&policy).unwrap();
    empty.enabled = true;
    assert!(empty.validate(&policy).is_err());
    let request = crate::cashu::treasury_sweep::Request {
        id: Uuid::new_v4(),
        mint_id: "mint-a".into(),
        maximum_input_sats: 1,
    };
    let mut sweep = crate::cashu::treasury_sweep::Policy {
        enabled: true,
        requests: vec![request.clone()],
        ..Default::default()
    };
    sweep.validate(&policy).unwrap();
    sweep.requests.push(request);
    assert!(sweep.validate(&policy).is_err());
    sweep.requests.pop();
    sweep.requests[0].mint_id = "unapproved".into();
    assert!(sweep.validate(&policy).is_err());
    let mut paused = policy.clone();
    paused.enabled = false;
    sweep.validate(&paused).unwrap();
    sweep.enabled = false;
    sweep.validate(&policy).unwrap();
    assert!(
        serde_json::from_str::<crate::cashu::treasury_sweep::Policy>(r#"{"unbounded":true}"#)
            .is_err()
    );
}

#[tokio::test]
async fn automatic_sweeps_rotate_mints_using_durable_admission_order() {
    let (_dir, _url, store, mut registration) = fixture().await;
    registration.approved.insert(
        ("mint-b".into(), "v1".into()),
        MintImportLimit {
            maximum_gross_sats: 10000,
            maximum_net_sats: 10000,
        },
    );
    for mint in ["mint-a", "mint-b"] {
        let mut data = wire(&registration, 200).data().clone();
        data.mint_id = mint.into();
        let receipt = WireReceipt::validate(data).unwrap();
        store
            .import_cashu_receipt(&registration, &authenticated(&registration, &receipt))
            .await
            .unwrap();
    }
    let mut policy = feed_treasury_policy();
    policy
        .approved_mints
        .insert("mint-b".into(), policy.approved_mints["mint-a"].clone());
    policy.limits.maximum_input_sats = 100;
    policy.limits.maximum_deposit_sats = 100;
    let sweep = crate::cashu::treasury_sweep::Policy {
        enabled: true,
        balance_trigger_sats: 1,
        ..Default::default()
    };
    for mint in ["mint-a", "mint-b", "mint-a", "mint-b"] {
        let intent = store
            .plan_cashu_sweep(&registration, &policy, &sweep)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(intent.mint_id, mint);
        assert_eq!(intent.input_sats, 100);
    }
    assert!(
        store
            .plan_cashu_sweep(&registration, &policy, &sweep)
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(store.feed_credit_sats().await.unwrap(), 400);
    assert_eq!(count(&store, "feed_attempts").await, 0);
}
