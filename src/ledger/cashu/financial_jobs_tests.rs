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
            unlimited_operations_per_24h: false,
            input_sats_per_24h: 20000,
            unlimited_input_sats_per_24h: false,
            fee_sats_per_24h: Some(1000),
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
    policy.limits.operations_per_24h = 0;
    policy.limits.unlimited_operations_per_24h = true;
    policy.limits.input_sats_per_24h = 0;
    policy.limits.unlimited_input_sats_per_24h = true;
    policy.limits.fee_sats_per_24h = Some(19);
    policy
        .approved_mints
        .get_mut("mint-a")
        .unwrap()
        .maximum_in_flight_sats = 100;
    let original_plan = crate::ledger::TreasuryPlan {
        id: Uuid::new_v4(),
        mint_id: "mint-a".into(),
        sources: vec![(ledger, 100)],
        deposit_sats: 90,
        maximum_fee_sats: 10,
    };
    let original = store
        .reserve_cashu_treasury(
            &registration,
            &policy,
            &original_plan,
        )
        .await
        .unwrap();
    // Nine sats remain. Waiting must not freeze a lower-fee invoice.
    assert!(store.plan_cashu_feed_treasury(&registration, &policy).await.unwrap().is_none());
    let ledger_b: i64 = sqlx::query_scalar("SELECT ledger_entry_id FROM cashu_receipt_inbox WHERE acquisition_id=?")
        .bind(b.data().acquisition_id.to_string()).fetch_one(&store.pool).await.unwrap();
    let blocked = crate::ledger::TreasuryPlan {
        id: Uuid::new_v4(), mint_id: "mint-b".into(), sources: vec![(ledger_b, 200)],
        deposit_sats: 190, maximum_fee_sats: 10,
    };
    assert!(store.reserve_cashu_treasury(&registration, &policy, &blocked).await.err().unwrap()
        .to_string().contains("daily fee budget exhausted"));
    assert_eq!(count(&store, "cashu_treasury_intents").await, 1);
    let mut tx = store.pool.begin().await.unwrap();
    let created: i64 = sqlx::query_scalar("SELECT created_at FROM cashu_treasury_intents WHERE id=?")
        .bind(original.id.to_string()).fetch_one(&mut *tx).await.unwrap();
    assert_eq!(super::treasury::daily_fee_remaining(&mut tx, created + 86_401, 19).await.unwrap(), 9);
    tx.rollback().await.unwrap();
    policy.limits.fee_sats_per_24h = None;
    assert!(store.reserve_cashu_treasury(&registration, &policy, &original_plan).await.unwrap() == original);
    policy.limits.fee_sats_per_24h = Some(20);
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

