// Separate-UID financial composition across two independently keyed issuers.
mod multi_mint {
    use super::*;
    use crate::treasury_scheduler::{Execution, Worker};
    use cdk::cdk_database::{Error as DatabaseError, WalletDatabase};
    use rusqlite::OptionalExtension as _;
    use std::time::Duration;

    #[tokio::test]
    #[ignore = "requires explicit separate-UID multi-mint financial runner"]
    async fn combined_multi_mint_wallet_child() -> Result<()> {
        ensure!(
            std::env::var("LG_CASHU_COMBINED_MULTI_MINT").as_deref() == Ok("1"),
            "explicit multi-mint mode required"
        );
        for name in [
            "LG_CASHU_COMBINED_RECEIPT_IPC",
            "LG_CASHU_COMBINED_TREASURY_IPC",
            "LG_CASHU_COMBINED_HTTP_INTAKE",
            "LG_CASHU_COMBINED_FEES",
            "LG_CASHU_COMBINED_NO_CHANGE",
        ] {
            ensure!(
                std::env::var(name).as_deref() == Ok("1"),
                "full fee-only IPC mode required"
            );
        }
        let root = std::path::PathBuf::from(std::env::var("LG_CASHU_COMBINED_FIXTURE")?);
        let bootstrap: Bootstrap = read_when_ready(&root, "bootstrap.json").await?;
        let core_uid = bootstrap
            .core_uid
            .ok_or_else(|| anyhow::anyhow!("core UID missing"))?;
        let group = std::env::var("LG_CASHU_COMBINED_IPC_GID")?.parse()?;
        let lost = std::env::var("LG_CASHU_COMBINED_LOST_REPLY").as_deref() == Ok("1");
        let sdk = std::env::var_os("LG_CASHU_COMBINED_SENDER_NODE").is_some();
        let mut harnesses = [Harness::new(100)?, Harness::new(100)?];
        for (index, h) in harnesses.iter_mut().enumerate() {
            h.id = if index == 0 { "fixture-a" } else { "fixture-b" }.into();
            h.epoch = bootstrap.wallet_epoch;
            h.mint.consume_melt_reserve.store(true, Ordering::SeqCst);
        }
        // The second configured mint has distinct signing keys and keyset ID.
        harnesses[1].mint.private_keys = Arc::new(
            (0..11)
                .map(|bit| {
                    (
                        Amount::from(1u64 << bit),
                        SecretKey::from_slice(&[40 + bit as u8; 32]).unwrap(),
                    )
                })
                .collect(),
        );
        let issuer_keys = Keys::new(
            harnesses[1]
                .mint
                .private_keys
                .iter()
                .map(|(amount, key)| (*amount, key.public_key()))
                .collect(),
        );
        harnesses[1].mint.keyset.id = Id::v1_from_keys(&issuer_keys);
        harnesses[1].mint.keyset.keys = issuer_keys;
        assert_ne!(harnesses[0].mint.keyset.id, harnesses[1].mint.keyset.id);
        let policies = harnesses.iter().map(Harness::policy).collect::<Vec<_>>();
        let mut pinned = PinnedKeys::default();
        for h in &harnesses {
            for (amount, key) in h.mint.keyset.keys.iter() {
                pinned.insert(&h.id, h.mint.keyset.id, u64::from(*amount), *key)?;
            }
        }
        let keys = Arc::new(pinned);
        for h in &mut harnesses {
            h.keys = keys.clone();
        }
        let directory = harnesses[0].directory.path();
        let path = directory.join("journal.sqlite");
        let epoch = bootstrap.wallet_epoch;
        let writer = Arc::new(WriterLock::acquire(directory)?);
        let journal = Arc::new(Mutex::new(Journal::open(&path, epoch, 64)?));
        let mut stores: BTreeMap<String, Arc<dyn WalletDatabase<DatabaseError> + Send + Sync>> =
            BTreeMap::new();
        let mut coordinators = Vec::new();
        for (h, policy) in harnesses.iter().zip(&policies) {
            let database =
                crate::wallet::open_mint_wallet_storage(directory, policy, &writer, true).await?;
            stores.insert(policy.id.clone(), database.clone());
            let mint_url: MintUrl = policy.advertised_url.parse()?;
            let gate = OperationGate::new(h.mint.clone(), journal.clone(), writer.clone(), 65536)?
                .with_wallet_database(database.clone())
                .with_key_policy(policy.id.clone(), keys.clone());
            let mut wallet = WalletBuilder::new()
                .mint_url(mint_url.clone())
                .unit(CurrencyUnit::Sat)
                .localstore(database.clone())
                .seed(h.seed)
                .client(CustomHttpClient::with_transport(
                    mint_url.clone(),
                    gate.clone(),
                    None,
                ))
                .without_rate_limiting()
                .use_http_subscription()
                .build()?;
            wallet.set_target_proof_count(0);
            coordinators.push(Coordinator {
                wallet,
                database,
                gate,
                journal: journal.clone(),
                keys: keys.clone(),
                seed: &h.seed,
                mint_id: policy.id.clone(),
                mint_url,
                permissions: Permissions {
                    admission: true,
                    acquisition: true,
                    receipt_export: true,
                    original_swap_replay: true,
                },
                maximum_receive_fee_sats: 1,
            });
        }
        assert_eq!(stores.len(), 2);
        assert!(!directory.join("cdk.sqlite").exists());
        let registry = MintRegistry::new(policies.clone())?;
        let descriptor = crate::descriptor::Descriptor::prepare(
            &registry,
            "https://public.example/api/v1/cashu",
            harnesses[0].receiver.public_key(),
            crate::descriptor::Encoding::CreqA,
        )?;
        let descriptor_id = descriptor.id().to_owned();
        let receiver = harnesses[0].receiver.public_key();
        let socket = root.parent().unwrap().join("wallet-run/intake.sock");
        let (server_keys, server_journal, server_writer) =
            (keys.clone(), journal.clone(), writer.clone());
        let instance = bootstrap.wallet_instance;
        let intake_server = tokio::spawn(async move {
            let registry = MintRegistry::new(policies)?;
            let limits = Limits {
                body_bytes: 65536,
                proof_count: 64,
                receipt_sats: 10000,
            };
            let descriptors = vec![descriptor.id().to_owned()];
            let intake = crate::ingress::Intake {
                core_uid,
                wallet_instance: instance,
                wallet_epoch: epoch,
                receive_cashu: true,
                receive_nutzap: false,
                registry: &registry,
                keys: &server_keys,
                limits: &limits,
                receiver,
                recipient: "",
                descriptors: &descriptors,
                public_request: Some(&descriptor),
                journal: server_journal,
                writer: &server_writer,
            };
            intake
                .serve(
                    &socket,
                    group,
                    Duration::from_secs(5),
                    64,
                    std::future::pending(),
                )
                .await
        });
        let receipt_client = crate::receipt_delivery::Client::new(
            bootstrap
                .receipt_socket
                .clone()
                .ok_or_else(|| anyhow::anyhow!("receipt socket missing"))?,
            core_uid,
            bootstrap.core_epoch,
            Duration::from_secs(5),
        )?;
        let origin = bootstrap
            .http_origin
            .clone()
            .ok_or_else(|| anyhow::anyhow!("HTTP origin missing"))?;
        let http = reqwest::Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(5))
            .build()?;
        let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
        loop {
            let reply = http
                .get(format!("{origin}/api/v1/cashu/request"))
                .send()
                .await?;
            if reply.status().is_success() {
                let public = fixture_json_response(reply).await?;
                ensure!(
                    public["enabled"] == true && public["id"] == descriptor_id,
                    "public descriptor changed"
                );
                break;
            }
            ensure!(
                tokio::time::Instant::now() < deadline,
                "public descriptor deadline"
            );
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        let mut receipts = Vec::new();
        let mut http_replay = None;
        for index in 0..2 {
            let amount = if index == 0 { 250 } else { 150 };
            let input = contribution(&harnesses[index], amount + 1, index == 0, index)?;
            let id = if index == 0 {
                new_id(coordinators[index].admit(&input)?)
            } else {
                let payload = json!({"id":descriptor_id,"mint":harnesses[index].policy().advertised_url,
                    "unit":"sat","proofs":input.intake().proofs});
                let (payload, accepted) = if sdk {
                    fixture_sdk_sender(&json!({"origin":origin,"descriptor_id":descriptor_id,
                        "mint":harnesses[index].policy().advertised_url,"keyset":harnesses[index].mint.keyset,
                        "gross_sats":amount+1,"proofs":input.intake().proofs})).await?
                } else {
                    let response = http
                        .post(format!("{origin}/api/v1/cashu"))
                        .header("content-type", "application/json")
                        .body(serde_json::to_vec(&payload)?)
                        .send()
                        .await?;
                    ensure!(
                        response.status() == reqwest::StatusCode::ACCEPTED,
                        "HTTP admission failed"
                    );
                    (payload, fixture_json_response(response).await?)
                };
                ensure!(
                    accepted["status"] == "pending"
                        && accepted["capability"]
                            .as_str()
                            .is_some_and(|v| v.len() == 64),
                    "HTTP must acknowledge pending admission only"
                );
                http_replay = Some((payload, accepted["capability"].clone()));
                let connection = rusqlite::Connection::open_with_flags(
                    &path,
                    rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
                )?;
                let id: String = connection.query_row(
                    "SELECT id FROM acquisitions WHERE state='validated' AND mint_id='fixture-b'",
                    [],
                    |r| r.get(0),
                )?;
                id.parse()?
            };
            let acquired = receipt(
                coordinators[index]
                    .acquire_new(id, &harnesses[index].receiver)
                    .await?,
            );
            assert_eq!(acquired.net_sats, amount);
            assert_eq!(acquired.mint_id, harnesses[index].id);
            let wire = crate::receipt_wire::from_private(instance, &acquired)?;
            // A lost local checkpoint must retain the same authenticated receipt.
            receipt_client.send(&wire).await?;
            receipt_client.deliver(&journal, instance, id).await?;
            receipts.push(wire.data().clone());
        }
        drop(coordinators);
        let bindings = harnesses
            .iter()
            .map(|h| {
                let binding = crate::treasury::TreasuryBinding {
                    core_epoch: bootstrap.core_epoch,
                    wallet_instance: instance,
                    wallet_epoch: epoch,
                    destination: bootstrap.destination.clone(),
                    mint_id: h.id.clone(),
                    policy_version: bootstrap.treasury_policy.clone(),
                };
                (h.id.clone(), binding)
            })
            .collect::<BTreeMap<_, _>>();
        let (server_bindings, server_journal, server_writer) =
            (bindings.clone(), journal.clone(), writer.clone());
        let socket = bootstrap
            .treasury_socket
            .ok_or_else(|| anyhow::anyhow!("treasury socket missing"))?;
        let treasury_server = tokio::spawn(async move {
            let intake = crate::treasury_ingress::Intake {
                core_uid,
                accept_new: true,
                bindings: &server_bindings,
                journal: server_journal,
                writer: &server_writer,
            };
            intake
                .serve(
                    &socket,
                    group,
                    Duration::from_secs(5),
                    64,
                    std::future::pending(),
                )
                .await
        });
        let contexts = harnesses
            .iter()
            .map(|h| {
                (
                    Arc::new(crate::operation_gate::TreasuryPolicy {
                        binding: bindings[&h.id].clone(),
                        mint: h.policy(),
                        seed: h.seed,
                    }),
                    h.mint.clone(),
                )
            })
            .collect::<Vec<_>>();
        let now = i64::try_from(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_secs(),
        )?;
        for round in 0..2 {
            let h = &harnesses[round];
            let binding = &bindings[&h.id];
            let deadline = tokio::time::Instant::now() + Duration::from_secs(120);
            let issued = loop {
                let connection = rusqlite::Connection::open_with_flags(
                    &path,
                    rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
                )?;
                let retained: Option<String> = connection.query_row(
                    "SELECT id FROM treasury_instructions WHERE json_extract(CAST(issued_json AS TEXT),'$.intent.mint_id')=? LIMIT 1", [&h.id], |r| r.get(0)).optional()?;
                if let Some(id) = retained {
                    break Journal::open(&path, epoch, 64)?
                        .treasury_instruction(binding, id.parse()?)?;
                }
                ensure!(
                    tokio::time::Instant::now() < deadline,
                    "treasury admission deadline"
                );
                tokio::time::sleep(Duration::from_millis(25)).await;
            };
            let observed_now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_secs();
            binding.validate_new_invoice(&issued, observed_now)?;
            let input = if round == 0 { 250 } else { 150 };
            let deposit = input - 2;
            assert_eq!(
                (
                    issued.intent.input_sats,
                    issued.intent.deposit_sats,
                    issued.intent.maximum_fee_sats
                ),
                (input, deposit, 2)
            );
            assert_eq!(issued.intent.sources.len(), 1);
            assert_eq!(
                issued.intent.sources[0].acquisition_id,
                receipts[round].acquisition_id
            );
            let quote = json!({"quote":format!("multi-mint-{round}"),"amount":deposit,"fee_reserve":1,
                "state":"UNPAID","expiry":observed_now+240,"payment_preimage":null,"change":null,
                "request":issued.invoice,"unit":"sat","method":"bolt11"});
            *h.mint.quote_reply.lock().unwrap() = Some(quote.clone());
            let mut paid = quote;
            paid["state"] = "PAID".into();
            paid["payment_preimage"] = if round == 0 {
                "37".repeat(32)
            } else {
                "38".repeat(32)
            }
            .into();
            paid["change"] = json!([]);
            *h.mint.melt_paid_reply.lock().unwrap() = Some(paid);
            h.mint.lose_melt_reply.store(lost, Ordering::SeqCst);
            h.mint.melt_status_unavailable.store(lost, Ordering::SeqCst);
            let mut execution = Execution {
                execute: true,
                reconcile: true,
                maximum_attempts: 4,
                maximum_input_sats: 400,
                maximum_fee_sats: 2,
                operations_per_24h: 2,
                input_sats_per_24h: 400,
                maximum_in_flight_sats: 400,
                split: Default::default(),
            };
            let mut paused = false;
            let mut report = None;
            for tick in 0..80 {
                let worker = Worker::from_mint_stores(
                    Arc::new(Mutex::new(Journal::open(&path, epoch, 64)?)),
                    stores.clone(),
                    writer.clone(),
                    keys.clone(),
                    contexts.clone(),
                    execution.clone(),
                    Duration::from_secs(10),
                    65536,
                )?;
                worker.tick(now + round as i64 * 100 + tick).await?;
                if lost && !paused && h.mint.melt_settled.load(Ordering::SeqCst) {
                    assert!(
                        Journal::open(&path, epoch, 64)?
                            .treasury_melt_report(binding, issued.intent.id)
                            .is_err()
                    );
                    let marker = if round == 0 {
                        "provider-verified.json"
                    } else {
                        "residual-provider-verified.json"
                    };
                    let verified: Uuid = read_when_ready(&root, marker).await?;
                    assert_eq!(verified, issued.intent.id);
                    execution.execute = false;
                    h.mint
                        .melt_status_unavailable
                        .store(false, Ordering::SeqCst);
                    paused = true;
                }
                if let Ok(value) =
                    Journal::open(&path, epoch, 64)?.treasury_melt_report(binding, issued.intent.id)
                {
                    report = Some(value);
                    break;
                }
            }
            let report = report
                .ok_or_else(|| anyhow::anyhow!("multi-mint original melt did not reconcile"))?;
            assert_eq!(paused, lost);
            assert_eq!(report.sources.len(), 1);
            assert_eq!(
                (
                    report.sources[0].deposit_sats,
                    report.sources[0].fee_sats,
                    report.sources[0].change_sats
                ),
                (deposit, 2, 0)
            );
            assert_eq!(h.mint.executions.load(Ordering::SeqCst), 1);
            assert_eq!(h.mint.quote_attempts.load(Ordering::SeqCst), 1);
            assert_eq!(h.mint.melt_attempts.load(Ordering::SeqCst), 1);
            // Stop exactly at this operation's cleanup before the next mint's
            // quote is configured; never erase or bypass a retained saga.
            for tick in 0..8 {
                let connection = rusqlite::Connection::open_with_flags(
                    &path,
                    rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
                )?;
                let done: u64 = connection.query_row(
                    "SELECT COUNT(*) FROM treasury_cdk_cleanup WHERE intent_id=?",
                    [issued.intent.id.to_string()],
                    |r| r.get(0),
                )?;
                if done == 1 {
                    break;
                }
                let worker = Worker::from_mint_stores(
                    Arc::new(Mutex::new(Journal::open(&path, epoch, 64)?)),
                    stores.clone(),
                    writer.clone(),
                    keys.clone(),
                    contexts.clone(),
                    execution.clone(),
                    Duration::from_secs(10),
                    65536,
                )?;
                worker.tick(now + round as i64 * 100 + 80 + tick).await?;
            }
            let connection = rusqlite::Connection::open_with_flags(
                &path,
                rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
            )?;
            assert_eq!(
                connection.query_row(
                    "SELECT COUNT(*) FROM treasury_cdk_cleanup WHERE intent_id=?",
                    [issued.intent.id.to_string()],
                    |r| r.get::<_, u64>(0)
                )?,
                1
            );
        }
        let _: Uuid = read_when_ready(&root, "credit-consumed.json").await?;
        for r in &receipts {
            receipt_client
                .deliver(&journal, instance, r.acquisition_id)
                .await?;
        }
        let (payload, capability) =
            http_replay.ok_or_else(|| anyhow::anyhow!("HTTP payment missing"))?;
        let status = http
            .post(format!("{origin}/api/v1/cashu/status"))
            .header("content-type", "application/json")
            .body(serde_json::to_vec(&json!({"capability":capability}))?)
            .send()
            .await?;
        ensure!(
            status.status() == reqwest::StatusCode::OK
                && fixture_json_response(status).await? == json!({"status":"credited"}),
            "HTTP status lost durable credit"
        );
        let replay = http
            .post(format!("{origin}/api/v1/cashu"))
            .header("content-type", "application/json")
            .body(serde_json::to_vec(&payload)?)
            .send()
            .await?;
        ensure!(
            replay.status() == reqwest::StatusCode::ACCEPTED
                && fixture_json_response(replay).await?
                    == json!({"status":"credited","capability":capability}),
            "consumed-credit HTTP replay changed receipt"
        );
        publish(&root, "receipt-replayed.json", &2u64)?;
        let health_db = rusqlite::Connection::open(&path)?;
        let health = crate::status::Funds::read(&health_db, None, now)?;
        assert_eq!(health.acquired_net_sats, 400);
        assert_eq!(health.unverified_input_sats, 0);
        assert_eq!(health.journal_custody_sats, 0);
        assert_eq!(health.pending_input_reserved_sats, 0);
        assert_eq!(health.ambiguous_submitted_input_sats, 0);
        assert_eq!(health.reported_treasury_deposit_sats, 396);
        assert_eq!(health.receive_fee_sats, 2);
        assert_eq!(health.completed_conversion_fee_sats, 4);
        assert_eq!(health.total_accounted_fee_sats, 6);
        for h in &harnesses {
            assert_eq!(h.mint.executions.load(Ordering::SeqCst), 1);
            assert_eq!(h.mint.melt_attempts.load(Ordering::SeqCst), 1);
        }
        intake_server.abort();
        treasury_server.abort();
        for marker in [
            "COMBINED_WALLET_HEALTH_ACCOUNTING_VERIFIED",
            "COMBINED_WALLET_PUBLIC_HTTP_FINANCIAL_IPC_VERIFIED",
            "COMBINED_WALLET_FULL_FINANCIAL_IPC_VERIFIED",
            "COMBINED_WALLET_CDK_RECEIPT_SOCKET_VERIFIED",
            "COMBINED_WALLET_REAL_FEES_CHANGE_VERIFIED",
            "COMBINED_WALLET_FEE_ONLY_ZERO_CHANGE_VERIFIED",
            "COMBINED_WALLET_TWO_MINTS_TWO_ACQUISITIONS_TWO_MELTS",
        ] {
            println!("{marker}");
        }
        if sdk {
            println!("COMBINED_WALLET_CASHU_TS_SENDER_VERIFIED");
        }
        if lost {
            println!("COMBINED_WALLET_LOST_REPLY_PAUSED_RECOVERY_VERIFIED");
        }
        Ok(())
    }
}
