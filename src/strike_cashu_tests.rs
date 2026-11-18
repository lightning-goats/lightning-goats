// Synthetic core/provider fixtures. No mint, external account or physical feed.
mod cashu_treasury_tests {
    use super::*;
    use crate::cashu::receipt_authority::{
        ImportLimits, MintImportLimit, ReceiptPolicy, fixture_authenticate, fixture_peer,
    };
    use crate::cashu::treasury::{Limits, MintLimits, PeerMelt, Policy};
    use lightning_goats_cashu_receipt::{
        ReceiptData, WireReceipt,
        treasury::{Destination, Intent, MeltData, hex_digest},
    };
    use sqlx::SqlitePool;

    struct Fixture {
        _directory: TempDir,
        ledger: LedgerStore,
        pool: SqlitePool,
        registration: ReceiptPolicy,
        policy: Policy,
        intent: Intent,
        runtime: StrikeRuntime,
        state: MockState,
        receipts: Vec<WireReceipt>,
    }
    impl Fixture {
        async fn new(
            parts: &[u64],
            native: u64,
            deposit: u64,
            maximum_fee: u64,
            alter: fn(&mut MockState),
        ) -> Self {
            Self::with_planning(parts, native, deposit, maximum_fee, alter, None).await
        }
        async fn with_planning(
            parts: &[u64],
            native: u64,
            deposit: u64,
            maximum_fee: u64,
            alter: fn(&mut MockState),
            sweep: Option<bool>,
        ) -> Self {
            let (directory, ledger) = ledger().await;
            ledger.initialize_reward_funding().await.unwrap();
            ledger.configure_feeder_credit_cap(1000).await.unwrap();
            let registration = ReceiptPolicy {
                wallet_uid: 60001,
                wallet_instance: Uuid::new_v4(),
                wallet_epoch: Uuid::new_v4(),
                core_epoch: ledger.overlay_stream_id().await.unwrap(),
                import_enabled: true,
                limits: ImportLimits {
                    receipts_per_24h: 100,
                    credit_sats_per_24h: 20000,
                    maximum_custody_sats: 20000,
                },
                approved: std::collections::BTreeMap::from([(
                    ("mint-a".into(), "v1".into()),
                    MintImportLimit {
                        maximum_gross_sats: 10000,
                        maximum_net_sats: 10000,
                    },
                )]),
            };
            if native > 0 {
                lightning(&ledger, native).await;
            }
            let mut receipts = Vec::new();
            for (index, net) in parts.iter().enumerate() {
                let id = Uuid::new_v4();
                let receipt = WireReceipt::validate(ReceiptData {
                    schema_version: 1,
                    wallet_instance: registration.wallet_instance,
                    wallet_epoch: registration.wallet_epoch,
                    acquisition_id: id,
                    transport: if index == 0 { "nutzap" } else { "cashu" }.into(),
                    source_ref: if index == 0 {
                        "19".repeat(32)
                    } else {
                        "reusable".into()
                    },
                    source_author: (index == 0).then(|| {
                        "79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798".into()
                    }),
                    mint_id: "mint-a".into(),
                    unit: "sat".into(),
                    gross_sats: *net,
                    receive_fee_sats: 0,
                    net_sats: *net,
                    acquisition_evidence_digest: format!("{}{}", id.simple(), id.simple()),
                    policy_version: "v1".into(),
                    credit_pool: "herd".into(),
                })
                .unwrap();
                ledger
                    .import_cashu_receipt(
                        &registration,
                        &fixture_authenticate(&registration, &receipt.encode().unwrap()).unwrap(),
                    )
                    .await
                    .unwrap();
                receipts.push(receipt);
            }
            let pool = crate::sqlite::connect_existing_durable(
                &format!("sqlite://{}", directory.path().join("strike.db").display()),
                1,
            )
            .await
            .unwrap();
            let source_ids: Vec<i64> = sqlx::query_scalar(
                "SELECT ledger_entry_id FROM cashu_receipt_inbox ORDER BY ledger_entry_id",
            )
            .fetch_all(&pool)
            .await
            .unwrap();
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let origin = format!("http://{}/", listener.local_addr().unwrap());
            let mut state = state();
            use sha2::Digest as _;
            let mut policy = Policy {
                enabled: true,
                version: "treasury-fixture-v1".into(),
                destination: Destination {
                    origin: origin.clone(),
                    account_id: Uuid::new_v4(),
                    credential_fingerprint: hex::encode(Sha256::digest(state.api_key.as_bytes())),
                    network: "bitcoin".into(),
                },
                approved_mints: std::collections::BTreeMap::from([(
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
                    fee_sats_per_24h: Some(10000),
                    maximum_in_flight_sats: 20000,
                },
            };
            let plan = crate::ledger::TreasuryPlan {
                id: Uuid::new_v4(),
                mint_id: "mint-a".into(),
                sources: source_ids.into_iter().zip(parts.iter().copied()).collect(),
                deposit_sats: deposit,
                maximum_fee_sats: maximum_fee,
            };
            let intent = if let Some(sweep) = sweep {
                use crate::cashu::treasury_worker::{Controls, Tick, Worker};
                policy.limits.maximum_fee_sats = maximum_fee;
                policy
                    .approved_mints
                    .get_mut("mint-a")
                    .unwrap()
                    .maximum_fee_sats = maximum_fee;
                if !sweep {
                    assert!(ledger.begin_feed_attempt(1000).await.unwrap().is_none());
                }
                let runtime = StrikeRuntime::new(
                    StrikeClient::new(&origin, state.api_key.into()).unwrap(),
                    StrikeWebhookVerifier::new("synthetic-webhook".into()).unwrap(),
                );
                let worker = Worker::new(
                    ledger.clone(),
                    runtime,
                    Arc::new(copy_registration(&registration)),
                    Arc::new(policy.clone()),
                    PlanningOnly,
                    Controls {
                        plan: true,
                        sweep: crate::cashu::treasury_sweep::Policy {
                            enabled: true,
                            balance_trigger_sats: 1,
                            ..Default::default()
                        },
                        inspect: true,
                        maximum_attempts: 8,
                        ..Default::default()
                    },
                    Duration::from_secs(30),
                )
                .unwrap();
                assert_eq!(worker.tick(1_800_000_000).await.unwrap(), Tick::Progress);
                assert_eq!(worker.tick(1_800_000_001).await.unwrap(), Tick::Idle);
                assert_eq!(state.api_calls.load(Ordering::SeqCst), 0);
                let id: String = sqlx::query_scalar("SELECT id FROM cashu_treasury_intents")
                    .fetch_one(&pool)
                    .await
                    .unwrap();
                let intent = ledger
                    .cashu_treasury_intent(Uuid::parse_str(&id).unwrap())
                    .await
                    .unwrap()
                    .unwrap();
                assert_eq!(intent.deposit_sats, deposit);
                let feeds: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM feed_financial_treasury")
                    .fetch_one(&pool)
                    .await
                    .unwrap();
                let sweeps: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM cashu_sweep_jobs")
                    .fetch_one(&pool)
                    .await
                    .unwrap();
                assert_eq!((feeds, sweeps), if sweep { (0, 1) } else { (1, 0) });
                intent
            } else {
                ledger
                    .reserve_cashu_treasury(&registration, &policy, &plan)
                    .await
                    .unwrap()
            };
            state.description_hash = hex_digest(intent.digest().unwrap());
            state.amount_btc = msat_to_btc_decimal(deposit * 1000);
            state.credited_amount = Some(json!({"amount":state.amount_btc,"currency":"BTC"}));
            state.invoice = crate::test_invoices::invoice(deposit * 1000, &state.description_hash);
            alter(&mut state);
            spawn_mock_on(listener, state.clone());
            let runtime = StrikeRuntime::new(
                StrikeClient::new(&origin, state.api_key.into()).unwrap(),
                StrikeWebhookVerifier::new("synthetic-webhook".into()).unwrap(),
            );
            Self {
                _directory: directory,
                ledger,
                pool,
                registration,
                policy,
                intent,
                runtime,
                state,
                receipts,
            }
        }
        fn event(&self) -> StrikeCompletedReceiveEvent {
            StrikeCompletedReceiveEvent {
                event_id: Uuid::new_v4(),
                receive_request_id: self.state.receive_request_id,
                receive_id: self.state.receive_id,
            }
        }
        fn report(&self, fees: u64, change: u64) -> PeerMelt {
            self.report_with_fee_time(fees, change, None)
        }
        fn report_with_fee_time(&self, fees: u64, change: u64, time: Option<i64>) -> PeerMelt {
            let sources = self.intent.allocate_result(fees, change).unwrap();
            let data = MeltData {
                version: 1,
                intent_id: self.intent.id,
                intent_digest: hex_digest(self.intent.digest().unwrap()),
                quote_id: "frozen-melt-quote".into(),
                operation_evidence_digest: "ab".repeat(32),
                sources,
            };
            PeerMelt::authenticate_with_fee_time(
                &self.registration,
                &fixture_peer(&self.registration),
                &serde_json::to_vec(&data).unwrap(),
                time,
            )
            .unwrap()
        }
        async fn issue(&self) {
            assert!(
                self.runtime
                    .create_cashu_treasury_invoice(
                        &self.ledger,
                        &self.registration,
                        &self.policy,
                        self.intent.id
                    )
                    .await
                    .unwrap()
                    .is_some()
            );
        }
        async fn count(&self, table: &str) -> i64 {
            sqlx::query_scalar(&format!("SELECT COUNT(*) FROM {table}"))
                .fetch_one(&self.pool)
                .await
                .unwrap()
        }
    }

    #[tokio::test]
    async fn late_wallet_fee_observation_delays_release_and_migrates_legacy_evidence() -> Result<()> {
        let f = Fixture::new(&[8], 0, 6, 2, |_| {}).await;
        f.issue().await;
        f.runtime.reconcile_and_credit(&f.ledger, &f.event()).await?;
        let strike_time: i64 = sqlx::query_scalar("SELECT verified_at FROM cashu_treasury_deposits")
            .fetch_one(&f.pool).await?;
        let wallet_time = strike_time + 600;
        assert_eq!(f.ledger.test_cashu_fee_budget(wallet_time, 2).await?, 0);
        let original = f.report_with_fee_time(1, 1, Some(wallet_time));
        for _ in 0..2 {
            assert!(f.ledger.record_cashu_melt_result(&f.registration, &original).await?);
        }
        assert_eq!(f.count("cashu_treasury_fee_observations").await, 1);
        assert_eq!(f.ledger.test_cashu_fee_budget(strike_time + 86_401, 2).await?, 1);
        assert_eq!(f.ledger.test_cashu_fee_budget(wallet_time + 86_400, 2).await?, 1);
        assert_eq!(f.ledger.test_cashu_fee_budget(wallet_time + 86_401, 2).await?, 2);
        let changed = f.report_with_fee_time(1, 1, Some(wallet_time + 1));
        assert!(f.ledger.record_cashu_melt_result(&f.registration, &changed).await.is_err());

        let original: (String,String) = sqlx::query_as("SELECT result_digest,result_json FROM cashu_treasury_wallet_results")
            .fetch_one(&f.pool).await?;
        // Disposable schema-26 image. Keep the old result, completion and deposit bytes.
        sqlx::raw_sql("DROP TABLE cashu_treasury_inspections;
                     DROP TABLE cashu_treasury_fee_observations;
                     DELETE FROM _sqlx_migrations WHERE version IN (27,28);")
            .execute(&f.pool).await?;
        sqlx::migrate!("./migrations").run(&f.pool).await?;
        let observed: i64 = sqlx::query_scalar("SELECT observed_at FROM cashu_treasury_fee_observations")
            .fetch_one(&f.pool).await?;
        assert!(observed >= strike_time);
        assert_eq!(f.ledger.test_cashu_fee_budget(observed + 86_400, 2).await?, 1);
        assert_eq!(f.ledger.test_cashu_fee_budget(observed + 86_401, 2).await?, 2);
        let retained: (String,String) = sqlx::query_as("SELECT result_digest,result_json FROM cashu_treasury_wallet_results")
            .fetch_one(&f.pool).await?;
        assert_eq!(original, retained);
        assert_eq!(f.count("cashu_treasury_completions").await, 1);
        assert_eq!(f.ledger.feed_credit_sats().await?, 8);
        Ok(())
    }
    #[tokio::test]
    async fn core_treasury_claims_are_bounded_and_uncertain_issuance_never_requeues() {
        use crate::cashu::treasury_worker::{Controls, Stage};
        let f = Fixture::new(&[8], 0, 7, 1, |_| {}).await;
        let controls = Controls {
            plan: false,
            sweep: Default::default(),
            issue: true,
            deliver: true,
            inspect: true,
            maximum_attempts: 2,
        };
        assert!(
            f.ledger
                .claim_cashu_treasury_work(&f.registration, &Controls::default(), 10, 30)
                .await
                .unwrap()
                .is_none()
        );
        assert!(
            f.ledger
                .claim_cashu_treasury_work(&f.registration, &controls, i64::MAX, 30)
                .await
                .is_err()
        );
        assert_eq!(f.count("cashu_treasury_work").await, 0);
        let first = f
            .ledger
            .claim_cashu_treasury_work(&f.registration, &controls, 10, 30)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(first.id, f.intent.id);
        assert_eq!(first.stage, Stage::Issue);
        assert!(
            f.ledger
                .claim_cashu_treasury_work(&f.registration, &controls, 44, 30)
                .await
                .unwrap()
                .is_none()
        );
        let reopened = LedgerStore::connect(&format!(
            "sqlite://{}",
            f._directory.path().join("strike.db").display()
        ))
        .await
        .unwrap();
        assert!(
            reopened
                .claim_cashu_treasury_work(&f.registration, &controls, 45, 30)
                .await
                .unwrap()
                .is_some()
        );
        assert!(
            reopened
                .claim_cashu_treasury_work(&f.registration, &controls, 1000, 30)
                .await
                .unwrap()
                .is_none()
        );
        assert!(
            f.ledger
                .claim_cashu_treasury_issuance(f.intent.id)
                .await
                .unwrap()
                .is_some()
        );
        let more = Controls {
            maximum_attempts: 100,
            ..controls
        };
        assert!(
            reopened
                .claim_cashu_treasury_work(&f.registration, &more, 2000, 30)
                .await
                .unwrap()
                .is_none()
        );
        assert_eq!(f.state.api_calls.load(Ordering::SeqCst), 0);
    }

    fn copy_registration(value: &ReceiptPolicy) -> ReceiptPolicy {
        ReceiptPolicy {
            wallet_uid: value.wallet_uid,
            wallet_instance: value.wallet_instance,
            wallet_epoch: value.wallet_epoch,
            core_epoch: value.core_epoch,
            import_enabled: value.import_enabled,
            limits: ImportLimits {
                receipts_per_24h: value.limits.receipts_per_24h,
                credit_sats_per_24h: value.limits.credit_sats_per_24h,
                maximum_custody_sats: value.limits.maximum_custody_sats,
            },
            approved: value
                .approved
                .iter()
                .map(|(key, limit)| {
                    (
                        key.clone(),
                        MintImportLimit {
                            maximum_gross_sats: limit.maximum_gross_sats,
                            maximum_net_sats: limit.maximum_net_sats,
                        },
                    )
                })
                .collect(),
        }
    }

    struct PlanningOnly;
    impl crate::cashu::treasury_worker::Wallet for PlanningOnly {
        async fn exchange(
            &self,
            _: &lightning_goats_cashu_receipt::treasury_rpc::Request,
        ) -> Result<crate::cashu::treasury_client::Response> {
            panic!("planning must not contact a wallet")
        }
    }

    #[tokio::test]
    async fn automatically_planned_feed_and_sweep_wait_for_strike_without_new_credit() {
        use crate::cashu::treasury_worker::{Controls, Tick, Worker};
        for sweep in [false, true] {
            for fee in [0, 10] {
                let f = Fixture::with_planning(
                    &[250, 150],
                    600,
                    400 - fee,
                    fee,
                    unchanged,
                    Some(sweep),
                )
                .await;
                let requests = Arc::new(std::sync::Mutex::new(Vec::new()));
                let registration = Arc::new(copy_registration(&f.registration));
                let worker = Worker::new(
                    f.ledger.clone(),
                    f.runtime.clone(),
                    registration.clone(),
                    Arc::new(f.policy.clone()),
                    ScheduledWallet {
                        registration,
                        data: MeltData {
                            version: 1,
                            intent_id: f.intent.id,
                            intent_digest: hex_digest(f.intent.digest().unwrap()),
                            quote_id: "automatically-planned-original-quote".into(),
                            operation_evidence_digest: "ab".repeat(32),
                            sources: f.intent.allocate_result(fee, 0).unwrap(),
                        },
                        requests: requests.clone(),
                        lose_reply: std::sync::atomic::AtomicBool::new(false),
                        pending_replies: 0.into(),
                    },
                    Controls {
                        issue: true,
                        deliver: true,
                        inspect: true,
                        maximum_attempts: 8,
                        ..Default::default()
                    },
                    Duration::from_secs(30),
                )
                .unwrap();
                let now = crate::gateway::now_epoch().unwrap();
                assert_eq!(worker.tick(now).await.unwrap(), Tick::Progress);
                assert_eq!(worker.tick(now + 1).await.unwrap(), Tick::Progress);
                assert_eq!(f.count("cashu_treasury_completions").await, 0);
                assert!(f.ledger.begin_feed_attempt(1000).await.unwrap().is_none());
                assert_eq!(f.count("feed_attempts").await, 0);
                assert_eq!(f.count("settled_payments").await, 1);
                assert_eq!(f.count("event_log").await, 3);
                assert_eq!(
                    f.runtime
                        .reconcile_and_credit(&f.ledger, &f.event())
                        .await
                        .unwrap(),
                    SettlementOutcome::TreasurySettlementAccepted
                );
                assert_eq!(
                    f.runtime
                        .reconcile_and_credit(&f.ledger, &f.event())
                        .await
                        .unwrap(),
                    SettlementOutcome::Duplicate
                );
                assert_eq!(worker.tick(now + 100).await.unwrap(), Tick::Idle);
                assert_eq!(f.ledger.feed_credit_sats().await.unwrap(), 1000);
                assert_eq!(f.count("event_log").await, 3);
                assert_eq!(f.count("cashu_treasury_intents").await, 1);
                assert_eq!(f.count("cashu_treasury_completions").await, 1);
                assert_eq!(requests.lock().unwrap().len(), 1);
                let id = f.ledger.begin_feed_attempt(1000).await.unwrap().unwrap();
                f.ledger.confirm_feed_attempt(id).await.unwrap();
                assert_eq!(f.ledger.feed_credit_sats().await.unwrap(), 0);
                assert_eq!(f.count("feed_attempts").await, 1);
                assert_eq!(f.count("event_log").await, 4);
                let reward: i64 = sqlx::query_scalar(
                    "SELECT strike_sats FROM feed_reward_funding WHERE request_id=?",
                )
                .bind(id.to_string())
                .fetch_one(&f.pool)
                .await
                .unwrap();
                assert_eq!(reward, 600);
            }
        }
    }

    struct ScheduledWallet {
        registration: Arc<ReceiptPolicy>,
        data: MeltData,
        requests: Arc<std::sync::Mutex<Vec<(String, Uuid, String)>>>,
        lose_reply: std::sync::atomic::AtomicBool,
        pending_replies: std::sync::atomic::AtomicUsize,
    }
    impl crate::cashu::treasury_worker::Wallet for ScheduledWallet {
        async fn exchange(
            &self,
            request: &lightning_goats_cashu_receipt::treasury_rpc::Request,
        ) -> Result<crate::cashu::treasury_client::Response> {
            use lightning_goats_cashu_receipt::treasury_rpc::{Command, FEE_TIME_VERSION, Phase, Reply};
            let action = match &request.command {
                Command::Retain(_) => "retain",
                Command::Status(_) => "status",
            };
            self.requests.lock().unwrap().push((
                action.into(),
                request.intent().id,
                hex_digest(request.intent().digest()?),
            ));
            if self.lose_reply.swap(false, Ordering::SeqCst) {
                bail!("fixture acknowledgment lost")
            }
            if self.pending_replies.fetch_update(Ordering::SeqCst, Ordering::SeqCst, |count| count.checked_sub(1)).is_ok() {
                return Ok(crate::cashu::treasury_client::Response {
                    reply: Reply {
                        version: request.version,
                        request_id: request.request_id,
                        intent_id: self.data.intent_id,
                        intent_digest: self.data.intent_digest.clone(),
                        phase: Phase::Bound,
                        report: None,
                        fee_recorded_at: None,
                    },
                    report: None,
                });
            }
            let fee_recorded_at = (request.version == FEE_TIME_VERSION).then_some(0);
            let report = PeerMelt::authenticate_with_fee_time(
                &self.registration,
                &fixture_peer(&self.registration),
                &serde_json::to_vec(&self.data)?,
                fee_recorded_at,
            )?;
            let reply = Reply {
                version: request.version,
                request_id: request.request_id,
                intent_id: self.data.intent_id,
                intent_digest: self.data.intent_digest.clone(),
                phase: Phase::Reconciled,
                report: Some(serde_json::from_slice(&serde_json::to_vec(&self.data)?)?),
                fee_recorded_at,
            };
            Ok(crate::cashu::treasury_client::Response {
                reply,
                report: Some(report),
            })
        }
    }

    #[tokio::test]
    async fn operator_reconciles_only_the_original_with_a_pending_wallet_and_exact_retry() -> Result<()> {
        use crate::cashu::operator_reconciliation::Approval;
        let f = Fixture::new(&[250, 150], 600, 400, 0, unchanged).await;
        f.issue().await;
        let now = crate::gateway::now_epoch()?;
        let mut decision = json!({
            "version":1, "purpose":"reconcile_original_cashu_settlement",
            "core_epoch":f.registration.core_epoch, "wallet_instance":f.registration.wallet_instance,
            "wallet_epoch":f.registration.wallet_epoch, "intent_id":f.intent.id,
            "intent_digest":hex_digest(f.intent.digest()?), "receive_request_id":f.state.receive_request_id,
            "receive_id":f.state.receive_id, "config_sha256":"11".repeat(32),
            "approved_at":now, "expires_at":now+300,
        });
        let approval: Approval = serde_json::from_value(decision.clone())?;
        let wallet = ScheduledWallet {
            registration: Arc::new(copy_registration(&f.registration)),
            data: f.report(0, 0).validate(&f.registration, &f.intent)?.clone(),
            requests: Arc::default(),
            lose_reply: false.into(),
            pending_replies: 0.into(),
        };
        async fn forbidden_strike() -> Result<StrikeClient> {
            panic!("retained or mismatched work must not use Strike")
        }
        let review = || std::future::ready(Ok(f.runtime.client.clone()));
        let original = f.state.api_calls.load(Ordering::SeqCst);

        for (key, wrong) in [
            ("receive_request_id", json!(Uuid::new_v4())),
            ("intent_digest", json!("22".repeat(32))),
            ("wallet_epoch", json!(Uuid::new_v4())),
        ] {
            let mut wrong_decision = decision.clone();
            wrong_decision[key] = wrong;
            let wrong: Approval = serde_json::from_value(wrong_decision)?;
            assert!(wrong.prepare(&f.ledger, &f.registration, &f.policy.destination, forbidden_strike(), &PlanningOnly).await.is_err());
        }
        assert_eq!(f.state.api_calls.load(Ordering::SeqCst), original);

        let preview = approval.prepare(&f.ledger, &f.registration, &f.policy.destination, review(), &wallet).await?;
        let preview = serde_json::to_value(approval.finish(&f.ledger, &f.registration, preview, false).await?)?;
        assert_eq!(preview["strike_verified_this_run"], true);
        assert_eq!(preview["wallet_report_observed"], true);
        assert_eq!(preview["strike_deposit_retained"], false);
        assert_eq!(preview["wallet_report_retained"], false);
        assert_eq!(preview["settlement_complete"], false);
        assert_eq!(f.count("cashu_treasury_deposits").await, 0);

        wallet.lose_reply.store(true, Ordering::SeqCst);
        let pending = approval.prepare(&f.ledger, &f.registration, &f.policy.destination, review(), &wallet).await?;
        let pending = serde_json::to_value(approval.finish(&f.ledger, &f.registration, pending, true).await?)?;
        assert_eq!(pending["strike_deposit_retained"], true);
        assert_eq!(pending["wallet_status_unavailable"], true);
        assert_eq!(pending["settlement_complete"], false);
        assert_eq!(f.count("cashu_treasury_deposits").await, 1);
        assert!(f.ledger.begin_feed_attempt(1000).await?.is_none());

        let reply = approval.prepare(&f.ledger, &f.registration, &f.policy.destination, forbidden_strike(), &wallet).await?;
        let completion = serde_json::to_value(approval.finish(&f.ledger, &f.registration, reply, true).await?)?;
        assert_eq!(completion["strike_verified_this_run"], false);
        assert_eq!(completion["wallet_report_retained"], true);
        assert_eq!(completion["settlement_complete"], true);
        assert_eq!(f.state.api_calls.load(Ordering::SeqCst), original + 4);
        let replay = approval.prepare(&f.ledger, &f.registration, &f.policy.destination, forbidden_strike(), &PlanningOnly).await?;
        let replay = serde_json::to_value(approval.finish(&f.ledger, &f.registration, replay, true).await?)?;
        assert_eq!(replay["settlement_complete"], true);
        assert_eq!(replay["wallet_report_observed"], false);
        assert_eq!(f.count("cashu_treasury_wallet_results").await, 1);
        assert_eq!(f.count("cashu_treasury_completions").await, 1);
        assert_eq!(f.count("settled_payments").await, 1);
        assert_eq!(f.count("event_log").await, 3);
        {
            let requests = wallet.requests.lock().unwrap();
            assert_eq!(requests.len(), 3);
            assert!(requests.iter().all(|(action, id, digest)| action == "status"
                && *id == f.intent.id && Some(digest.as_str()) == decision["intent_digest"].as_str()));
        }
        decision["receive_id"] = json!(Uuid::new_v4());
        let changed: Approval = serde_json::from_value(decision)?;
        assert!(changed.prepare(&f.ledger, &f.registration, &f.policy.destination, forbidden_strike(), &PlanningOnly).await.is_err());

        let feed = f.ledger.begin_feed_attempt(1000).await?.context("missing funded attempt")?;
        f.ledger.confirm_feed_attempt(feed).await?;
        assert!(f.ledger.begin_feed_attempt(1000).await?.is_none());
        assert_eq!(f.count("feed_attempts").await, 1);
        Ok(())
    }

    #[tokio::test]
    async fn core_treasury_worker_reopens_after_lost_ack_and_waits_for_independent_strike() {
        use crate::cashu::treasury_worker::{Controls, Tick, Worker};
        for delivery_still_enabled in [false, true] {
            let f = Fixture::new(&[8], 0, 7, 1, |_| {}).await;
            let event = f.event();
            let requests = Arc::new(std::sync::Mutex::new(Vec::new()));
            let data = MeltData {
                version: 1,
                intent_id: f.intent.id,
                intent_digest: hex_digest(f.intent.digest().unwrap()),
                quote_id: "original-quote".into(),
                operation_evidence_digest: "ab".repeat(32),
                sources: f.intent.allocate_result(1, 0).unwrap(),
            };
            let registration = Arc::new(f.registration);
            let mut policy = Arc::new(f.policy);
            let wallet = ScheduledWallet {
                registration: registration.clone(),
                data,
                requests: requests.clone(),
                lose_reply: std::sync::atomic::AtomicBool::new(true),
                pending_replies: 0.into(),
            };
            let controls = Controls {
                plan: false,
                sweep: Default::default(),
                issue: true,
                deliver: true,
                inspect: true,
                maximum_attempts: 1,
            };
            let now = crate::gateway::now_epoch().unwrap();
            let worker = Worker::new(
                f.ledger.clone(),
                f.runtime.clone(),
                registration.clone(),
                policy.clone(),
                wallet,
                controls.clone(),
                Duration::from_secs(30),
            )
            .unwrap();
            assert_eq!(worker.tick(now).await.unwrap(), Tick::Progress);
            let issued = f
                .ledger
                .cashu_treasury_invoice(f.intent.id)
                .await
                .unwrap()
                .unwrap();
            assert_eq!(worker.tick(now + 1).await.unwrap(), Tick::RetryRetained);
            assert_eq!(worker.tick(now + 2).await.unwrap(), Tick::Idle);
            drop(worker);
            if !delivery_still_enabled {
                // Upgrade a historical wallet work row with exhausted delivery.
                sqlx::raw_sql(
                    "DROP TABLE cashu_treasury_inspections;
                     DELETE FROM _sqlx_migrations WHERE version=28;",
                )
                .execute(&f.pool)
                .await
                .unwrap();
            }
            if delivery_still_enabled {
                Arc::get_mut(&mut policy).unwrap().enabled = false;
            }
            let reopened = LedgerStore::connect(&format!(
                "sqlite://{}",
                f._directory.path().join("strike.db").display()
            ))
            .await
            .unwrap();
            let data = MeltData {
                version: 1,
                intent_id: f.intent.id,
                intent_digest: hex_digest(f.intent.digest().unwrap()),
                quote_id: "original-quote".into(),
                operation_evidence_digest: "ab".repeat(32),
                sources: f.intent.allocate_result(1, 0).unwrap(),
            };
            let wallet = ScheduledWallet {
                registration: registration.clone(),
                data,
                requests: requests.clone(),
                lose_reply: std::sync::atomic::AtomicBool::new(false),
                pending_replies: 2.into(),
            };
            let worker = Worker::new(
                reopened.clone(),
                f.runtime.clone(),
                registration,
                policy,
                wallet,
                Controls {
                    issue: false,
                    deliver: delivery_still_enabled,
                    ..controls
                },
                Duration::from_secs(30),
            )
            .unwrap();
            assert_eq!(worker.tick(now + 36).await.unwrap(), Tick::RetryRetained);
            assert_eq!(worker.tick(now + 70).await.unwrap(), Tick::Idle);
            assert_eq!(worker.tick(now + 71).await.unwrap(), Tick::RetryRetained);
            assert_eq!(worker.tick(now + 111).await.unwrap(), Tick::Progress);
            assert_eq!(worker.tick(now + 1000).await.unwrap(), Tick::Idle);
            let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM cashu_treasury_completions")
                .fetch_one(&f.pool)
                .await
                .unwrap();
            assert_eq!(count, 0, "wallet evidence alone cannot settle funding");
            assert_eq!(
                f.runtime
                    .reconcile_and_credit(&reopened, &event)
                    .await
                    .unwrap(),
                SettlementOutcome::TreasurySettlementAccepted
            );
            assert_eq!(
                f.runtime
                    .reconcile_and_credit(&reopened, &event)
                    .await
                    .unwrap(),
                SettlementOutcome::Duplicate
            );
            let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM cashu_treasury_completions")
                .fetch_one(&f.pool)
                .await
                .unwrap();
            assert_eq!(count, 1);
            assert_eq!(reopened.feed_credit_sats().await.unwrap(), 8);
            assert_eq!(
                reopened
                    .cashu_treasury_invoice(f.intent.id)
                    .await
                    .unwrap()
                    .unwrap()
                    .invoice,
                issued.invoice
            );
            let seen = requests.lock().unwrap();
            assert_eq!(seen.len(), 4);
            assert_eq!(seen[0].0, "retain");
            for request in &seen[1..] {
                assert_eq!(request.0, "status");
                assert_eq!(seen[0].1, request.1);
                assert_eq!(seen[0].2, request.2);
            }
        }
    }

    #[tokio::test]
    async fn private_cashu_status_tracks_original_evidence_and_feeder_state() -> Result<()> {
        let f = Fixture::with_planning(&[8], 992, 7, 1, |_| {}, Some(false)).await;
        let url = format!("sqlite://{}", f._directory.path().join("strike.db").display());
        let reader = LedgerStore::connect_recovery_read_only(&url).await?;
        assert!(reader.inspect_cashu_settlement(Uuid::nil()).await.is_err());
        assert!(reader.inspect_cashu_settlement(Uuid::new_v4()).await.is_err());
        let original = reader.inspect_cashu_settlement(f.intent.id).await?;
        assert!(!original.issuance_claimed);
        assert!(original.receive_request_id.is_none());
        assert_eq!(
            original.linked_feed_plan.as_ref().unwrap().pending_sources_covered,
            Some(false)
        );

        f.issue().await;
        assert_eq!(
            f.runtime.reconcile_and_credit(&f.ledger, &f.event()).await?,
            SettlementOutcome::TreasurySettlementAccepted
        );
        let partial = reader.inspect_cashu_settlement(f.intent.id).await?;
        assert!(partial.issuance_claimed);
        assert_eq!(partial.receive_request_id, Some(f.state.receive_request_id));
        assert_eq!(partial.verified_receive_id, Some(f.state.receive_id));
        assert!(partial.strike_deposit_verified_at.is_some());
        assert!(!partial.wallet_report_retained);
        assert!(!partial.settlement_complete);
        assert_eq!(
            partial.linked_feed_plan.unwrap().pending_sources_covered,
            Some(false)
        );

        f.ledger.record_cashu_melt_result(&f.registration, &f.report(1, 0)).await?;
        let settled = reader.inspect_cashu_settlement(f.intent.id).await?;
        assert!(settled.wallet_report_retained);
        assert!(settled.settlement_complete);
        assert_eq!(settled.settled_fee_sats, Some(1));
        assert_eq!(settled.settled_change_sats, Some(0));
        let plan = settled.linked_feed_plan.as_ref().unwrap();
        assert_eq!(plan.id, original.linked_feed_plan.unwrap().id);
        assert_eq!(plan.threshold_sats, 1000);
        assert_eq!(plan.pending_sources_covered, Some(true));
        assert!(plan.feeder_request_id.is_none());
        let json = serde_json::to_string(&settled)?;
        for private in [&f.state.invoice, &f.state.payment_hash, &f.state.description_hash] {
            assert!(!json.contains(private));
        }
        assert_eq!(f.count("feed_attempts").await, 0);
        assert_eq!(f.ledger.feed_credit_sats().await?, 1000);

        let request = f.ledger.begin_feed_attempt(1000).await?.context("expected funded plan")?;
        let dispatched = reader.inspect_cashu_settlement(f.intent.id).await?.linked_feed_plan.unwrap();
        assert_eq!(dispatched.id, plan.id);
        assert_eq!(dispatched.state, "dispatched");
        assert_eq!(dispatched.pending_sources_covered, None);
        assert_eq!(dispatched.feeder_request_id, Some(request));
        assert_eq!(dispatched.feeder_status.as_deref(), Some("intent_committed"));
        f.ledger.confirm_feed_attempt(request).await?;
        let confirmed = reader.inspect_cashu_settlement(f.intent.id).await?.linked_feed_plan.unwrap();
        assert_eq!(confirmed.feeder_status.as_deref(), Some("confirmed"));
        assert_eq!(f.ledger.feed_credit_sats().await?, 0);
        Ok(())
    }

    #[tokio::test]
    async fn core_treasury_pause_preserves_delivery_budget_and_expiry_uses_only_status() -> Result<()> {
        use crate::cashu::treasury_worker::{Controls, Tick, Worker};
        for expired in [false, true] {
            let f = Fixture::new(&[8], 0, 7, 1, |_| {}).await;
            let registration = Arc::new(copy_registration(&f.registration));
            let policy = Arc::new(f.policy.clone());
            let requests = Arc::new(std::sync::Mutex::new(Vec::new()));
            let make_worker = |deliver, lose_reply| Worker::new(
                f.ledger.clone(),
                f.runtime.clone(),
                registration.clone(),
                policy.clone(),
                ScheduledWallet {
                    registration: registration.clone(),
                    data: f.report(1, 0).validate(&registration, &f.intent).unwrap().clone(),
                    requests: requests.clone(),
                    lose_reply: std::sync::atomic::AtomicBool::new(lose_reply),
                    pending_replies: 1.into(),
                },
                Controls {
                    issue: true,
                    deliver,
                    inspect: true,
                    maximum_attempts: 1,
                    ..Default::default()
                },
                Duration::from_secs(1),
            );
            let now = crate::gateway::now_epoch()?;
            let paused = make_worker(false, true)?;
            assert_eq!(paused.tick(now).await?, Tick::Progress);
            assert_eq!(paused.tick(now + 1).await?, Tick::RetryRetained);
            let attempts: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM cashu_treasury_work WHERE stage='wallet'",
            ).fetch_one(&f.pool).await?;
            assert_eq!(attempts, 0);
            drop(paused);

            let resumed = make_worker(true, false)?;
            let delivery = if expired {
                let issued = f.ledger.cashu_treasury_invoice(f.intent.id).await?.unwrap();
                let invoice: lightning_invoice::Bolt11Invoice = issued.invoice.parse()?;
                i64::try_from(invoice.expires_at().unwrap().as_secs())?
            } else {
                now + 2
            };
            assert_eq!(resumed.tick(delivery).await?, Tick::RetryRetained);
            assert_eq!(resumed.tick(delivery + 30).await?, Tick::Progress);
            let commands = requests.lock().unwrap().iter()
                .map(|request| request.0.clone()).collect::<Vec<_>>();
            assert_eq!(commands, ["status", if expired { "status" } else { "retain" }, "status"]);
            assert_eq!(f.count("cashu_treasury_completions").await, 0);
            assert_eq!(f.ledger.feed_credit_sats().await?, 8);
        }
        Ok(())
    }

    async fn lightning(ledger: &LedgerStore, sats: u64) {
        ledger
            .record_payment(&SettledPayment {
                source: "strike".into(),
                source_id: Uuid::new_v4().to_string(),
                payment_hash: None,
                address_user: "herd".into(),
                credit_pool: "herd".into(),
                amount_msat: sats * 1000,
                settled_at: Some(1700000000),
                context_json: None,
            })
            .await
            .unwrap();
    }
    fn unchanged(_: &mut MockState) {}

    #[tokio::test]
    async fn mixed_three_rail_zero_fee_transfer_is_private_and_unlocks_one_original_feed() {
        let f = Fixture::new(&[250, 150], 600, 400, 0, unchanged).await;
        assert_eq!(f.ledger.feed_credit_sats().await.unwrap(), 1000);
        assert!(f.ledger.begin_feed_attempt(1000).await.unwrap().is_none());
        f.issue().await;
        // Provider callback can precede the wallet's lost execution response.
        assert_eq!(
            f.runtime
                .reconcile_and_credit(&f.ledger, &f.event())
                .await
                .unwrap(),
            SettlementOutcome::TreasurySettlementAccepted
        );
        assert!(f.ledger.begin_feed_attempt(1000).await.unwrap().is_none());
        assert_eq!(f.ledger.feed_credit_sats().await.unwrap(), 1000);
        assert_eq!(f.count("settled_payments").await, 1);
        assert_eq!(f.count("event_log").await, 3);
        assert_eq!(
            f.runtime
                .reconcile_and_credit(&f.ledger, &f.event())
                .await
                .unwrap(),
            SettlementOutcome::Duplicate
        );
        assert!(
            f.ledger
                .record_cashu_melt_result(&f.registration, &f.report(0, 0))
                .await
                .unwrap()
        );
        let id = f.ledger.begin_feed_attempt(1000).await.unwrap().unwrap();
        f.ledger.confirm_feed_attempt(id).await.unwrap();
        assert_eq!(f.ledger.feed_credit_sats().await.unwrap(), 0);
        assert_eq!(f.count("event_log").await, 4);
        let strike: i64 =
            sqlx::query_scalar("SELECT strike_sats FROM feed_reward_funding WHERE request_id=?")
                .bind(id.to_string())
                .fetch_one(&f.pool)
                .await
                .unwrap();
        assert_eq!(strike, 600); // Cashu reward provenance remains separately off.
        assert_eq!(f.count("cashu_treasury_completions").await, 1);
        assert_eq!(f.count("zap_receipt_jobs").await, 0);
        for receipt in &f.receipts {
            f.ledger
                .import_cashu_receipt(
                    &f.registration,
                    &fixture_authenticate(&f.registration, &receipt.encode().unwrap()).unwrap(),
                )
                .await
                .unwrap();
        }
        f.issue().await;
        assert!(
            f.ledger
                .record_cashu_melt_result(&f.registration, &f.report(0, 0))
                .await
                .unwrap()
        );
        assert_eq!(
            f.runtime
                .reconcile_and_credit(&f.ledger, &f.event())
                .await
                .unwrap(),
            SettlementOutcome::Duplicate
        );
        assert_eq!(f.count("event_log").await, 4);
        assert_eq!(f.ledger.feed_credit_sats().await.unwrap(), 0);
        assert!(f.ledger.begin_feed_attempt(1000).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn actual_fees_and_owned_change_conserve_custody_and_never_reduce_original_credit() {
        let f = Fixture::new(&[400], 600, 398, 2, unchanged).await;
        f.issue().await;
        assert!(
            !f.ledger
                .record_cashu_melt_result(&f.registration, &f.report(1, 1))
                .await
                .unwrap()
        );
        assert!(f.ledger.begin_feed_attempt(1000).await.unwrap().is_none());
        f.runtime
            .reconcile_and_credit(&f.ledger, &f.event())
            .await
            .unwrap();
        let totals: (i64, i64, i64, i64) = sqlx::query_as(
            "SELECT input_sats,deposit_sats,fee_sats,change_sats FROM cashu_treasury_completions",
        )
        .fetch_one(&f.pool)
        .await
        .unwrap();
        assert_eq!(totals, (400, 398, 1, 1));
        assert_eq!(f.ledger.feed_credit_sats().await.unwrap(), 1000);
        assert!(f.ledger.begin_feed_attempt(1000).await.unwrap().is_none()); // The returned sat remains custody.
        let source = f.intent.sources[0].ledger_id;
        let plan = crate::ledger::TreasuryPlan {
            id: Uuid::new_v4(),
            mint_id: "mint-a".into(),
            sources: vec![(source, 1)],
            deposit_sats: 1,
            maximum_fee_sats: 0,
        };
        f.ledger
            .reserve_cashu_treasury(&f.registration, &f.policy, &plan)
            .await
            .unwrap();
        let extra = crate::ledger::TreasuryPlan {
            id: Uuid::new_v4(),
            ..plan
        };
        assert!(
            f.ledger
                .reserve_cashu_treasury(&f.registration, &f.policy, &extra)
                .await
                .is_err()
        );
        lightning(&f.ledger, 1).await;
        let id = f.ledger.begin_feed_attempt(1000).await.unwrap().unwrap();
        f.ledger.confirm_feed_attempt(id).await.unwrap();
        assert_eq!(f.ledger.feed_credit_sats().await.unwrap(), 1);
        assert_eq!(f.count("cashu_treasury_deposits").await, 1);
    }

    #[tokio::test]
    async fn an_uncertain_creation_can_only_adopt_original_resource_and_disabled_mode_keeps_reads()
    {
        let mut f = Fixture::new(&[400], 600, 400, 0, unchanged).await;
        f.ledger
            .claim_cashu_treasury_issuance(f.intent.id)
            .await
            .unwrap()
            .unwrap();
        assert!(
            f.runtime
                .create_cashu_treasury_invoice(&f.ledger, &f.registration, &f.policy, f.intent.id)
                .await
                .unwrap()
                .is_none()
        );
        assert_eq!(f.state.api_calls.load(Ordering::SeqCst), 0);
        f.policy.enabled = false;
        let adopted = f
            .runtime
            .adopt_cashu_treasury_invoice(&f.ledger, f.intent.id, f.state.receive_request_id)
            .await
            .unwrap();
        assert!(adopted.intent == f.intent);
        assert!(
            f.runtime
                .create_cashu_treasury_invoice(&f.ledger, &f.registration, &f.policy, f.intent.id)
                .await
                .unwrap()
                .is_some()
        );
        assert_eq!(f.state.api_calls.load(Ordering::SeqCst), 1);
        assert_eq!(f.count("cashu_treasury_claims").await, 1);
        assert_eq!(f.count("cashu_treasury_issuances").await, 1);
        assert_eq!(f.ledger.feed_credit_sats().await.unwrap(), 1000);
    }

    #[tokio::test]
    async fn pending_wallet_report_conflicts_and_custody_races_never_release_original_sources() {
        let f = Fixture::new(&[400], 600, 398, 2, unchanged).await;
        f.issue().await;
        assert!(
            !f.ledger
                .record_cashu_melt_result(&f.registration, &f.report(1, 1))
                .await
                .unwrap()
        );
        assert!(
            f.ledger
                .record_cashu_melt_result(&f.registration, &f.report(2, 0))
                .await
                .is_err()
        );
        let plan = crate::ledger::TreasuryPlan {
            id: Uuid::new_v4(),
            mint_id: "mint-a".into(),
            sources: vec![(f.intent.sources[0].ledger_id, 1)],
            deposit_sats: 1,
            maximum_fee_sats: 0,
        };
        assert!(
            f.ledger
                .reserve_cashu_treasury(&f.registration, &f.policy, &plan)
                .await
                .is_err()
        );
        assert!(f.ledger.begin_feed_attempt(1000).await.unwrap().is_none());
        assert_eq!(f.count("cashu_treasury_completions").await, 0);
        assert_eq!(f.count("cashu_treasury_deposits").await, 0);
        assert_eq!(f.ledger.feed_credit_sats().await.unwrap(), 1000);
    }

    #[tokio::test]
    async fn fiat_or_conflicting_provider_receipts_and_wrong_credentials_are_never_donations() {
        let mut f = Fixture::new(&[400], 600, 400, 0, |state| {
            state.received_amount = Some(json!({"amount":state.amount_btc,"currency":"USD"}))
        })
        .await;
        let correct = f.policy.destination.credential_fingerprint.clone();
        f.policy.destination.credential_fingerprint = "00".repeat(32);
        // The retained intent remains bound to the real credential; changing new
        // policy cannot create an invoice under another destination registration.
        assert!(
            f.runtime
                .create_cashu_treasury_invoice(&f.ledger, &f.registration, &f.policy, f.intent.id)
                .await
                .is_err()
        );
        assert_eq!(f.state.api_calls.load(Ordering::SeqCst), 0);
        f.policy.destination.credential_fingerprint = correct;
        f.issue().await;
        assert!(
            f.runtime
                .reconcile_and_credit(&f.ledger, &f.event())
                .await
                .is_err()
        );
        assert_eq!(f.count("cashu_treasury_deposits").await, 0);
        assert_eq!(f.count("settled_payments").await, 1);
        assert_eq!(f.count("event_log").await, 2);
        assert_eq!(f.ledger.feed_credit_sats().await.unwrap(), 1000);
        assert!(
            f.ledger
                .record_payment(&SettledPayment {
                    source: "strike".into(),
                    source_id: f.state.receive_id.to_string(),
                    payment_hash: Some(f.state.payment_hash.clone()),
                    address_user: "herd".into(),
                    credit_pool: "herd".into(),
                    amount_msat: 400000,
                    settled_at: None,
                    context_json: None
                })
                .await
                .is_err()
        );
    }
    include!("strike_cashu_combined_fixture.rs");
}
