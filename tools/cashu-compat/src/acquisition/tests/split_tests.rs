//! Real CDK standalone splits, including nonzero fees and lost-response recovery.
use super::*;

fn wallet_for(
    gate: OperationGate<FixtureMint>,
    coordinator: &Coordinator<'_, FixtureMint>,
    seed: [u8; 64],
) -> Result<Wallet> {
    Ok(WalletBuilder::new()
        .mint_url(coordinator.mint_url.clone())
        .unit(CurrencyUnit::Sat)
        .localstore(coordinator.database.clone())
        .seed(seed)
        .client(CustomHttpClient::with_transport(
            coordinator.mint_url.clone(),
            gate,
            None,
        ))
        .without_rate_limiting()
        .use_http_subscription()
        .build()?)
}
#[tokio::test]
async fn real_cdk_split_accounts_nonzero_preparation_and_future_melt_fees() -> Result<()> {
    split_case(false, false, false, false).await
}
#[tokio::test]
async fn lost_split_reply_reopens_paused_and_restores_without_a_second_swap() -> Result<()> {
    split_case(true, false, false, false).await
}
#[tokio::test]
async fn canceled_split_before_cdk_cannot_allocate_a_fresh_operation_after_restart() -> Result<()> {
    split_case(false, true, false, false).await
}
#[tokio::test]
async fn split_executor_commits_original_owned_outputs_without_another_contribution() -> Result<()>
{
    split_case(false, false, true, false).await
}
#[tokio::test]
async fn split_executor_reopens_paused_and_recovers_only_original_operation() -> Result<()> {
    split_case(true, false, true, false).await
}
#[tokio::test]
async fn split_worker_progresses_only_durable_stages_and_pauses_spending() -> Result<()> {
    split_case(false, false, true, true).await
}
#[tokio::test]
async fn split_worker_recovers_lost_reply_while_execution_is_paused() -> Result<()> {
    split_case(true, false, true, true).await
}
async fn split_case(
    lost_reply: bool,
    cancel_before_cdk: bool,
    use_executor: bool,
    use_worker: bool,
) -> Result<()> {
    let harness = Harness::new(100)?;
    let mut coordinator = harness.open().await?;
    // Keep one large proof so exact selection cannot satisfy the smaller intent.
    coordinator.wallet.set_target_proof_count(0);
    let body = serde_json::to_vec(
        &json!({"id":"reusable","mint":harness.policy().advertised_url,"unit":"sat",
        "proofs":[harness.mint.issue_amount("split-source-sixteen".into(),16),harness.mint.issue_amount("split-source-one".into(),1)]}),
    )?;
    let contribution = ordinary_json(
        &body,
        "split-source",
        &["reusable".into()],
        &MintRegistry::new(vec![harness.policy()])?,
        &harness.keys,
        &Limits {
            body_bytes: 65536,
            proof_count: 8,
            receipt_sats: 10000,
        },
        harness.receiver.public_key(),
    )?;
    let acquisition = new_id(coordinator.admit(&contribution)?);
    let acquired = receipt(
        coordinator
            .acquire_new(acquisition, &harness.receiver)
            .await?,
    );
    assert_eq!(acquired.net_sats, 16);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs();
    let (mut binding, mut issued) = crate::treasury::tests::fixture();
    binding.wallet_epoch = harness.epoch;
    binding.mint_id = harness.policy().id;
    issued.intent.wallet_epoch = binding.wallet_epoch;
    issued.intent.mint_id = binding.mint_id.clone();
    issued.intent.input_sats = 5;
    issued.intent.deposit_sats = 3;
    issued.intent.maximum_fee_sats = 2;
    issued.intent.sources[0].acquisition_id = acquisition;
    issued.intent.sources[0].input_sats = 5;
    issued.intent.sources[0].receipt_digest = lightning_goats_cashu_receipt::treasury::hex_digest(
        crate::receipt_wire::from_private(binding.wallet_instance, &acquired)?.digest()?,
    );
    issued.invoice = crate::treasury::tests::signed_invoice_hash(
        cdk::lightning_invoice::Currency::Bitcoin,
        3000,
        now,
        300,
        "ab",
    );
    if use_worker {
        use sha2::{Digest, Sha256};
        issued.payment_hash =
            lightning_goats_cashu_receipt::treasury::hex_digest(Sha256::digest([55u8; 32]).into());
        issued.invoice = crate::treasury::tests::signed_invoice_payment_hash(
            cdk::lightning_invoice::Currency::Bitcoin,
            3000,
            now,
            300,
            &issued.payment_hash,
        );
    }
    let all = coordinator.wallet.get_unspent_proofs().await?;
    let proofs = all
        .into_iter()
        .filter(|p| u64::from(p.amount) > 1)
        .collect::<Vec<_>>();
    assert_eq!(proofs.iter().map(|p| u64::from(p.amount)).sum::<u64>(), 16);
    let input_ys = proofs
        .iter()
        .map(|p| p.y())
        .collect::<Result<Vec<_>, _>>()?;
    let mut canonical_ys = input_ys.iter().map(|y| y.to_hex()).collect::<Vec<_>>();
    canonical_ys.sort();
    let limits = crate::treasury_split::Limits {
        maximum_input_sats: 16,
        maximum_fee_sats: 1,
        maximum_outputs: 8,
    };
    let reservation = {
        let mut journal = coordinator.journal.lock().unwrap();
        journal.bind_treasury_instruction(&binding, &issued, now)?;
        journal.reserve_treasury_sources(&binding, issued.intent.id, now)?;
        assert!(
            journal
                .select_treasury_proofs(&binding, issued.intent.id, now)
                .is_err()
        );
        if use_executor {
            None
        } else {
            Some(journal.reserve_treasury_split(
                &binding,
                issued.intent.id,
                &crate::journal::treasury_split::Request {
                    split_id: Uuid::from_u128(500),
                    input_ys: canonical_ys,
                    fee_sats: 1,
                    denominations: vec![1, 2, 4, 8, 16],
                },
                limits,
                now,
            )?)
        }
    };
    let policy = Arc::new(crate::operation_gate::TreasuryPolicy {
        binding: binding.clone(),
        mint: harness.policy(),
        seed: harness.seed,
    });
    let admission = crate::journal::treasury_split::AdmissionPolicy {
        enabled: true,
        limits,
        operations_per_24h: 1,
        input_sats_per_24h: 16,
        maximum_in_flight_sats: 16,
    };
    if use_worker {
        drop(coordinator);
        return worker_case(
            &harness,
            policy,
            admission,
            &issued,
            acquisition,
            lost_reply,
        )
        .await;
    }
    if use_executor {
        drop(coordinator);
        return executor_case(
            &harness,
            policy,
            admission,
            &issued,
            acquisition,
            lost_reply,
        )
        .await;
    }
    let reservation = reservation.expect("direct transport fixture reservation");
    let mut gate = coordinator
        .gate
        .clone()
        .with_treasury_policy(policy.clone())
        .with_split_policy(Arc::new(admission.clone()))?;
    let mut wallet = wallet_for(gate.clone(), &coordinator, harness.seed)?;
    *harness.mint.quote_reply.lock().unwrap() = Some(
        json!({"quote":"split-original-quote","amount":3,"fee_reserve":0,
        "state":"UNPAID","expiry":now+250,"payment_preimage":null,"change":null,
        "request":issued.invoice,"unit":"sat","method":"bolt11"}),
    );
    // Missing quote refuses preparation without consuming its one durable claim.
    assert!(gate.arm_split(issued.intent.id).is_err());
    {
        let _lease = gate.arm_treasury(issued.intent.id)?;
        wallet
            .melt_quote(
                cdk::nuts::PaymentMethod::BOLT11,
                issued.invoice.clone(),
                None,
                None,
            )
            .await?;
    }
    let lease = gate.arm_split(issued.intent.id)?;
    assert!(gate.arm_treasury(issued.intent.id).is_err());
    assert!(coordinator.gate.arm(acquisition, false, 1).is_err());
    if cancel_before_cdk {
        drop(lease);
        assert!(gate.arm_split(issued.intent.id).is_err());
        drop(wallet);
        drop(gate);
        drop(coordinator);
        coordinator = harness.open().await?;
        gate = coordinator
            .gate
            .clone()
            .with_treasury_policy(policy.clone())
            .with_split_policy(Arc::new(admission))?;
        assert!(gate.arm_split(issued.intent.id).is_err());
        assert!(gate.arm_split_recovery(issued.intent.id).is_err());
        assert!(
            coordinator
                .database
                .get_incomplete_sagas()
                .await?
                .is_empty()
        );
        assert_eq!(harness.mint.executions.load(Ordering::SeqCst), 1);
        assert_eq!(harness.mint.quote_attempts.load(Ordering::SeqCst), 1);
        assert_eq!(harness.mint.melt_attempts.load(Ordering::SeqCst), 0);
        assert!(
            coordinator
                .journal
                .lock()
                .unwrap()
                .select_treasury_proofs(&binding, issued.intent.id, now)
                .is_err()
        );
        return Ok(());
    }
    harness.mint.lose_reply.store(lost_reply, Ordering::SeqCst);
    let target = cdk::amount::SplitTarget::Values(
        reservation
            .plan()
            .outputs()
            .iter()
            .map(|o| o.amount.into())
            .collect(),
    );
    let result = wallet.swap(None, target, proofs, None, false, false).await;
    if lost_reply {
        assert!(result.is_err());
    } else {
        result?;
    }
    drop(lease);
    assert_eq!(harness.mint.executions.load(Ordering::SeqCst), 2); // acquisition + one split
    assert!(gate.arm_split(issued.intent.id).is_err());
    if lost_reply {
        drop(wallet);
        drop(gate);
        drop(coordinator);
        coordinator = harness.open().await?;
        let mut paused = admission.clone();
        paused.enabled = false;
        gate = coordinator
            .gate
            .clone()
            .with_treasury_policy(policy.clone())
            .with_split_policy(Arc::new(paused))?;
        wallet = wallet_for(gate.clone(), &coordinator, harness.seed)?;
        let _recovery = gate.arm_split_recovery(issued.intent.id)?;
        let sagas = coordinator.database.get_incomplete_sagas().await?;
        assert_eq!(sagas.len(), 1);
        let original = coordinator
            .journal
            .lock()
            .unwrap()
            .treasury_split_derivation(&binding, issued.intent.id)?;
        assert_eq!(sagas[0].id, original.cdk_operation);
        wallet.recover_incomplete_sagas().await?;
        assert!(
            coordinator
                .database
                .get_incomplete_sagas()
                .await?
                .is_empty()
        );
    }
    assert_eq!(harness.mint.executions.load(Ordering::SeqCst), 2);
    assert_eq!(harness.mint.quote_attempts.load(Ordering::SeqCst), 1);
    assert_eq!(harness.mint.melt_attempts.load(Ordering::SeqCst), 0);
    let derivation = coordinator
        .journal
        .lock()
        .unwrap()
        .treasury_split_derivation(&binding, issued.intent.id)?;
    let output_ys = derivation.output_ys(&harness.seed)?;
    let owned = coordinator
        .database
        .get_proofs_by_ys(output_ys.clone())
        .await?;
    let fresh_inputs: CheckStateResponse = harness
        .mint
        .http_post(
            coordinator.mint_url.join("v1/checkstate")?,
            None,
            &cdk::nuts::CheckStateRequest { ys: input_ys },
        )
        .await?;
    let fresh_outputs: CheckStateResponse = harness
        .mint
        .http_post(
            coordinator.mint_url.join("v1/checkstate")?,
            None,
            &cdk::nuts::CheckStateRequest { ys: output_ys },
        )
        .await?;
    let mut journal = coordinator.journal.lock().unwrap();
    let verified = journal.verify_treasury_split_outputs(
        &binding,
        issued.intent.id,
        &harness.seed,
        &harness.keys,
        &owned,
        &fresh_inputs,
        &fresh_outputs,
    )?;
    assert_eq!(verified.input_sats(), 16);
    assert_eq!(verified.fee_sats(), 1);
    assert_eq!(verified.outputs().iter().map(|o| o.amount).sum::<u64>(), 15);
    assert!(verified.commit(&mut journal, &binding)?);
    let selected = journal.select_treasury_proofs(&binding, issued.intent.id, now)?;
    assert_eq!(selected[&acquisition].len(), 1); // four sats remain for melt
    let after = journal.export_receipt(acquisition)?;
    assert_eq!(
        lightning_goats_cashu_receipt::treasury::hex_digest(
            crate::receipt_wire::from_private(binding.wallet_instance, &after)?.digest()?
        ),
        issued.intent.sources[0].receipt_digest
    );
    Ok(())
}

