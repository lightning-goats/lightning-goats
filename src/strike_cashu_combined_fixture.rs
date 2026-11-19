// Financial composition fixture with optional authenticated receipt/treasury IPC.
// Baseline file-coordinated modes remain for fault isolation.
mod combined_fixture {
    use super::*;
    use crate::cashu::treasury_worker::{Controls, Tick, Wallet, Worker};
    use anyhow::ensure;
    use lightning_goats_cashu_receipt::treasury_rpc::{Command, FEE_TIME_VERSION, Phase, Reply, Request};
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;
    use std::path::{Path, PathBuf};

    // This test captures logical checkpoints at controlled points in the
    // fixture. The production collector uses the pinned SQLite backup API.
    async fn snapshot(pool: &sqlx::SqlitePool, target: &Path) -> Result<()> {
        ensure!(!target.exists(), "fixture snapshot already exists");
        sqlx::query("VACUUM INTO ?")
            .bind(target.to_str().unwrap())
            .execute(pool)
            .await?;
        #[cfg(unix)]
        std::fs::set_permissions(target, std::fs::Permissions::from_mode(0o600))?;
        Ok(())
    }

    async fn assert_restored_core_fenced(directory: &Path) -> Result<()> {
        let path = directory.join("strike.db");
        let original = std::fs::read(&path)?;
        std::fs::write(
            directory.join(crate::state_maintenance::RECOVERY_MARKER),
            b"synthetic restore requires reconciliation",
        )?;
        let Err(error) = LedgerStore::connect(&format!("sqlite://{}", path.display())).await else {
            anyhow::bail!("fenced core unexpectedly opened");
        };
        assert!(error.chain().any(|cause| cause.to_string()
            == "restored journal requires explicit external reconciliation before startup"));
        assert_eq!(std::fs::read(&path)?, original);
        for name in ["strike.db-wal", "strike.db-shm", "strike.db-journal"] {
            assert!(!directory.join(name).exists());
        }
        Ok(())
    }

