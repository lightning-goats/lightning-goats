fn feed_treasury_policy() -> crate::cashu::treasury::Policy {
    use crate::cashu::treasury::{Limits, MintLimits, Policy};
    Policy {
        enabled: true,
        version: "feed-planner-fixture-v1".into(),
        destination: lightning_goats_cashu_receipt::treasury::Destination {
            origin: "https://treasury.example/".into(),
            account_id: Uuid::new_v4(),
            credential_fingerprint: "55".repeat(32),
            network: "bitcoin".into(),
        },
        approved_mints: BTreeMap::from([(
            "mint-a".into(),
            MintLimits {
                minimum_deposit_sats: 1,
                maximum_deposit_sats: 10000,
                maximum_input_sats: 10000,
                maximum_fee_sats: 10,
                maximum_in_flight_sats: 20000,
            },
        )]),
        invoice_expiry_seconds: 300,
        limits: Limits {
            minimum_deposit_sats: 1,
            maximum_deposit_sats: 10000,
            maximum_input_sats: 10000,
            maximum_fee_sats: 10,
            operations_per_24h: 100,
            input_sats_per_24h: 20000,
            maximum_in_flight_sats: 20000,
        },
    }
}

#[tokio::test]
async fn financial_plan_creates_one_atomic_treasury_job_and_retains_it_on_supersession() {
    let (_dir, url, store, registration) = fixture().await;
    lightning(&store, 600).await;
    let receipt = wire(&registration, 400);
    store
        .import_cashu_receipt(&registration, &authenticated(&registration, &receipt))
        .await
        .unwrap();
    assert!(store.begin_feed_attempt(1000).await.unwrap().is_none());
    let mut policy = feed_treasury_policy();
    policy.enabled = false;
    assert!(
        store
            .plan_cashu_feed_treasury(&registration, &policy)
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(count(&store, "cashu_treasury_intents").await, 0);
    policy.enabled = true;
    sqlx::raw_sql("CREATE TRIGGER fail_plan_job_link BEFORE INSERT ON feed_financial_treasury BEGIN SELECT RAISE(ABORT,'fixture job link'); END;")
        .execute(&store.pool).await.unwrap();
    assert!(
        store
            .plan_cashu_feed_treasury(&registration, &policy)
            .await
            .is_err()
    );
    assert_eq!(count(&store, "cashu_treasury_intents").await, 0);
    assert_eq!(count(&store, "cashu_treasury_sources").await, 0);
    sqlx::raw_sql("DROP TRIGGER fail_plan_job_link")
        .execute(&store.pool)
        .await
        .unwrap();
    let intent = store
        .plan_cashu_feed_treasury(&registration, &policy)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        (
            intent.input_sats,
            intent.deposit_sats,
            intent.maximum_fee_sats
        ),
        (400, 390, 10)
    );
    assert_eq!(intent.sources.len(), 1);
    assert_eq!(
        intent.sources[0].acquisition_id,
        receipt.data().acquisition_id
    );
    assert_eq!(count(&store, "feed_financial_treasury").await, 1);
    let reopened = LedgerStore::connect(&url).await.unwrap();
    assert!(
        reopened
            .plan_cashu_feed_treasury(&registration, &policy)
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(count(&store, "cashu_treasury_intents").await, 1);
    lightning(&store, 400).await;
    let request = store.begin_feed_attempt(1000).await.unwrap().unwrap();
    store.confirm_feed_attempt(request).await.unwrap();
    assert_eq!(store.feed_credit_sats().await.unwrap(), 400);
    assert!(store.cashu_treasury_intent(intent.id).await.unwrap() == Some(intent));
    let linked_state:String=sqlx::query_scalar("SELECT f.state FROM feed_financial_treasury j JOIN feed_financial_plans f ON f.id=j.plan_id")
        .fetch_one(&store.pool).await.unwrap();
    assert_eq!(linked_state, "released");
    assert_eq!(count(&store, "cashu_treasury_issuances").await, 0);
}

#[tokio::test]
async fn financial_job_planning_skips_a_mint_with_no_exposure_capacity() {
    let (_dir, _url, store, mut registration) = fixture().await;
    let original = &registration.approved[&("mint-a".into(), "v1".into())];
    let approved = MintImportLimit {
        maximum_gross_sats: original.maximum_gross_sats,
        maximum_net_sats: original.maximum_net_sats,
    };
    registration
        .approved
        .insert(("mint-b".into(), "v1".into()), approved);
    lightning(&store, 600).await;
    let a = wire(&registration, 200);
    store
        .import_cashu_receipt(&registration, &authenticated(&registration, &a))
        .await
        .unwrap();
    let mut b = wire(&registration, 200).data().clone();
    b.mint_id = "mint-b".into();
    let b = WireReceipt::validate(b).unwrap();
    store
        .import_cashu_receipt(&registration, &authenticated(&registration, &b))
        .await
        .unwrap();
    assert!(store.begin_feed_attempt(1000).await.unwrap().is_none());
    let extra = wire(&registration, 100);
    store
        .import_cashu_receipt(&registration, &authenticated(&registration, &extra))
        .await
        .unwrap();
    let ledger: i64 = sqlx::query_scalar(
        "SELECT ledger_entry_id FROM cashu_receipt_inbox WHERE acquisition_id=?",
    )
    .bind(extra.data().acquisition_id.to_string())
    .fetch_one(&store.pool)
    .await
    .unwrap();
    let mut policy = feed_treasury_policy();
    let other = policy.approved_mints["mint-a"].clone();
    policy.approved_mints.insert("mint-b".into(), other);
    policy
        .approved_mints
        .get_mut("mint-a")
        .unwrap()
        .maximum_in_flight_sats = 100;
    store
        .reserve_cashu_treasury(
            &registration,
            &policy,
            &crate::ledger::TreasuryPlan {
                id: Uuid::new_v4(),
                mint_id: "mint-a".into(),
                sources: vec![(ledger, 100)],
                deposit_sats: 90,
                maximum_fee_sats: 10,
            },
        )
        .await
        .unwrap();
    let planned = store
        .plan_cashu_feed_treasury(&registration, &policy)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(planned.mint_id, "mint-b");
    assert_eq!(planned.input_sats, 200);
    assert_eq!(planned.sources[0].acquisition_id, b.data().acquisition_id);
    assert!(
        store
            .plan_cashu_feed_treasury(&registration, &policy)
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(count(&store, "cashu_treasury_intents").await, 2);
}