#[tokio::test]
async fn local_ecash_funds_confirmed_feeding_and_only_legacy_lots_can_be_swept() {
    let (_dir,url,store,policy) = fixture().await;
    let older = wire(&policy, 400);
    store.import_cashu_receipt(&policy, &authenticated(&policy,&older)).await.unwrap();
    let local = wire(&policy, 1000);
    let peer = authenticated(&policy,&local);
    assert!(store.import_cashu_with_custody(&policy,&peer,Some("mint-a")).await.unwrap().retained_cashu);
    let reopened = LedgerStore::connect(&url).await.unwrap();
    assert!(reopened.import_cashu_receipt(&policy,&peer).await.unwrap().retained_cashu);
    assert_eq!(count(&reopened,"retained_cashu_lots").await,1);
    let mut handoff = lightning_goats_cashu_receipt::CollectorHandoff {
        wallet_instance: policy.wallet_instance,
        wallet_epoch: policy.wallet_epoch,
        core_epoch: policy.core_epoch,
        acquisition_id: local.data().acquisition_id,
        evidence_digest: local.data().acquisition_evidence_digest.clone(),
        collected_sats: 999,
    };
    reopened.record_collector_handoff(&handoff).await.unwrap();
    reopened.record_collector_handoff(&handoff).await.unwrap();
    handoff.collected_sats = 1000;
    assert!(reopened.record_collector_handoff(&handoff).await.is_err());
    let mut feeds = Vec::new();
    for spendable in [500, 499] {
        let request = reopened.begin_feed_attempt(500).await.unwrap().unwrap();
        reopened.confirm_feed_attempt(request).await.unwrap();
        feeds.push((request, spendable));
    }
    assert_eq!(reopened.feed_credit_sats().await.unwrap(), 401);
    let mut tx = reopened.pool.begin().await.unwrap();
    let reward_policy = crate::reward_funding::RewardPolicy {
        funding_source: crate::reward_funding::RewardFundingSource::RetainedCashu,
        owner: crate::nostr_event::test_event(1, 1, 100, "operator funding", vec![]).pubkey,
        member_percent_ceiling: 10, max_sats_per_feed: 100, max_sats_per_24h: 1000,
    };
    for (request, spendable) in feeds {
        let funding = crate::ledger::inspect_confirmed_reward_funding(&mut tx, request)
            .await.unwrap().unwrap();
        assert_eq!((funding.strike_sats, funding.retained_cashu_sats), (0, spendable));
        let page = reopened.operator_funding_page(&reward_policy, policy.core_epoch, request, 0).await.unwrap();
        assert_eq!(page.funding, funding);
        assert!(page.reservation.is_none());
        assert!(!page.more);
        assert_eq!(page.allocations.iter().map(|a| a.consumed_sats).sum::<u64>(), spendable);
        let plan_hash = "ab".repeat(32);
        let reserved_sats = if spendable == 500 { 10 } else { 0 };
        let grant = if reserved_sats > 0 {
            Some(reopened.reserve_reward(&reward_policy, policy.core_epoch, request, &plan_hash, reserved_sats).await.unwrap())
        } else { None };
        let remainder = spendable - reserved_sats;
        let hash = if reserved_sats > 0 { [1;32] } else { [2;32] };
        let invoice = crate::test_invoices::invoice_with_payment_hash((remainder-1)*1000, &"cd".repeat(32),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap(), hash);
        let mut terms = crate::payout::operator::Execution {
            allocation: crate::reward_funding::OperatorAllocation {
                epoch: policy.core_epoch, owner: reward_policy.owner.clone(), core_uid: 1, request_id:request,
                plan_hash:plan_hash.clone(), funding_hash:funding.proof_hash.clone(), member_grant:grant.map(|g|g.id),
                collected_sats:spendable, member_reserved_sats:reserved_sats, operator_sats:remainder,
            },
            address:"sat@lightning-goats.com".into(), invoice, payment_hash:hex::encode(hash),
            amount_sat:remainder-1, maximum_fee_sat:1, lots:page.allocations,
        };
        assert!(reopened.claim_operator_execution(&terms,&reward_policy,false).await.is_err());
        terms.lots[0].consumed_sats -= 1;
        assert!(reopened.claim_operator_execution(&terms,&reward_policy,true).await.is_err());
        terms.lots[0].consumed_sats += 1;
        for _ in 0..2 {
            assert_eq!(reopened.claim_operator_execution(&terms,&reward_policy,true).await.unwrap().0,terms.digest().unwrap());
        }
        assert!(reopened.reserve_reward(&reward_policy,policy.core_epoch,request,&"ef".repeat(32),1).await.is_err());

    }
    tx.rollback().await.unwrap();
    assert_eq!(count(&reopened,"cashu_treasury_intents").await,0);
    let retained_id: i64 = sqlx::query_scalar("SELECT ledger_id FROM retained_cashu_lots")
        .fetch_one(&reopened.pool).await.unwrap();
    let direct = crate::ledger::TreasuryPlan {
        id: Uuid::new_v4(),
        mint_id: "mint-a".into(),
        sources: vec![(retained_id, 1000)],
        deposit_sats: 990,
        maximum_fee_sats: 10,
    };
    assert!(reopened.reserve_cashu_treasury(&policy, &feed_treasury_policy(), &direct).await.is_err());
    assert_eq!(count(&reopened,"cashu_treasury_intents").await,0);
    let sweep=crate::cashu::treasury_sweep::Request {id:Uuid::new_v4(),mint_id:"mint-a".into(),maximum_input_sats:1000};
    let planned=reopened.plan_operator_cashu_settlement(&policy,&feed_treasury_policy(),&sweep,false).await.unwrap().unwrap();
    assert_eq!(planned.input_sats,400);
    assert_eq!(planned.sources.len(),1);
    assert_eq!(planned.sources[0].acquisition_id,older.data().acquisition_id);
}