    fn publish(root: &Path, name: &str, value: &impl serde::Serialize) -> Result<()> {
        let temporary = root.join(format!("{name}.pending"));
        let target = root.join(name);
        ensure!(
            !temporary.exists() && !target.exists(),
            "fixture output already exists"
        );
        std::fs::write(&temporary, serde_json::to_vec(value)?)?;
        std::fs::rename(temporary, target)?;
        Ok(())
    }
    async fn read_when_ready<T: serde::de::DeserializeOwned>(root: &Path, name: &str) -> Result<T> {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(120);
        loop {
            match std::fs::read(root.join(name)) {
                Ok(bytes) => {
                    ensure!(bytes.len() <= 65536, "fixture input exceeds bound");
                    return Ok(serde_json::from_slice(&bytes)?);
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            }
            ensure!(
                tokio::time::Instant::now() < deadline,
                "fixture peer deadline"
            );
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    }
    type ProviderStates = Arc<std::sync::Mutex<Vec<MockState>>>;
    async fn residual_create(
        State(states): State<ProviderStates>,
        headers: HeaderMap,
        Json(body): Json<Value>,
    ) -> axum::response::Response {
        let selected = states
            .lock()
            .unwrap()
            .iter()
            .find(|s| body["bolt11"]["descriptionHash"] == s.description_hash)
            .cloned();
        match selected {
            Some(state) => create_handler(State(state), headers, Json(body))
                .await
                .into_response(),
            None => StatusCode::BAD_REQUEST.into_response(),
        }
    }
    async fn residual_request(
        State(states): State<ProviderStates>,
        axum::extract::Path(id): axum::extract::Path<Uuid>,
    ) -> axum::response::Response {
        let selected = states
            .lock()
            .unwrap()
            .iter()
            .find(|s| s.receive_request_id == id)
            .cloned();
        match selected {
            Some(state) => request_handler(State(state)).await.into_response(),
            None => StatusCode::NOT_FOUND.into_response(),
        }
    }
    async fn residual_receives(
        State(states): State<ProviderStates>,
        axum::extract::Path(id): axum::extract::Path<Uuid>,
        query: Query<ReceiveQuery>,
    ) -> axum::response::Response {
        let selected = states
            .lock()
            .unwrap()
            .iter()
            .find(|s| s.receive_request_id == id)
            .cloned();
        match selected {
            Some(state) => receive_handler(State(state), query).await.into_response(),
            None => StatusCode::NOT_FOUND.into_response(),
        }
    }
    fn spawn_residual_provider(listener: TcpListener, states: ProviderStates) {
        let app = Router::new()
            .route("/v1/receive-requests", post(residual_create))
            .route("/v1/receive-requests/{id}", get(residual_request))
            .route("/v1/receive-requests/{id}/receives", get(residual_receives))
            .with_state(states);
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
    }
    struct FixtureWallet {
        root: PathBuf,
        registration: Arc<ReceiptPolicy>,
        lost_reply: bool,
        ledger: LedgerStore,
        pool: SqlitePool,
        runtime: StrikeRuntime,
        event: StrikeCompletedReceiveEvent,
        treasury_socket: Option<PathBuf>,
        round: u64,
        expected_credit: u64,
    }
    impl Wallet for FixtureWallet {
        async fn exchange(
            &self,
            request: &Request,
        ) -> Result<crate::cashu::treasury_client::Response> {
            let Command::Retain(issued) = &request.command else {
                bail!("unexpected fixture inspection")
            };
            if let Some(socket) = &self.treasury_socket {
                let deadline = tokio::time::Instant::now() + Duration::from_secs(120);
                while !socket.exists() {
                    ensure!(
                        tokio::time::Instant::now() < deadline,
                        "treasury IPC listener deadline"
                    );
                    tokio::time::sleep(Duration::from_millis(25)).await;
                }
                let client = crate::cashu::treasury_client::Client::new(
                    socket.clone(),
                    self.registration.clone(),
                    Duration::from_secs(5),
                    1,
                )?;
                let mut response = client.send(request).await?;
                let mut provider_verified = false;
                while response.report.is_none() {
                    ensure!(
                        tokio::time::Instant::now() < deadline,
                        "treasury IPC report deadline"
                    );
                    if self.lost_reply
                        && !provider_verified
                        && response.reply.phase == Phase::Submitted
                    {
                        assert_eq!(
                            self.runtime
                                .reconcile_and_credit(&self.ledger, &self.event)
                                .await?,
                            SettlementOutcome::TreasurySettlementAccepted
                        );
                        assert_eq!(
                            count(&self.pool, "cashu_treasury_completions").await?,
                            self.round as i64
                        );
                        assert_eq!(
                            count(&self.pool, "cashu_treasury_deposits").await?,
                            self.round as i64 + 1
                        );
                        assert_eq!(self.ledger.feed_credit_sats().await?, self.expected_credit);
                        assert!(self.ledger.begin_feed_attempt(1000).await?.is_none());
                        publish(
                            &self.root,
                            if self.round == 0 {
                                "provider-verified.json"
                            } else {
                                "residual-provider-verified.json"
                            },
                            &issued.intent.id,
                        )?;
                        provider_verified = true;
                    }
                    tokio::time::sleep(Duration::from_millis(25)).await;
                    let status = Request {
                        version: request.version,
                        request_id: Uuid::new_v4(),
                        command: Command::Status(Box::new(issued.intent.clone())),
                    };
                    response = client.send(&status).await?;
                }
                ensure!(
                    !self.lost_reply || provider_verified,
                    "lost reply bypassed early core observation"
                );
                // Status replies authenticate the retained result, but carry the
                // status request's correlation ID. Return an actual reply to the
                // caller's original Retain request, exercising its safe retry.
                let repeated = client.send(request).await?;
                ensure!(
                    repeated.report.is_some()
                        && repeated.reply.fee_recorded_at.is_some()
                        && repeated.reply.fee_recorded_at == response.reply.fee_recorded_at,
                    "retained treasury result or its fee time changed"
                );
                return Ok(repeated);
            }
            publish(&self.root, "issued.json", issued)?;
            if self.lost_reply {
                let submitted: Uuid = read_when_ready(&self.root, "submitted.json").await?;
                assert_eq!(submitted, issued.intent.id);
                assert_eq!(
                    self.runtime
                        .reconcile_and_credit(&self.ledger, &self.event)
                        .await?,
                    SettlementOutcome::TreasurySettlementAccepted
                );
                assert_eq!(
                    count(&self.pool, "cashu_treasury_completions").await?,
                    self.round as i64
                );
                assert_eq!(
                    count(&self.pool, "cashu_treasury_deposits").await?,
                    self.round as i64 + 1
                );
                assert_eq!(self.ledger.feed_credit_sats().await?, self.expected_credit);
                assert!(self.ledger.begin_feed_attempt(1000).await?.is_none());
                assert_eq!(count(&self.pool, "feed_attempts").await?, 0);
                assert_eq!(count(&self.pool, "event_log").await?, 3);
                publish(&self.root, "provider-verified.json", &submitted)?;
            }
            let data: MeltData = read_when_ready(&self.root, "report.json").await?;
            data.validate(&issued.intent)?;
            let fee_recorded_at = (request.version == FEE_TIME_VERSION).then_some(0);
            let report = PeerMelt::authenticate_with_fee_time(
                &self.registration,
                &fixture_peer(&self.registration),
                &serde_json::to_vec(&data)?,
                fee_recorded_at,
            )?;
            Ok(crate::cashu::treasury_client::Response {
                reply: Reply {
                    version: request.version,
                    request_id: request.request_id,
                    intent_id: issued.intent.id,
                    intent_digest: hex_digest(issued.intent.digest()?),
                    phase: Phase::Reconciled,
                    report: Some(data),
                    fee_recorded_at,
                },
                report: Some(report),
            })
        }
    }
    async fn count(pool: &SqlitePool, table: &str) -> Result<i64> {
        Ok(sqlx::query_scalar(&format!("SELECT COUNT(*) FROM {table}"))
            .fetch_one(pool)
            .await?)
    }

    #[tokio::test]
    #[ignore = "requires explicit combined core/wallet fixture runner"]
    async fn combined_settlement_core_child() -> Result<()> {
        use sha2::Digest;
        let root = PathBuf::from(std::env::var("LG_CASHU_COMBINED_FIXTURE")?);
        let receipt_ipc = std::env::var("LG_CASHU_COMBINED_RECEIPT_IPC").as_deref() == Ok("1");
        let private = if receipt_ipc {
            PathBuf::from(std::env::var("LG_CASHU_COMBINED_PRIVATE")?)
        } else {
            root.clone()
        };
        crate::private_ipc::private_state(&private)?;
        let treasury_ipc = std::env::var("LG_CASHU_COMBINED_TREASURY_IPC").as_deref() == Ok("1");
        ensure!(
            !treasury_ipc || receipt_ipc,
            "treasury IPC requires receipt IPC identities"
        );
        let http_intake = std::env::var("LG_CASHU_COMBINED_HTTP_INTAKE").as_deref() == Ok("1");
        ensure!(
            !http_intake || treasury_ipc,
            "HTTP composition requires financial IPC"
        );
        let repeat_request =
            std::env::var("LG_CASHU_COMBINED_REPEAT_REQUEST").as_deref() == Ok("1");
        ensure!(
            !repeat_request || http_intake,
            "repeated request requires HTTP composition"
        );
        let settle_change = std::env::var("LG_CASHU_COMBINED_SETTLE_CHANGE").as_deref() == Ok("1");
        let multi_mint = std::env::var("LG_CASHU_COMBINED_MULTI_MINT").as_deref() == Ok("1");
        ensure!(
            !settle_change || (http_intake && !repeat_request),
            "residual settlement requires ordinary HTTP and its own mode"
        );
        let receipt_count = 2 + usize::from(repeat_request || settle_change);
        let initial_credit = 1000 + 16 * u64::from(settle_change);
        let contribution_count = 1 + i64::try_from(receipt_count)?;
        let lost_reply = std::env::var("LG_CASHU_COMBINED_LOST_REPLY").as_deref() == Ok("1");
        let funded_rollback =
            std::env::var("LG_CASHU_COMBINED_FUNDED_ROLLBACK").as_deref() == Ok("1");
        let fees = std::env::var("LG_CASHU_COMBINED_FEES").as_deref() == Ok("1");
        let returned_change =
            fees && std::env::var("LG_CASHU_COMBINED_NO_CHANGE").as_deref() != Ok("1");
        let conversion_fee_sats = if fees {
            2 + u64::from(repeat_request)
        } else {
            0
        };
        ensure!(
            !multi_mint
                || (http_intake && fees && !returned_change && !repeat_request && !settle_change),
            "multi-mint composition requires its own HTTP fee-only mode"
        );
        let mint_ids = if multi_mint {
            vec!["fixture-a", "fixture-b"]
        } else {
            vec!["fixture"]
        };
        let (directory, ledger) = ledger().await;
        ledger.initialize_reward_funding().await?;
        ledger.configure_feeder_credit_cap(1000).await?;
        lightning(&ledger, 600).await;
        let registration = Arc::new(ReceiptPolicy {
            wallet_uid: if receipt_ipc {
                std::env::var("LG_CASHU_COMBINED_WALLET_UID")?.parse()?
            } else {
                60001
            },
            wallet_instance: Uuid::new_v4(),
            wallet_epoch: Uuid::new_v4(),
            core_epoch: ledger.overlay_stream_id().await?,
            import_enabled: true,
            limits: ImportLimits {
                receipts_per_24h: 10,
                unlimited_receipts_per_24h: false,
                credit_sats_per_24h: 1000,
                maximum_custody_sats: 1000,
            },
            approved: mint_ids
                .iter()
                .map(|id| {
                    (
                        (id.to_string(), "fixture-v1".into()),
                        MintImportLimit {
                            maximum_gross_sats: 1000,
                            maximum_net_sats: 1000,
                        },
                    )
                })
                .collect(),
        });
        let pool = crate::sqlite::connect_existing_durable(
            &format!("sqlite://{}", directory.path().join("strike.db").display()),
            1,
        )
        .await?;
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let origin = format!("http://{}/", listener.local_addr()?);
        let mut state = state();
        let policy = Arc::new(Policy {
            enabled: true,
            version: "combined-fixture-v1".into(),
            destination: Destination {
                origin: origin.clone(),
                account_id: Uuid::new_v4(),
                credential_fingerprint: hex::encode(Sha256::digest(state.api_key.as_bytes())),
                network: "bitcoin".into(),
            },
            approved_mints: mint_ids
                .iter()
                .map(|id| {
                    (
                        id.to_string(),
                        MintLimits {
                            minimum_deposit_sats: 1,
                            maximum_deposit_sats: 400,
                            maximum_input_sats: 400,
                            maximum_fee_sats: conversion_fee_sats,
                            maximum_in_flight_sats: 400,
                        },
                    )
                })
                .collect(),
            invoice_expiry_seconds: 300,
            limits: Limits {
                minimum_deposit_sats: 1,
                maximum_deposit_sats: 400,
                maximum_input_sats: 400,
                maximum_fee_sats: conversion_fee_sats,
                operations_per_24h: 0,
                unlimited_operations_per_24h: true,
                input_sats_per_24h: 0,
                unlimited_input_sats_per_24h: true,
                // The first residual-mode intent reserves two but spends one.
                // The second can fit only after the verified fee replaces it.
                fee_sats_per_24h: Some(if settle_change { 3 } else { conversion_fee_sats * (1 + u64::from(multi_mint)) }),
                maximum_in_flight_sats: 400,
            },
        });
        let receipt_socket = root.parent().unwrap().join("core-run/receipts.sock");
        let receipt_server = if receipt_ipc {
            use crate::cashu::receipt_server::{Config, Mint, Server};
            let config = Config {
                socket: receipt_socket.clone(),
                ipc_gid: std::env::var("LG_CASHU_COMBINED_IPC_GID")?.parse()?,
                wallet_uid: registration.wallet_uid,
                wallet_instance: registration.wallet_instance,
                wallet_epoch: registration.wallet_epoch,
                core_epoch: registration.core_epoch,
                import_enabled: true,
                receipts_per_24h: 10,
                unlimited_receipts_per_24h: false,
                credit_sats_per_24h: 1000,
                maximum_custody_sats: 1000,
                approved: mint_ids
                    .iter()
                    .map(|id| Mint {
                        mint_id: id.to_string(),
                        policy_version: "fixture-v1".into(),
                        maximum_gross_sats: 1000,
                        maximum_net_sats: 1000,
                    })
                    .collect(),
                deadline_seconds: 5,
                connections_per_second: 64,
            };
            let server = Server::new(config, ledger.clone())?;
            Some(tokio::spawn(async move {
                server.serve(std::future::pending()).await
            }))
        } else {
            None
        };
        let mut bootstrap = json!({"wallet_instance":registration.wallet_instance,
            "wallet_epoch":registration.wallet_epoch,"core_epoch":registration.core_epoch,
            "destination":policy.destination,"treasury_policy":policy.version});
        if receipt_ipc {
            bootstrap["receipt_socket"] = json!(receipt_socket);
            bootstrap["core_uid"] = json!(crate::private_ipc::uid()?);
        }
        let treasury_socket = root.parent().unwrap().join("wallet-run/treasury.sock");
        if treasury_ipc {
            bootstrap["treasury_socket"] = json!(treasury_socket);
        }
        let public_server = if http_intake {
            let forwarder = Arc::new(crate::cashu::intake::Forwarder::new(
                crate::cashu::intake::Policy {
                    socket: root.parent().unwrap().join("wallet-run/intake.sock"),
                    wallet_uid: registration.wallet_uid,
                    wallet_instance: registration.wallet_instance,
                    wallet_epoch: registration.wallet_epoch,
                    receive_cashu: true,
                    receive_nutzap: false,
                    body_bytes: 65536,
                    concurrent_requests: 2,
                    deadline: Duration::from_secs(5),
                },
            )?);
            let app = crate::cashu::http::router(Some(forwarder));
            let listener = TcpListener::bind("127.0.0.1:0").await?;
            bootstrap["http_origin"] = json!(format!("http://{}", listener.local_addr()?));
            Some(tokio::spawn(
                async move { axum::serve(listener, app).await },
            ))
        } else {
            None
        };
        publish(&root, "bootstrap.json", &bootstrap)?;
        let data: Vec<ReceiptData> = if receipt_ipc {
            let deadline = tokio::time::Instant::now() + Duration::from_secs(120);
            while count(&pool, "cashu_receipt_inbox").await? != i64::try_from(receipt_count)? {
                ensure!(
                    tokio::time::Instant::now() < deadline,
                    "receipt IPC import deadline"
                );
                tokio::time::sleep(Duration::from_millis(25)).await;
            }
            let rows: Vec<String> = sqlx::query_scalar(
                "SELECT receipt_json FROM cashu_receipt_inbox ORDER BY ledger_entry_id",
            )
            .fetch_all(&pool)
            .await?;
            rows.into_iter()
                .map(|row| serde_json::from_str(&row).map_err(Into::into))
                .collect::<Result<_>>()?
        } else {
            read_when_ready(&root, "receipts.json").await?
        };
        assert_eq!(data.len(), receipt_count);
        assert_eq!((&*data[0].transport, data[0].net_sats), ("nutzap", 250));
        for (index, ordinary) in data[1..].iter().enumerate() {
            assert_eq!(
                (&*ordinary.transport, ordinary.net_sats),
                (
                    "cashu",
                    if settle_change && index == 1 {
                        16
                    } else if repeat_request {
                        75
                    } else {
                        150
                    }
                )
            );
        }
        let mut receipts = Vec::new();
        for value in data {
            let receipt = WireReceipt::validate(value)?;
            if !receipt_ipc {
                ledger
                    .import_cashu_receipt(
                        &registration,
                        &fixture_authenticate(&registration, &receipt.encode()?)?,
                    )
                    .await?;
            }
            receipts.push(receipt);
        }
        assert_eq!(ledger.feed_credit_sats().await?, initial_credit);
        assert!(ledger.begin_feed_attempt(1000).await?.is_none());
        let runtime = StrikeRuntime::new(
            StrikeClient::new(&origin, state.api_key.into())?,
            StrikeWebhookVerifier::new("synthetic-webhook".into())?,
        );
        let controls = Controls {
            plan: true,
            inspect: true,
            maximum_attempts: 8,
            ..Default::default()
        };
        let planner = Worker::new(
            ledger.clone(),
            runtime.clone(),
            registration.clone(),
            policy.clone(),
            PlanningOnly,
            controls,
            Duration::from_secs(30),
        )?;
        let now = i64::try_from(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_secs(),
        )?;
        assert_eq!(planner.tick(now).await?, Tick::Progress);
        if !multi_mint {
            assert_eq!(planner.tick(now + 1).await?, Tick::Idle);
        }
        let id: String = sqlx::query_scalar("SELECT id FROM cashu_treasury_intents")
            .fetch_one(&pool)
            .await?;
        let intent = ledger.cashu_treasury_intent(id.parse()?).await?.unwrap();
        assert_eq!(
            intent.sources.len(),
            if multi_mint {
                1
            } else {
                receipt_count - usize::from(settle_change)
            }
        );
        if multi_mint {
            assert_eq!(intent.mint_id, "fixture-a");
            assert_eq!(
                (
                    intent.input_sats,
                    intent.deposit_sats,
                    intent.maximum_fee_sats
                ),
                (250, 248, 2)
            );
            assert_eq!(receipts[0].data().mint_id, "fixture-a");
            assert_eq!(receipts[1].data().mint_id, "fixture-b");
        }
        assert_eq!(count(&pool, "feed_financial_treasury").await?, 1);
        state.description_hash = hex_digest(intent.digest()?);
        state.payment_hash = hex::encode(Sha256::digest([55u8; 32]));
        state.amount_btc = msat_to_btc_decimal(intent.deposit_sats * 1000);
        state.credited_amount = Some(json!({"amount":state.amount_btc,"currency":"BTC"}));
        state.invoice = crate::test_invoices::invoice_with_payment_hash(
            intent.deposit_sats * 1000,
            &state.description_hash,
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?,
            Sha256::digest([55u8; 32]).into(),
        );
        let provider_states = Arc::new(std::sync::Mutex::new(vec![state.clone()]));
        if settle_change || multi_mint {
            spawn_residual_provider(listener, provider_states.clone());
        } else {
            spawn_mock_on(listener, state.clone());
        }
        drop(planner);
        let event = StrikeCompletedReceiveEvent {
            event_id: Uuid::new_v4(),
            receive_request_id: state.receive_request_id,
            receive_id: state.receive_id,
        };
        let worker = Worker::new(
            ledger.clone(),
            runtime.clone(),
            registration.clone(),
            policy.clone(),
            FixtureWallet {
                root: root.clone(),
                registration: registration.clone(),
                lost_reply,
                ledger: ledger.clone(),
                pool: pool.clone(),
                runtime: runtime.clone(),
                event: event.clone(),
                treasury_socket: treasury_ipc.then_some(treasury_socket.clone()),
                round: 0,
                expected_credit: initial_credit,
            },
            Controls {
                issue: true,
                deliver: true,
                inspect: true,
                maximum_attempts: 8,
                ..Default::default()
            },
            Duration::from_secs(60),
        )?;
        assert_eq!(worker.tick(now + 2).await?, Tick::Progress);
        if multi_mint {
            assert_eq!(count(&pool, "cashu_treasury_issuances").await?, 1);
            assert_eq!(count(&pool, "cashu_treasury_deposits").await?, 0);
            snapshot(&pool, &private.join("mint-custody-core-before.db")).await?;
        }
        if funded_rollback {
            let old = private.join("funded-before-melt");
            std::fs::create_dir(&old)?;
            assert_eq!(count(&pool, "cashu_treasury_issuances").await?, 1);
            assert_eq!(count(&pool, "cashu_treasury_deposits").await?, 0);
            snapshot(&pool, &old.join("strike.db")).await?;
        }
        assert_eq!(worker.tick(now + 3).await?, Tick::Progress);
        assert_eq!(
            count(&pool, "cashu_treasury_completions").await?,
            i64::from(lost_reply)
        );
        if !lost_reply {
            assert!(ledger.begin_feed_attempt(1000).await?.is_none());
        }
        assert_eq!(count(&pool, "feed_attempts").await?, 0);
        assert_eq!(count(&pool, "event_log").await?, contribution_count);
        assert_eq!(
            runtime.reconcile_and_credit(&ledger, &event).await?,
            if lost_reply {
                SettlementOutcome::Duplicate
            } else {
                SettlementOutcome::TreasurySettlementAccepted
            }
        );
        assert_eq!(
            runtime.reconcile_and_credit(&ledger, &event).await?,
            SettlementOutcome::Duplicate
        );
        assert_eq!(worker.tick(now + 100).await?, Tick::Idle);
        assert_eq!(count(&pool, "settled_payments").await?, 1);
        assert_eq!(count(&pool, "event_log").await?, contribution_count);
        assert_eq!(ledger.feed_credit_sats().await?, initial_credit);
        if returned_change && !multi_mint {
            // Even with an affordable one-sat invoice, this plan must not spend its
            // own returned fee reserve again. It is still available to a later batch.
            assert!(
                ledger
                    .plan_cashu_feed_treasury(&registration, &policy)
                    .await?
                    .is_none()
            );
            assert_eq!(count(&pool, "cashu_treasury_intents").await?, 1);
        }
        if settle_change || multi_mint {
            assert!(
                multi_mint || returned_change,
                "residual mode requires returned change"
            );
            assert!(ledger.begin_feed_attempt(1000).await?.is_none());
            let request = crate::cashu::treasury_sweep::Request {
                id: Uuid::new_v4(),
                mint_id: "fixture".into(),
                maximum_input_sats: 17,
            };
            let second = if multi_mint {
                ledger
                    .plan_cashu_feed_treasury(&registration, &policy)
                    .await?
                    .unwrap()
            } else {
                ledger
                    .plan_operator_cashu_settlement(&registration, &policy, &request, true)
                    .await?
                    .unwrap()
            };
            assert_eq!(
                (
                    second.input_sats,
                    second.deposit_sats,
                    second.maximum_fee_sats
                ),
                if multi_mint {
                    (150, 148, 2)
                } else {
                    (17, 15, 2)
                }
            );
            assert_eq!(second.sources.len(), if multi_mint { 1 } else { 2 });
            assert_eq!(
                second.sources.iter().map(|s| s.input_sats).sum::<u64>(),
                if multi_mint { 150 } else { 17 }
            );
            assert!(second.sources.iter().any(|s| s.acquisition_id
                == receipts.last().unwrap().data().acquisition_id
                && s.input_sats == if multi_mint { 150 } else { 16 }));
            if multi_mint {
                assert_eq!(second.mint_id, "fixture-b");
                assert_eq!(count(&pool, "feed_financial_treasury").await?, 2);
            }
            let mut state2 = state.clone();
            state2.receive_request_id = Uuid::new_v4();
            state2.receive_id = Uuid::new_v4();
            state2.description_hash = hex_digest(second.digest()?);
            state2.payment_hash = hex::encode(Sha256::digest([56u8; 32]));
            state2.amount_btc = msat_to_btc_decimal(second.deposit_sats * 1000);
            state2.credited_amount = Some(json!({"amount":state2.amount_btc,"currency":"BTC"}));
            state2.invoice = crate::test_invoices::invoice_with_payment_hash(
                second.deposit_sats * 1000,
                &state2.description_hash,
                std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?,
                Sha256::digest([56u8; 32]).into(),
            );
            provider_states.lock().unwrap().push(state2.clone());
            publish(&root, "residual-ready.json", &second.id)?;
            let event2 = StrikeCompletedReceiveEvent {
                event_id: Uuid::new_v4(),
                receive_request_id: state2.receive_request_id,
                receive_id: state2.receive_id,
            };
            let worker2 = Worker::new(
                ledger.clone(),
                runtime.clone(),
                registration.clone(),
                policy.clone(),
                FixtureWallet {
                    root: root.clone(),
                    registration: registration.clone(),
                    lost_reply,
                    ledger: ledger.clone(),
                    pool: pool.clone(),
                    runtime: runtime.clone(),
                    event: event2.clone(),
                    treasury_socket: Some(treasury_socket.clone()),
                    round: 1,
                    expected_credit: initial_credit,
                },
                Controls {
                    issue: true,
                    deliver: true,
                    inspect: true,
                    maximum_attempts: 8,
                    ..Default::default()
                },
                Duration::from_secs(60),
            )?;
            assert_eq!(worker2.tick(now + 101).await?, Tick::Progress);
            assert_eq!(worker2.tick(now + 102).await?, Tick::Progress);
            assert_eq!(
                runtime.reconcile_and_credit(&ledger, &event2).await?,
                if lost_reply {
                    SettlementOutcome::Duplicate
                } else {
                    SettlementOutcome::TreasurySettlementAccepted
                }
            );
            assert_eq!(
                runtime.reconcile_and_credit(&ledger, &event2).await?,
                SettlementOutcome::Duplicate
            );
            assert_eq!(count(&pool, "cashu_treasury_completions").await?, 2);
            assert_eq!(count(&pool, "cashu_treasury_deposits").await?, 2);
            // Both treasury receives retain their own purpose-bound evidence.
            // The original native donation remains the only settled-payment row.
            assert_eq!(count(&pool, "settled_payments").await?, 1);
            assert_eq!(count(&pool, "event_log").await?, contribution_count);
            assert_eq!(ledger.feed_credit_sats().await?, initial_credit);
            if multi_mint {
                assert!(
                    ledger
                        .plan_cashu_feed_treasury(&registration, &policy)
                        .await?
                        .is_none()
                );
            } else {
                let replay = ledger
                    .plan_operator_cashu_settlement(&registration, &policy, &request, true)
                    .await?
                    .unwrap();
                assert!(replay == second);
            }
            assert_eq!(count(&pool, "cashu_treasury_intents").await?, 2);
            if settle_change {
                println!("COMBINED_CORE_RESIDUAL_CHANGE_SETTLED");
            }
            if multi_mint {
                println!("COMBINED_CORE_TWO_MINTS_FINANCIAL_PLAN_VERIFIED");
            }
        } else if returned_change {
            // The retained one-sat change remains custody and original credit,
            // but is not yet settled coverage. Later native funding can proceed.
            assert!(ledger.begin_feed_attempt(1000).await?.is_none());
            lightning(&ledger, 1).await;
            assert_eq!(ledger.feed_credit_sats().await?, 1001);
        }
        let feeding = ledger.begin_feed_attempt(1000).await?.unwrap();
        ledger.confirm_feed_attempt(feeding).await?;
        assert_eq!(
            ledger.feed_credit_sats().await?,
            if settle_change {
                16
            } else {
                u64::from(returned_change)
            }
        );
        if receipt_ipc {
            publish(&root, "credit-consumed.json", &feeding)?;
            let replayed: u64 = read_when_ready(&root, "receipt-replayed.json").await?;
            ensure!(
                replayed == u64::try_from(receipt_count)?
                    && count(&pool, "cashu_receipt_inbox").await? == i64::try_from(receipt_count)?,
                "receipt replay changed imports"
            );
            println!("COMBINED_CORE_CDK_RECEIPT_SOCKET_VERIFIED");
        }
        for receipt in receipts {
            if !receipt_ipc {
                ledger
                    .import_cashu_receipt(
                        &registration,
                        &fixture_authenticate(&registration, &receipt.encode()?)?,
                    )
                    .await?;
            }
        }
        assert_eq!(
            ledger.feed_credit_sats().await?,
            if settle_change {
                16
            } else {
                u64::from(returned_change)
            }
        );
        assert!(ledger.begin_feed_attempt(1000).await?.is_none());
        assert_eq!(count(&pool, "feed_attempts").await?, 1);
        assert_eq!(
            count(&pool, "event_log").await?,
            contribution_count + 1 + i64::from(returned_change && !settle_change)
        );
        let reward: i64 =
            sqlx::query_scalar("SELECT strike_sats FROM feed_reward_funding WHERE request_id=?")
                .bind(feeding.to_string())
                .fetch_one(&pool)
                .await?;
        assert_eq!(
            reward,
            if returned_change && !settle_change {
                601
            } else {
                600
            }
        );
        let result = if multi_mint {
            json!({"credit_sats":0,"contributions":3,"synthetic_confirmed_feeds":1,"treasury_deposit_sats":396,
                "new_transfer_credit_sats":0,"original_granted_credit_sats":1000,"receive_fees_sats":2,"conversion_fee_sats":4,
                "retained_change_sats":0,"supplemental_native_sats":0,"settlement_operations":2,"configured_mints":2})
        } else if settle_change {
            json!({"credit_sats":16,"contributions":4,"synthetic_confirmed_feeds":1,"treasury_deposit_sats":413,
                "new_transfer_credit_sats":0,"original_granted_credit_sats":1016,"receive_fees_sats":3,"conversion_fee_sats":3,
                "retained_change_sats":0,"supplemental_native_sats":0,"settlement_operations":2})
        } else if fees {
            json!({"credit_sats":u64::from(returned_change),"contributions":contribution_count + i64::from(returned_change),"synthetic_confirmed_feeds":1,
                "treasury_deposit_sats":400-conversion_fee_sats,"new_transfer_credit_sats":0,
                "original_granted_credit_sats":1000,"receive_fees_sats":receipt_count,
                "conversion_fee_sats":conversion_fee_sats-u64::from(returned_change),"retained_change_sats":u64::from(returned_change),"supplemental_native_sats":u64::from(returned_change)})
        } else {
            json!({"credit_sats":0,"contributions":contribution_count,"synthetic_confirmed_feeds":1,
                "treasury_deposit_sats":400,"new_transfer_credit_sats":0})
        };
        publish(&root, "core-result.json", &result)?;
        if fees {
            println!("COMBINED_CORE_FEES_CHANGE_CONSERVED");
            if !returned_change {
                println!("COMBINED_CORE_FEE_ONLY_ORIGINAL_CREDIT_CONSUMED");
            }
        }
        if lost_reply {
            println!("COMBINED_CORE_EARLY_STRIKE_WAITS_FOR_WALLET_VERIFIED");
        }
        let installed_review =
            multi_mint && std::env::var("LG_CASHU_COMBINED_INSTALLED_REVIEW").as_deref() == Ok("1");
        if installed_review {
            let id: Uuid = read_when_ready(&root, "reference-credit.json").await?;
            let amount: i64 = sqlx::query_scalar(
                "SELECT net_sats FROM cashu_receipt_inbox WHERE acquisition_id=? AND wallet_instance=?",
            )
            .bind(id.to_string())
            .bind(registration.wallet_instance.to_string())
            .fetch_one(&pool)
            .await?;
            assert_eq!(amount, 16);
            assert_eq!(ledger.feed_credit_sats().await?, 16);
            assert_eq!(count(&pool, "cashu_receipt_inbox").await?, 3);
            assert_eq!(count(&pool, "cashu_treasury_deposits").await?, 2);
            assert_eq!(count(&pool, "feed_attempts").await?, 1);
        }
        if let Some(server) = receipt_server {
            server.abort();
        }
        if repeat_request {
            println!("COMBINED_CORE_REUSABLE_REQUEST_TWO_PAYMENTS_VERIFIED");
        }
        if http_intake {
            println!("COMBINED_CORE_PUBLIC_HTTP_FINANCIAL_IPC_VERIFIED");
        }
        if let Some(server) = public_server {
            server.abort();
        }
        if funded_rollback {
            snapshot(&pool, &private.join("funded-after-effects.db")).await?;
            assert_restored_core_fenced(&private.join("funded-before-melt")).await?;
        }
        if installed_review {
            snapshot(&pool, &private.join("mint-custody-core-after.db")).await?;
            publish(&root, "restore-core-ready.json", &true)?;
            ensure!(
                read_when_ready::<bool>(&root, "restore-review-finished.json").await?,
                "restore review did not verify the original provider outcomes"
            );
        }
        if treasury_ipc {
            println!("COMBINED_CORE_FULL_FINANCIAL_IPC_VERIFIED");
        }
        if settle_change || multi_mint {
            println!("COMBINED_CORE_TWO_SETTLEMENTS_ONE_SYNTHETIC_FEED");
        } else {
            println!("COMBINED_CORE_ONE_SETTLEMENT_ONE_SYNTHETIC_FEED");
        }
        Ok(())
    }
}