async fn executor_case(
    harness: &Harness,
    policy: Arc<crate::operation_gate::TreasuryPolicy>,
    admission: crate::journal::treasury_split::AdmissionPolicy,
    issued: &lightning_goats_cashu_receipt::treasury::Issued,
    acquisition: Uuid,
    lost_reply: bool,
) -> Result<()> {
    let writer = Arc::new(WriterLock::acquire(harness.directory.path())?);
    let mut database = crate::wallet::open_wallet_storage(harness.directory.path()).await?;
    let path = harness.directory.path().join("journal.sqlite");
    let mut journal = Arc::new(Mutex::new(Journal::open(&path, harness.epoch, 8)?));
    let mut executor = crate::treasury_execution::Executor::new(
        harness.mint.clone(),
        policy.clone(),
        database.clone(),
        journal.clone(),
        writer.clone(),
        harness.keys.clone(),
        65536,
    )?;
    assert!(executor.execute_split(issued.intent.id).await.is_err());
    assert!(executor.reserve_split(issued.intent.id).await.is_err());
    executor = executor.with_split_policy(Arc::new(admission.clone()))?;
    let reservation = executor.reserve_split(issued.intent.id).await?;
    assert_eq!(reservation.plan().fee_sats(), 1);
    assert_eq!(
        reservation
            .plan()
            .outputs()
            .iter()
            .map(|o| o.amount)
            .sum::<u64>(),
        15
    );
    assert_eq!(
        executor.reserve_split(issued.intent.id).await?.split_id(),
        reservation.split_id()
    );
    assert_eq!(harness.mint.executions.load(Ordering::SeqCst), 1);
    assert_eq!(harness.mint.quote_attempts.load(Ordering::SeqCst), 0);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs();
    *harness.mint.quote_reply.lock().unwrap() = Some(json!({
        "quote":"executor-original-quote","amount":3,"fee_reserve":0,"state":"UNPAID",
        "expiry":now+250,"payment_preimage":null,"change":null,"request":issued.invoice,
        "unit":"sat","method":"bolt11"
    }));
    executor.quote_reserved(issued.intent.id).await?;
    harness.mint.lose_reply.store(lost_reply, Ordering::SeqCst);
    let result = executor.execute_split(issued.intent.id).await;
    if lost_reply {
        assert!(result.is_err());
    } else {
        result?;
    }
    assert!(executor.execute_split(issued.intent.id).await.is_err());
    drop(executor);
    drop(writer);
    drop(database);
    drop(journal);
    let writer = Arc::new(WriterLock::acquire(harness.directory.path())?);
    database = crate::wallet::open_wallet_storage(harness.directory.path()).await?;
    journal = Arc::new(Mutex::new(Journal::open(&path, harness.epoch, 8)?));
    let mut paused = admission;
    paused.enabled = false;
    executor = crate::treasury_execution::Executor::new(
        harness.mint.clone(),
        policy.clone(),
        database.clone(),
        journal.clone(),
        writer,
        harness.keys.clone(),
        65536,
    )?
    .with_split_policy(Arc::new(paused))?;
    assert!(executor.execute_split(issued.intent.id).await.is_err());
    if lost_reply {
        let original = database.get_incomplete_sagas().await?;
        assert_eq!(original.len(), 1);
        let mut unrelated = original[0].clone();
        unrelated.id = Uuid::new_v4();
        database.add_saga(unrelated.clone()).await?;
        assert!(executor.reconcile_split(issued.intent.id).await.is_err());
        assert_eq!(database.get_incomplete_sagas().await?.len(), 2);
        database.delete_saga(&unrelated.id).await?;
        // A conflicting original saga must also remain untouched.
        let mut changed = original[0].clone();
        changed.amount = 1u64.into();
        changed.update_state(changed.state);
        assert!(database.update_saga(changed).await?);
        assert!(executor.reconcile_split(issued.intent.id).await.is_err());
        let mut restore = database.get_saga(&original[0].id).await?.unwrap();
        restore.amount = original[0].amount;
        restore.update_state(restore.state);
        assert!(database.update_saga(restore).await?);
        let mut quote = database
            .get_melt_quote("executor-original-quote")
            .await?
            .unwrap();
        let unrelated_owner = Uuid::new_v4().to_string();
        quote.used_by_operation = Some(unrelated_owner.clone());
        database.add_melt_quote(quote.clone()).await?;
        assert!(executor.reconcile_split(issued.intent.id).await.is_err());
        assert_eq!(
            database
                .get_melt_quote(&quote.id)
                .await?
                .unwrap()
                .used_by_operation,
            Some(unrelated_owner)
        );
        quote = database.get_melt_quote(&quote.id).await?.unwrap();
        quote.used_by_operation = None;
        database.add_melt_quote(quote).await?;
    }
    assert!(executor.reconcile_split(issued.intent.id).await?);
    assert!(executor.reconcile_split(issued.intent.id).await?);
    assert!(database.get_incomplete_sagas().await?.is_empty());
    assert_eq!(harness.mint.executions.load(Ordering::SeqCst), 2);
    assert_eq!(harness.mint.quote_attempts.load(Ordering::SeqCst), 1);
    assert_eq!(harness.mint.melt_attempts.load(Ordering::SeqCst), 0);
    {
        let health_db = rusqlite::Connection::open(&path)?;
        let health = crate::status::Funds::read(&health_db, None, i64::try_from(now)?)?;
        assert_eq!(health.acquired_net_sats, 16);
        assert_eq!(health.journal_custody_sats, 15);
        assert_eq!(health.split_preparation_fee_sats, 1);
        assert_eq!(health.unsettled_preparation_fee_sats, 1);
        assert_eq!(health.completed_conversion_fee_sats, 0);
        assert_eq!(health.total_accounted_fee_sats, health.receive_fee_sats + 1);
        let mut locked = journal.lock().unwrap();
        let selected = locked.select_treasury_proofs(&policy.binding, issued.intent.id, now)?;
        assert_eq!(selected[&acquisition].len(), 1);
        let after = locked.export_receipt(acquisition)?;
        assert_eq!(
            lightning_goats_cashu_receipt::treasury::hex_digest(
                crate::receipt_wire::from_private(policy.binding.wallet_instance, &after)?
                    .digest()?
            ),
            issued.intent.sources[0].receipt_digest
        );
    }
    let prepared = executor.prepare_reserved(issued.intent.id).await?;
    let context = journal.lock().unwrap().unsubmitted_treasury_preparation(
        &policy.binding,
        issued.intent.id,
        now,
    )?;
    assert_eq!(context.operation, prepared);
    assert_eq!(context.input_fee, 1);
    assert_eq!(context.ys.len(), 1);
    assert_eq!(harness.mint.executions.load(Ordering::SeqCst), 2);
    assert_eq!(harness.mint.quote_attempts.load(Ordering::SeqCst), 1);
    assert_eq!(harness.mint.melt_attempts.load(Ordering::SeqCst), 0);
    Ok(())
}