#[tokio::test]
async fn project_treasury_invoice_reopens_without_issuing_or_crediting_again() {
    exercise_conversion_restart(false, false).await;
}

#[tokio::test]
async fn project_invoice_local_nutzap_and_foreign_token_fund_one_confirmed_feed() {
    exercise_conversion_restart(true, false).await;
}

#[tokio::test]
async fn usd_feed_waits_for_original_foreign_melt_and_project_proofs() {
    exercise_conversion_restart(true, true).await;
}

async fn exercise_conversion_restart(mixed:bool, priced: bool) {
    let (dir,url,ledger,mut registration)=fixture().await;
    if priced {
        ledger.initialize_feed_cycles(1000).await.unwrap();
        let now = chrono::DateTime::<chrono::Utc>::from(std::time::SystemTime::now());
        let body = json!({"market":{"source":crate::feeding_price::SOURCE,
            "observed_at":now.to_rfc3339(),"price_usd_exact":"100000"}});
        let observation = crate::feeding_price::Observation::from_cache(
            &serde_json::to_vec(&body).unwrap(), now.timestamp_millis()).unwrap();
        ledger.refresh_feed_price(&observation).await.unwrap();
    }
    if mixed {
        // Start at the authenticated settlement boundary used by the normal poller.
        let id=Uuid::new_v4();
        let hash=hex::encode([0x41;32]);
        let description_hash="cf".repeat(32);
        let invoice=crate::test_invoices::invoice_with_payment_hash(600000,&description_hash,
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap(),[0x41;32]);
        ledger.prepare_breez_invoice(id,&json!({"amount_sat":600,"user":"dexter","credit_pool":"herd","zap":false})).await.unwrap();
        ledger.bind_breez_invoice(id,&invoice,&hash).await.unwrap();
        let collector=crate::config::BreezCollectorConfig{socket:dir.path().join("viewer.sock"),peer_uid:crate::private_ipc::uid().unwrap()};
        let listener=tokio::net::UnixListener::bind(&collector.socket).unwrap();
        let server=tokio::spawn(async move {
            let (mut socket,_)=listener.accept().await.unwrap();
            let request:serde_json::Value=crate::private_ipc::read(&mut socket,65536).await.unwrap();
            assert_eq!(request,json!({"claim_status":id}));
            crate::private_ipc::write(&mut socket,&json!({"request_id":id,"payment_hash":hash,"amount_sat":600,"proof_digest":"bc".repeat(32)}),65536).await.unwrap();
        });
        assert!(ledger.poll_breez_claim(&collector).await.unwrap());
        server.await.unwrap();
        assert_eq!(ledger.feed_credit_sats().await.unwrap(),600);
        registration.approved.insert(("goats".into(),"v1".into()),MintImportLimit{maximum_gross_sats:1000,maximum_net_sats:1000});
        let mut nutzap=wire(&registration,249).data().clone();
        nutzap.mint_id="goats".into();
        nutzap.transport="nutzap".into();
        nutzap.source_ref="1c".repeat(32);
        nutzap.source_author=Some(crate::nostr_event::test_event(3,1,100,"",vec![]).pubkey);
        let nutzap=WireReceipt::validate(nutzap).unwrap();
        let peer=authenticated(&registration,&nutzap);
        let ack=ledger.import_cashu_with_custody(&registration,&peer,Some("goats")).await.unwrap();
        assert_eq!(ack.net_sats,249);
        assert_eq!(ledger.import_cashu_with_custody(&registration,&peer,Some("goats")).await.unwrap(),ack);
        ledger.record_collector_handoff(&lightning_goats_cashu_receipt::CollectorHandoff{
            wallet_instance:registration.wallet_instance,wallet_epoch:registration.wallet_epoch,core_epoch:registration.core_epoch,
            acquisition_id:ack.acquisition_id,evidence_digest:nutzap.data().acquisition_evidence_digest.clone(),collected_sats:249
        }).await.unwrap();
        assert_eq!(ledger.feed_credit_sats().await.unwrap(),850);
    }
    let foreign_net=if mixed {150} else {1000};
    let receipt=wire(&registration,foreign_net);
    let foreign=authenticated(&registration,&receipt);
    let result=ledger.import_cashu_receipt(&registration,&foreign).await.unwrap();
    assert_eq!(ledger.import_cashu_receipt(&registration,&foreign).await.unwrap(),result);
    assert_eq!(ledger.feed_credit_sats().await.unwrap(),1000);
    let mut policy = feed_treasury_policy();
    policy.destination.origin = "https://mint.lightning-goats.com/".into();
    assert!(policy.validate().is_err());
    policy.invoice_expiry_seconds = 900;
    let sweep = crate::cashu::treasury_sweep::Request {
        id: Uuid::new_v4(), mint_id: "mint-a".into(), maximum_input_sats: 1000,
    };
    let intent = ledger.plan_operator_cashu_settlement(&registration, &policy, &sweep, true)
        .await.unwrap().unwrap();
    use crate::cashu::treasury_worker::{Controls, Stage};
    let controls = Controls {issue:true, deliver:true, inspect:true, maximum_attempts:3, ..Default::default()};
    assert!(ledger.claim_cashu_treasury_work(&registration, &controls, false, 100, 10).await.unwrap().is_none());
    let issue = ledger.claim_cashu_treasury_work(&registration, &controls, true, 100, 10).await.unwrap().unwrap();
    assert_eq!((issue.id,issue.stage),(intent.id,Stage::Issue));
    let collector = crate::config::BreezCollectorConfig {
        socket: dir.path().join("collector.sock"), peer_uid: crate::private_ipc::uid().unwrap(),
    };
    let digest = lightning_goats_cashu_receipt::treasury::hex_digest(intent.digest().unwrap());
    let invoice = crate::test_invoices::invoice_with_expiry(intent.deposit_sats * 1000, &digest,
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap(),
        [0x53; 32], std::time::Duration::from_secs(900));
    let expected = serde_json::json!({"treasury_id":intent.id,"amount_sat":intent.deposit_sats,"description_hash":digest});
    let reply = serde_json::json!({"invoice":invoice});
    let listener = tokio::net::UnixListener::bind(&collector.socket).unwrap();
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let request: serde_json::Value = crate::private_ipc::read(&mut stream, 65536).await.unwrap();
        assert_eq!(request, expected);
        crate::private_ipc::write(&mut stream, &reply, 65536).await.unwrap();
    });
    let issued = ledger.issue_breez_treasury_invoice(&collector, &registration, &policy, intent.id).await.unwrap();
    server.await.unwrap();
    assert_eq!(issued.invoice, invoice);
    assert_eq!(ledger.cashu_treasury_invoice(intent.id).await.unwrap().unwrap().invoice, invoice);
    let deliver = ledger.claim_cashu_treasury_work(&registration,&controls,true,101,10).await.unwrap().unwrap();
    assert_eq!((deliver.id,deliver.stage),(intent.id,Stage::Deliver));
    ledger.close_cashu_treasury_delivery(intent.id,true).await.unwrap();
    let inspect = ledger.claim_cashu_treasury_work(&registration,&controls,true,117,10).await.unwrap().unwrap();
    assert_eq!((inspect.id,inspect.stage),(intent.id,Stage::Inspect));
    let fee = intent.maximum_fee_sats.min(1);
    let change = intent.input_sats - intent.deposit_sats - fee;
    let report = lightning_goats_cashu_receipt::treasury::MeltData {
        version: 1, intent_id: intent.id, intent_digest: digest,
        quote_id: "original-foreign-mint-quote".into(), operation_evidence_digest: "ef".repeat(32),
        sources: intent.allocate_result(fee, change).unwrap(),
    };
    let report = crate::cashu::treasury::PeerMelt::authenticate(&registration,
        &crate::cashu::receipt_authority::fixture_peer(&registration),
        &serde_json::to_vec(&report).unwrap()).unwrap();
    assert!(!ledger.record_cashu_melt_result(&registration, &report).await.unwrap());
    assert_eq!(count(&ledger, "breez_treasury_source_results").await, 0);
    assert!(ledger.claim_cashu_treasury_work(&registration,&controls,true,1000,10).await.unwrap().is_none());
    drop(ledger);
    let ledger = LedgerStore::connect(&url).await.unwrap();
    assert_eq!(ledger.issue_breez_treasury_invoice(&collector, &registration, &policy, intent.id)
        .await.unwrap().invoice, invoice);
    if collector.socket.exists() { std::fs::remove_file(&collector.socket).unwrap(); }
    let listener = tokio::net::UnixListener::bind(&collector.socket).unwrap();
    let lookup = serde_json::json!({"treasury_claim_status":intent.id});
    let reply = serde_json::json!({"request_id":intent.id, "payment_hash":issued.payment_hash,
                                  "amount_sat":intent.deposit_sats, "proof_digest":"ab".repeat(32)});
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let request: serde_json::Value = crate::private_ipc::read(&mut stream, 65536).await.unwrap();
        assert_eq!(request, lookup);
        crate::private_ipc::write(&mut stream, &reply, 65536).await.unwrap();
    });
    assert!(ledger.begin_feed_attempt(1000).await.unwrap().is_none());
    assert!(ledger.poll_breez_claim(&collector).await.unwrap());
    server.await.unwrap();
    assert!(ledger.record_cashu_melt_result(&registration, &report).await.unwrap());
    let amounts: (i64,i64,i64) = sqlx::query_as("SELECT deposit_sats,fee_sats,change_sats FROM breez_treasury_source_results WHERE intent_id=?")
        .bind(intent.id.to_string()).fetch_one(&ledger.pool).await.unwrap();
    assert_eq!(amounts, (intent.deposit_sats as i64, fee as i64, change as i64));
    let mut tx = ledger.pool.begin().await.unwrap();
    assert_eq!(super::treasury::available_custody(&mut tx,intent.sources[0].ledger_id).await.unwrap(), change);
    let now: i64 = sqlx::query_scalar("SELECT unixepoch()").fetch_one(&mut *tx).await.unwrap();
    assert_eq!(super::treasury::daily_fee_remaining(&mut tx,now,100).await.unwrap(), 100-fee);
    tx.rollback().await.unwrap();
    assert_eq!(count(&ledger, "breez_collected_claims").await, 1+i64::from(mixed));
    assert!(!ledger.poll_breez_claim(&collector).await.unwrap());
    assert!(ledger.record_payment(&SettledPayment {
        source: "strike".into(), source_id: Uuid::new_v4().to_string(),
        payment_hash: Some(issued.payment_hash), address_user: "dexter".into(),
        credit_pool: "herd".into(), amount_msat: intent.deposit_sats * 1000,
        settled_at: None, context_json: None,
    }).await.is_err());
    assert_eq!(count(&ledger, "settled_payments").await, i64::from(mixed));
    assert_eq!(ledger.feed_credit_sats().await.unwrap(), 1000);
    let range: (i64,i64,i64) = sqlx::query_as("SELECT start_sats,input_sats,deposit_sats FROM cashu_funding_ranges WHERE intent_id=?")
        .bind(intent.id.to_string()).fetch_one(&ledger.pool).await.unwrap();
    assert_eq!(range, (0, foreign_net as i64, intent.deposit_sats as i64));
    let feed = ledger.begin_feed_attempt(1000).await.unwrap().unwrap();
    ledger.confirm_feed_attempt(feed).await.unwrap();
    if priced {
        assert_eq!(count(&ledger, "feed_cycle_attempts").await, 1);
        assert_eq!(count(&ledger, "feed_cycle_plans").await, 1);
        assert_eq!(ledger.feed_goal().await.unwrap().unwrap().funded_count, 0);
    }
    let mut tx = ledger.pool.begin().await.unwrap();
    let funding = crate::ledger::inspect_confirmed_reward_funding(&mut tx,feed).await.unwrap().unwrap();
    assert_eq!((funding.strike_sats, funding.retained_cashu_sats),(0,intent.deposit_sats+if mixed {849} else {0}));
    assert_eq!(ledger.feed_credit_sats().await.unwrap(),0);
    assert!(ledger.begin_feed_attempt(1000).await.unwrap().is_none());
    assert_eq!(count(&ledger,"feed_attempts").await,1);
    tx.rollback().await.unwrap();
}