async fn worker_case(
    harness: &Harness,
    policy: Arc<crate::operation_gate::TreasuryPolicy>,
    admission: crate::journal::treasury_split::AdmissionPolicy,
    issued: &lightning_goats_cashu_receipt::treasury::Issued,
    acquisition: Uuid,
    lost_reply: bool,
) -> Result<()> {
    use crate::treasury_scheduler::{Execution, Tick, Worker};
    let writer = Arc::new(WriterLock::acquire(harness.directory.path())?);
    let database = crate::wallet::open_wallet_storage(harness.directory.path()).await?;
    let path = harness.directory.path().join("journal.sqlite");
    let make_worker = |execution| {
        Worker::from_parts(
            Arc::new(Mutex::new(Journal::open(&path, harness.epoch, 8)?)),
            database.clone(),
            writer.clone(),
            harness.keys.clone(),
            vec![(policy.clone(), harness.mint.clone())],
            execution,
            std::time::Duration::from_secs(10),
            65536,
        )
    };
    let mut execution = Execution {
        execute: true,
        reconcile: true,
        maximum_attempts: 4,
        maximum_input_sats: 5,
        maximum_fee_sats: 2,
        operations_per_24h: 1,
        input_sats_per_24h: 5,
        unlimited_input_sats_per_24h: false,
        fee_sats_per_24h: Some(2),
        maximum_in_flight_sats: 5,
        split: Default::default(),
    };
    let mut now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs();
    let worker = make_worker(execution.clone())?;
    assert_eq!(worker.tick(now as i64).await?, Tick::RetryRetained);
    assert_eq!(harness.mint.quote_attempts.load(Ordering::SeqCst), 0);
    drop(worker);
    execution.split = admission;
    let worker = make_worker(execution.clone())?;
    assert_eq!(worker.tick(now as i64 + 15).await?, Tick::Progress); // reserve split
    *harness.mint.quote_reply.lock().unwrap() = Some(json!({"quote":"scheduled-original-quote",
        "amount":3,"fee_reserve":0,"state":"UNPAID","expiry":now+250,
        "payment_preimage":null,"change":null,"request":issued.invoice,"unit":"sat","method":"bolt11"}));
    assert_eq!(worker.tick(now as i64 + 16).await?, Tick::Progress); // original quote
    drop(worker);
    let mut paused = execution.clone();
    paused.execute = false;
    let worker = make_worker(paused.clone())?;
    assert_eq!(worker.tick(now as i64 + 17).await?, Tick::Idle);
    drop(worker);
    let worker = make_worker(execution.clone())?;
    harness.mint.lose_reply.store(lost_reply, Ordering::SeqCst);
    assert_eq!(
        worker.tick(now as i64 + 18).await?,
        if lost_reply {
            Tick::RetryRetained
        } else {
            Tick::Progress
        }
    );
    drop(worker);
    paused.split.enabled = false;
    let worker = make_worker(paused.clone())?;
    assert_eq!(worker.tick(now as i64 + 19).await?, Tick::Progress); // recover only original
    assert_eq!(worker.tick(now as i64 + 20).await?, Tick::Idle); // selection/spending remains paused
    drop(worker);
    let worker = make_worker(execution.clone())?;
    assert_eq!(worker.tick(now as i64 + 21).await?, Tick::Progress); // select verified split proofs
    // A conflicting CDK quote must not consume the one-time preparation claim.
    let mut conflict = database
        .get_melt_quote("scheduled-original-quote")
        .await?
        .unwrap();
    conflict.amount = 9u64.into();
    database.add_melt_quote(conflict).await?;
    assert_eq!(worker.tick(now as i64 + 22).await?, Tick::RetryRetained);
    let fixture = rusqlite::Connection::open(&path)?;
    let claims: i64 = fixture.query_row(
        "SELECT COUNT(*) FROM treasury_preparation_claims",
        [],
        |r| r.get(0),
    )?;
    assert_eq!(claims, 0);
    drop(fixture);
    assert_eq!(
        u64::from(
            database
                .get_melt_quote("scheduled-original-quote")
                .await?
                .unwrap()
                .amount
        ),
        9
    );
    // Model the journal/CDK crash boundary: the retained journal quote exists,
    // but CDK's row does not. Reconstruction must reuse the original quote.
    database
        .remove_melt_quote("scheduled-original-quote")
        .await?;
    drop(worker);
    let worker = make_worker(execution.clone())?;
    now += 15; // durable retry backoff
    assert_eq!(worker.tick(now as i64 + 22).await?, Tick::Progress); // restore and prepare original quote
    drop(worker);
    let worker = make_worker(paused.clone())?;
    assert_eq!(worker.tick(now as i64 + 23).await?, Tick::Idle);
    assert_eq!(harness.mint.executions.load(Ordering::SeqCst), 2);
    assert_eq!(harness.mint.quote_attempts.load(Ordering::SeqCst), 1);
    assert_eq!(harness.mint.melt_attempts.load(Ordering::SeqCst), 0);
    let mut journal = Journal::open(&path, harness.epoch, 8)?;
    let prepared =
        journal.unsubmitted_treasury_preparation(&policy.binding, issued.intent.id, now)?;
    assert_eq!(prepared.input_fee, 1);
    assert_eq!(prepared.ys.len(), 1);
    let after = journal.export_receipt(acquisition)?;
    assert_eq!(
        lightning_goats_cashu_receipt::treasury::hex_digest(
            crate::receipt_wire::from_private(policy.binding.wallet_instance, &after)?.digest()?
        ),
        issued.intent.sources[0].receipt_digest
    );
    drop(worker);
    drop(journal);
    let mut paid = harness.mint.quote_reply.lock().unwrap().clone().unwrap();
    paid["state"] = "PAID".into();
    paid["payment_preimage"] = "37".repeat(32).into();
    paid["change"] = json!([]);
    *harness.mint.melt_paid_reply.lock().unwrap() = Some(paid);
    harness
        .mint
        .lose_melt_reply
        .store(lost_reply, Ordering::SeqCst);
    harness
        .mint
        .melt_status_unavailable
        .store(lost_reply, Ordering::SeqCst);
    let worker = make_worker(execution)?;
    assert_eq!(
        worker.tick(now as i64 + 24).await?,
        if lost_reply {
            Tick::RetryRetained
        } else {
            Tick::Progress
        }
    );
    assert!(harness.mint.melt_settled.load(Ordering::SeqCst));
    drop(worker);
    harness
        .mint
        .melt_status_unavailable
        .store(false, Ordering::SeqCst);
    let worker = make_worker(paused)?;
    assert_eq!(worker.tick(now as i64 + 25).await?, Tick::Progress);
    let before_cleanup = Journal::open(&path, harness.epoch, 8)?
        .treasury_melt_report(&policy.binding, issued.intent.id)?;
    if lost_reply {
        let sagas = database.get_incomplete_sagas().await?;
        assert_eq!(sagas.len(), 1);
        let mut unrelated = sagas[0].clone();
        unrelated.id = Uuid::new_v4();
        database.add_saga(unrelated.clone()).await?;
        harness.mint.fail_check_once.store(true, Ordering::SeqCst);
        assert_eq!(worker.tick(now as i64 + 26).await?, Tick::RetryRetained);
        assert!(database.get_saga(&sagas[0].id).await?.is_some());
        {
            let conn = rusqlite::Connection::open(&path)?;
            conn.execute_batch("CREATE TRIGGER refuse_cleanup BEFORE INSERT ON treasury_cdk_cleanup BEGIN SELECT RAISE(ABORT,'synthetic cleanup checkpoint failure'); END;")?;
        }
        assert_eq!(worker.tick(now as i64 + 41).await?, Tick::RetryRetained);
        assert!(database.get_saga(&sagas[0].id).await?.is_none());
        assert!(database.get_saga(&unrelated.id).await?.is_some());
        assert!(
            Journal::open(&path, harness.epoch, 8)?
                .treasury_melt_report(&policy.binding, issued.intent.id)?
                == before_cleanup
        );
        {
            let conn = rusqlite::Connection::open(&path)?;
            conn.execute_batch("DROP TRIGGER refuse_cleanup;")?;
        }
        assert_eq!(worker.tick(now as i64 + 61).await?, Tick::Progress);
        assert_eq!(worker.tick(now as i64 + 62).await?, Tick::Idle);
        assert!(database.get_saga(&unrelated.id).await?.is_some());
        database.delete_saga(&unrelated.id).await?; // remove only the injected fixture conflict
    } else {
        assert_eq!(worker.tick(now as i64 + 26).await?, Tick::Progress);
        assert_eq!(worker.tick(now as i64 + 27).await?, Tick::Idle);
    }
    assert!(database.get_incomplete_sagas().await?.is_empty());
    let quote = database
        .get_melt_quote("scheduled-original-quote")
        .await?
        .unwrap();
    assert_eq!(quote.state, cdk::nuts::MeltQuoteState::Paid);
    assert!(quote.used_by_operation.is_none());
    drop(worker);
    let mut journal = Journal::open(&path, harness.epoch, 8)?;
    let report = journal.treasury_melt_report(&policy.binding, issued.intent.id)?;
    report.validate(&issued.intent)?;
    assert_eq!(report.sources.len(), 1);
    assert_eq!(report.sources[0].input_sats, 5);
    assert_eq!(report.sources[0].deposit_sats, 3);
    assert_eq!(report.sources[0].fee_sats, 2); // one split fee plus one melt input fee
    assert_eq!(report.sources[0].change_sats, 0);
    let health_db = rusqlite::Connection::open(&path)?;
    let health = crate::status::Funds::read(&health_db, None, i64::try_from(now)?)?;
    assert_eq!(health.journal_custody_sats, 11);
    assert_eq!(health.split_preparation_fee_sats, 1);
    assert_eq!(health.unsettled_preparation_fee_sats, 0);
    assert_eq!(health.completed_conversion_fee_sats, 2);
    assert_eq!(health.reported_treasury_deposit_sats, 3);
    assert_eq!(health.total_accounted_fee_sats, health.receive_fee_sats + 2);
    assert_eq!(harness.mint.executions.load(Ordering::SeqCst), 2);
    assert_eq!(harness.mint.quote_attempts.load(Ordering::SeqCst), 1);
    assert_eq!(harness.mint.melt_attempts.load(Ordering::SeqCst), 1);
    let after = journal.export_receipt(acquisition)?;
    assert_eq!(
        lightning_goats_cashu_receipt::treasury::hex_digest(
            crate::receipt_wire::from_private(policy.binding.wallet_instance, &after)?.digest()?
        ),
        issued.intent.sources[0].receipt_digest
    );
    drop(journal);
    let journal = Journal::open(&path, harness.epoch, 8)?;
    assert!(journal.treasury_melt_report(&policy.binding, issued.intent.id)? == report);
    let mut receiver = harness.open_with_writer(writer.clone()).await?;
    harness.mint.lose_reply.store(false, Ordering::SeqCst);
    let contribution =
        harness.ordinary(harness.mint.issue("after-cleanup".into()), "after-cleanup")?;
    let later = new_id(receiver.admit(&contribution)?);
    let received = receipt(receiver.acquire_new(later, &harness.receiver).await?);
    assert_eq!(received.net_sats, 7);
    assert_eq!(harness.mint.melt_attempts.load(Ordering::SeqCst), 1);
    assert!(journal.treasury_melt_report(&policy.binding, issued.intent.id)? == report);
    Ok(())
}
