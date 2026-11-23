//! Protected startup assembly. No mint requests, core credit or automatic initialization.
use crate::{
    ingress::Intake,
    journal::Journal,
    service_config::{Config, Prepared},
    service_keys::Keys,
    writer_lock::WriterLock,
};
use anyhow::{Result, ensure};
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

pub struct Runtime {
    pub policy: Prepared,
    pub keys: Keys,
    pub writer: Arc<WriterLock>,
    pub journal: Arc<Mutex<Journal>>,
    pub(crate) mint_databases:
        std::collections::BTreeMap<String, Arc<cdk_sqlite::WalletSqliteDatabase>>,
    pub(crate) config: Config,
}

impl Runtime {
    /// Offline operator action under the existing writer lock. Never starts workers.
    pub fn prepare_announcement(
        &self,
        id: uuid::Uuid,
        now: u64,
    ) -> Result<crate::publication::Intent> {
        self.journal
            .lock()
            .map_err(|_| anyhow::anyhow!("wallet journal unavailable"))?
            .prepare_configured_announcement(id, &self.config, now)
    }
    /// Run the bounded private listener and serialized acquisition worker together.
    /// Shutdown drops in-flight work; durable claims and original operation IDs remain.
    pub async fn serve(&self, shutdown: impl std::future::Future<Output = ()>) -> Result<()> {
        let mut worker = crate::scheduler::Worker::new(self)?;
        let delivery = crate::receipt_outbox::Worker::new(self)?;
        let mut relays = crate::relay_intake::Worker::new(self)?;
        let signing = crate::publication_signing::Worker::new(self)?;
        let publishing = crate::publication_delivery::Worker::new(self)?;
        let treasury = crate::treasury_scheduler::Worker::new(self)?;
        tokio::pin!(shutdown);
        tokio::select! {
            _ = &mut shutdown => Ok(()),
            result = async { tokio::try_join!(self.serve_intake(std::future::pending()), self.serve_treasury(std::future::pending()), worker.run(), delivery.run(), relays.run(), signing.run(), publishing.run(), treasury.run(), crate::collector_handoff::run(self)) } => result.map(|_| ()),
        }
    }
    /// Explicit provisioning action for a directory with keys but no databases.
    /// A partial failure is retained for reconciliation, never silently reset.
    pub async fn initialize(config: Config) -> Result<Self> {
        Self::open_inner(config, true).await
    }

    /// Normal restart requires both existing databases and their startup binding.
    pub async fn open(config: Config) -> Result<Self> {
        Self::open_inner(config, false).await
    }

    async fn open_inner(config: Config, initialize: bool) -> Result<Self> {
        ensure!(rusqlite::version() == "3.54.1", "unqualified SQLite engine");
        let policy = config.prepare()?;
        let keys = config.load_keys()?;
        let writer = Arc::new(WriterLock::acquire(&config.state_directory)?);
        for name in ["wallet.sqlite", "acquisition.sqlite"] {
            let path = config.state_directory.join(name);
            match std::fs::symlink_metadata(&path) {
                Ok(metadata) => ensure!(
                    !initialize && metadata.is_file() && metadata.len() > 0,
                    "wallet database inventory conflicts with startup mode"
                ),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    ensure!(
                        initialize,
                        "wallet database missing; restore before startup"
                    );
                }
                Err(error) => return Err(error.into()),
            }
            // Orphan sidecars are evidence of incomplete recovery, not a blank wallet.
            if initialize {
                for suffix in ["-wal", "-shm", "-journal"] {
                    match std::fs::symlink_metadata(
                        config.state_directory.join(format!("{name}{suffix}")),
                    ) {
                        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                        Err(error) => return Err(error.into()),
                        Ok(_) => anyhow::bail!(
                            "wallet database sidecar exists; reconcile before initialization"
                        ),
                    }
                }
            }
        }
        let mut journal = Journal::open(
            &config.state_directory.join("acquisition.sqlite"),
            config.wallet_epoch,
            config.proof_count,
        )?;
        // An older protected config may still recover existing operations and
        // exact receipts. It grants no authority for new intake without limits.
        journal.set_custody_limits(config.custody_limits.clone().unwrap_or_default());
        let binding = keys.binding();
        if !initialize {
            journal.runtime_registration(config.wallet_instance, &binding, false)?;
            journal.require_queued_receivers(&keys)?;
        }
        if initialize {
            drop(crate::wallet::open_wallet_storage(&config.state_directory).await?);
            journal.runtime_registration(config.wallet_instance, &binding, true)?;
        } else {
            // Inspect legacy state without invoking CDK migrations on restart.
            use std::os::unix::fs::MetadataExt;
            let metadata = std::fs::symlink_metadata(config.state_directory.join("wallet.sqlite"))?;
            ensure!(
                metadata.is_file()
                    && metadata.nlink() == 1
                    && metadata.uid() == config.runtime_uid
                    && metadata.mode() & 0o077 == 0,
                "legacy CDK store must retain private runtime ownership"
            );
        }
        // The old shared CDK file remains retained, never silently abandoned.
        let legacy_empty = {
            let connection = rusqlite::Connection::open_with_flags(
                config.state_directory.join("wallet.sqlite"),
                rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY
                    | rusqlite::OpenFlags::SQLITE_OPEN_NOFOLLOW,
            )?;
            let rows: i64 = connection.query_row(
                "SELECT (SELECT COUNT(*) FROM proof)+(SELECT COUNT(*) FROM wallet_sagas)",
                [],
                |r| r.get(0),
            )?;
            rows == 0
        };
        journal.require_cdk_namespaces(legacy_empty)?;
        journal.verify_mint_namespaces(&config.mints)?;
        let mut mint_databases = std::collections::BTreeMap::new();
        for mint in &config.mints {
            let create = journal.prepare_cdk_namespace(mint)?;
            let store = crate::wallet::open_mint_wallet_storage(
                &config.state_directory,
                mint,
                &writer,
                create,
            )
            .await?;
            if create {
                journal.complete_cdk_namespace(mint)?;
            }
            mint_databases.insert(mint.id.clone(), store);
        }
        Ok(Self {
            config,
            policy,
            keys,
            writer,
            journal: Arc::new(Mutex::new(journal)),
            mint_databases,
        })
    }

    /// Separate socket; paused admission can retain status/replay service.
    pub async fn serve_treasury(
        &self,
        shutdown: impl std::future::Future<Output = ()>,
    ) -> Result<()> {
        let Some(config) = self.config.treasury_intake.as_ref().filter(|c| c.enabled) else {
            shutdown.await;
            return Ok(());
        };
        let bindings = config.bindings(&self.config)?;
        crate::treasury_ingress::Intake {
            core_uid: self.config.core_uid,
            accept_new: config.accept_new,
            bindings: &bindings,
            journal: self.journal.clone(),
            writer: &self.writer,
        }
        .serve(
            &config.socket,
            self.config.ipc_gid,
            Duration::from_secs(self.config.deadline_seconds),
            self.config.connections_per_second,
            shutdown,
        )
        .await
    }

    pub async fn serve_intake(
        &self,
        shutdown: impl std::future::Future<Output = ()>,
    ) -> Result<()> {
        Intake {
            core_uid: self.config.core_uid,
            wallet_instance: self.config.wallet_instance,
            wallet_epoch: self.config.wallet_epoch,
            receive_cashu: self.config.receive_cashu,
            receive_nutzap: self.config.receive_nutzap,
            registry: &self.policy.registry,
            keys: &self.policy.keys,
            limits: &self.policy.limits,
            receiver: self.policy.receiver,
            recipient: &self.config.recipient,
            descriptors: &self.config.descriptors,
            retired_descriptors: &self.config.retired_descriptors,
            retained_receivers: &self.config.retained_receiving_keys,
            public_request: self.policy.public_request.as_ref(),
            journal: self.journal.clone(),
            writer: &self.writer,
        }
        .serve(
            &self.config.intake_socket,
            self.config.ipc_gid,
            Duration::from_secs(self.config.deadline_seconds),
            self.config.connections_per_second,
            shutdown,
        )
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cdk::nuts::SecretKey;
    use std::{os::unix::fs::PermissionsExt, path::Path};
    use uuid::Uuid;

    fn configuration(directory: &Path, instance: Uuid, epoch: Uuid) -> Result<Config> {
        let uid = rustix::process::geteuid().as_raw();
        Config::decode(&serde_json::to_vec(&serde_json::json!({
            "version":1,"runtime_uid":uid,"core_uid":uid+1,"ipc_gid":12347,
            "wallet_instance":instance,"wallet_epoch":epoch,
            "state_directory":directory,"intake_socket":"/run/cashu-fixture/intake.sock",
            "receiving_key":SecretKey::from_slice(&[3;32])?.public_key().to_hex(),
            "recipient":SecretKey::from_slice(&[4;32])?.public_key().x_only_public_key().to_string(),
            "descriptors":[],"mints":[],"keysets":[],"body_bytes":65536,"proof_count":64,
            "receipt_sats":10000,"deadline_seconds":5,"connections_per_second":8
        }))?)
    }
    fn keys(directory: &Path) -> Result<()> {
        for (name, bytes) in [
            ("wallet-seed.bin", vec![7; 64]),
            ("receiving-key.bin", vec![3; 32]),
        ] {
            let path = directory.join(name);
            std::fs::write(&path, bytes)?;
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
        }
        Ok(())
    }

    fn retained_fixture_journal(runtime: &Runtime) -> Result<Journal> {
        // Seed data from before the runtime required a custody policy.
        Journal::open(
            &runtime.config.state_directory.join("acquisition.sqlite"),
            runtime.config.wallet_epoch,
            runtime.config.proof_count,
        )
    }

    #[tokio::test]
    async fn rotated_wallet_preserves_registration_and_requires_keys_for_queued_proofs()
    -> Result<()> {
        use crate::{
            journal::{Transport, ValidatedIntake},
            service_config::RetainedReceiver,
        };
        let dir = crate::test_directory()?;
        let instance = Uuid::new_v4();
        let epoch = Uuid::new_v4();
        keys(dir.path())?;
        let initialized = Runtime::initialize(configuration(dir.path(), instance, epoch)?).await?;
        let original = initialized.keys.receiving.public_key();
        let binding = initialized.keys.binding();
        drop(initialized);
        let additional = |scalar| -> Result<cdk::nuts::PublicKey> {
            let key = SecretKey::from_slice(&[scalar; 32])?.public_key();
            let path = dir
                .path()
                .join(format!("receiving-key-{}.bin", key.to_hex()));
            std::fs::write(&path, [scalar; 32])?;
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
            Ok(key)
        };
        let active = additional(8)?;
        let retained = additional(9)?;
        let rotated = |keep_old| -> Result<Config> {
            let mut config = configuration(dir.path(), instance, epoch)?;
            config.receiving_key_anchor = Some(original.to_hex());
            config.receiving_key = active.to_hex();
            if keep_old {
                config.retained_receiving_keys.push(RetainedReceiver {
                    public_key: retained.to_hex(),
                    nutzap: None,
                });
            }
            Ok(config)
        };
        let runtime = Runtime::open(rotated(true)?).await?;
        assert_eq!(runtime.keys.binding(), binding);
        assert_eq!(runtime.keys.receiving.public_key(), active);
        assert_eq!(runtime.keys.signing_key(original)?.public_key(), original);
        let proof = crate::intake::tests::proof(
            serde_json::json!([
                "P2PK", {"nonce":"retained fixture","data":retained.to_hex()}
            ])
            .to_string(),
        );
        retained_fixture_journal(&runtime)?.admit(&ValidatedIntake {
            mint_id: "fixture",
            policy_version: "v1",
            transport: Transport::Cashu,
            source_ref: "original-queued-fixture",
            source_author: None,
            proofs: &[proof],
        })?;
        drop(runtime);
        assert!(Runtime::open(rotated(false)?).await.is_err());
        assert_eq!(Runtime::open(rotated(true)?).await?.keys.binding(), binding);
        Ok(())
    }

    fn announcement_configuration(directory: &Path, instance: Uuid, epoch: Uuid) -> Result<Config> {
        let mut config = configuration(directory, instance, epoch)?;
        config.receive_nutzap = true;
        config.mints = vec![crate::mint_policy::MintPolicy {
            id: "fixture".into(),
            advertised_url: "https://mint.example".into(),
            policy_version: "v1".into(),
            receive_cashu: false,
            receive_nutzap: true,
            bolt11_exit_qualified: true,
        }];
        let key = SecretKey::from_slice(&[8; 32])?.public_key();
        let keys = cdk::nuts::Keys::new(std::collections::BTreeMap::from([(8u64.into(), key)]));
        config.keysets = vec![crate::service_config::Keyset {
            mint_id: "fixture".into(),
            id: cdk::nuts::Id::v1_from_keys(&keys).to_string(),
            keys: std::collections::BTreeMap::from([(8, key.to_hex())]),
        }];
        config.relay_intake = Some(crate::relay_intake::Config {
            enabled: true,
            relays: vec!["wss://relay.example/".into()],
            historical_floor: 0,
            window_seconds: 300,
            overlap_seconds: 30,
            event_limit: 32,
        });
        config.prepare()?;
        Ok(config)
    }
    #[tokio::test]
    async fn treasury_listener_is_default_off_and_survives_rejected_connections() -> Result<()> {
        use std::os::unix::fs::MetadataExt;
        use tokio::io::AsyncReadExt;
        let dir = crate::test_directory()?;
        let ipc = crate::test_directory()?;
        std::fs::set_permissions(ipc.path(), std::fs::Permissions::from_mode(0o710))?;
        keys(dir.path())?;
        let mut config = configuration(dir.path(), Uuid::new_v4(), Uuid::new_v4())?;
        config.ipc_gid = std::fs::metadata(ipc.path())?.gid();
        config.mints = vec![crate::mint_policy::MintPolicy {
            id: "fixture".into(),
            advertised_url: "https://mint.example".into(),
            policy_version: "v1".into(),
            receive_cashu: false,
            receive_nutzap: false,
            bolt11_exit_qualified: false,
        }];
        let socket = ipc.path().join("treasury.sock");
        let (binding, _) = crate::treasury::tests::fixture();
        config.treasury_intake = Some(crate::treasury_config::Config {
            enabled: false,
            accept_new: false,
            socket: socket.clone(),
            core_epoch: binding.core_epoch,
            destination: binding.destination,
            policies: std::collections::BTreeMap::from([("fixture".into(), "v1".into())]),
        });
        let mut runtime = Runtime::initialize(config).await?;
        runtime.serve_treasury(async {}).await?;
        assert!(!socket.exists());
        let status = crate::status::Status::read(&runtime.config)?;
        assert!(!status.treasury_intake_enabled && !status.treasury_admission_enabled);
        runtime.config.treasury_intake.as_mut().unwrap().enabled = true;
        let status = crate::status::Status::read(&runtime.config)?;
        assert!(status.treasury_intake_enabled && !status.treasury_admission_enabled);
        let (stop, stopped) = tokio::sync::oneshot::channel();
        let listener = runtime.serve_treasury(async {
            let _ = stopped.await;
        });
        let rejected_clients = async {
            for _ in 0..2 {
                let mut stream = loop {
                    match tokio::net::UnixStream::connect(&socket).await {
                        Ok(stream) => break stream,
                        Err(_) => tokio::time::sleep(Duration::from_millis(10)).await,
                    }
                };
                let mut byte = [0; 1];
                assert_eq!(stream.read(&mut byte).await?, 0);
            }
            let _ = stop.send(());
            Ok::<_, anyhow::Error>(())
        };
        tokio::time::timeout(Duration::from_secs(4), async {
            tokio::try_join!(listener, rejected_clients)
        })
        .await??;
        assert!(
            runtime
                .journal
                .lock()
                .unwrap()
                .treasury_instruction(&binding_for_status(&runtime), Uuid::from_u128(5))
                .is_err()
        );
        Ok(())
    }

    fn binding_for_status(runtime: &Runtime) -> crate::treasury::TreasuryBinding {
        runtime
            .config
            .treasury_intake
            .as_ref()
            .unwrap()
            .bindings(&runtime.config)
            .unwrap()
            .remove("fixture")
            .unwrap()
    }

    #[tokio::test]
    async fn offline_announcement_preserves_original_identity_policy_and_completed_publication()
    -> Result<()> {
        let dir = crate::test_directory()?;
        keys(dir.path())?;
        let instance = Uuid::new_v4();
        let epoch = Uuid::new_v4();
        let operation = Uuid::new_v4();
        let runtime = Runtime::initialize(configuration(dir.path(), instance, epoch)?).await?;
        assert!(runtime.prepare_announcement(operation, 100).is_err());
        drop(runtime);
        let runtime =
            Runtime::open(announcement_configuration(dir.path(), instance, epoch)?).await?;
        assert!(runtime.prepare_announcement(Uuid::nil(), 100).is_err());
        let original = runtime.prepare_announcement(operation, 100)?;
        assert!(runtime.prepare_announcement(Uuid::new_v4(), 100).is_err());
        assert!(runtime.prepare_announcement(Uuid::new_v4(), 99).is_err());
        original.require_current_announcement(Some(&crate::publication::Intent::announcement(
            &runtime.config,
            0,
        )?))?;
        assert!(runtime.prepare_announcement(operation, 999)? == original);
        let id = format!("announcement:{operation}");
        assert!(
            runtime
                .journal
                .lock()
                .unwrap()
                .signed_publication(&id)?
                .is_none()
        );
        assert!(
            Runtime::open(announcement_configuration(dir.path(), instance, epoch)?)
                .await
                .is_err()
        );
        let raw = {
            let mut journal = runtime.journal.lock().unwrap();
            let raw = crate::publication::tests::sign(journal.publication_unsigned(&id)?)?;
            journal.freeze_signed_publication(&id, &raw)?;
            journal.freeze_publication_routes(
                &id,
                &["wss://relay.example/".into()],
                &crate::publication_relay::Discovery {
                    read_relays: vec![],
                    signed_list: None,
                },
                100,
            )?;
            let delivery = journal.claim_publication_delivery(100, 1)?.unwrap();
            journal.acknowledge_publication_relay(&id, &delivery.relay, &delivery.event_id)?;
            raw
        };
        drop(runtime);
        let runtime =
            Runtime::open(announcement_configuration(dir.path(), instance, epoch)?).await?;
        assert!(runtime.prepare_announcement(operation, 1000)? == original);
        assert_eq!(
            runtime.journal.lock().unwrap().signed_publication(&id)?,
            Some(raw.clone())
        );
        assert!(
            runtime
                .journal
                .lock()
                .unwrap()
                .claim_publication_delivery(2000, 1)?
                .is_none()
        );
        drop(runtime);
        let mut changed = announcement_configuration(dir.path(), instance, epoch)?;
        changed.relay_intake.as_mut().unwrap().relays = vec!["wss://different.example/".into()];
        let runtime = Runtime::open(changed).await?;
        assert!(runtime.prepare_announcement(operation, 2000).is_err());
        assert!(runtime.journal.lock().unwrap().publication_intent(&id)? == original);
        assert_eq!(
            runtime.journal.lock().unwrap().signed_publication(&id)?,
            Some(raw)
        );
        let replacement_id = Uuid::new_v4();
        let replacement = runtime.prepare_announcement(replacement_id, 2001)?;
        assert!(replacement != original);
        assert!(
            matches!(replacement,crate::publication::Intent::Announcement {created_at:2001,ref relays,..} if relays==&vec!["wss://different.example/".to_owned()])
        );
        assert!(
            runtime
                .journal
                .lock()
                .unwrap()
                .signed_publication(&format!("announcement:{replacement_id}"))?
                .is_none()
        );
        Ok(())
    }
    #[tokio::test]
    async fn health_reports_legacy_and_future_ages_without_inventing_times() -> Result<()> {
        use crate::journal::{Transport, ValidatedIntake};
        let directory = crate::test_directory()?;
        keys(directory.path())?;
        let config = configuration(directory.path(), Uuid::new_v4(), Uuid::new_v4())?;
        let runtime = Runtime::initialize(config).await?;
        let proofs = [crate::intake::tests::proof("private-health-input".into())];
        retained_fixture_journal(&runtime)?.admit_at(
            &ValidatedIntake {
                mint_id: "historical",
                policy_version: "v1",
                transport: Transport::Cashu,
                source_ref: "private-source",
                source_author: None,
                proofs: &proofs,
            },
            300,
        )?;
        let future = crate::status::Status::read_at(&runtime.config, 200)?;
        assert_eq!(future.health.queue.future_admission_timestamps, 1);
        assert_eq!(future.health.queue.oldest_pending_age_seconds, None);
        let db = rusqlite::Connection::open(directory.path().join("acquisition.sqlite"))?;
        db.execute("DELETE FROM acquisition_admission_times", [])?;
        let legacy = crate::status::Status::read_at(&runtime.config, 400)?;
        assert_eq!(legacy.health.queue.pending_age_unknown, 1);
        assert_eq!(legacy.health.queue.oldest_pending_age_seconds, None);
        db.execute("DROP TABLE acquisition_admission_times", [])?;
        let old_schema = crate::status::Status::read_at(&runtime.config, 400)?;
        assert_eq!(old_schema.health.queue.pending_age_unknown, 1);
        let recreated: bool = db.query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name='acquisition_admission_times')",
            [],
            |r| r.get(0),
        )?;
        assert!(!recreated);
        Ok(())
    }

    #[tokio::test]
    async fn health_detail_truncation_does_not_truncate_custody_or_pending_totals() -> Result<()> {
        use crate::journal::{Transport, ValidatedIntake};
        let directory = crate::test_directory()?;
        keys(directory.path())?;
        let config = configuration(directory.path(), Uuid::new_v4(), Uuid::new_v4())?;
        let runtime = Runtime::initialize(config).await?;
        let mut last = None;
        let mut retained = retained_fixture_journal(&runtime)?;
        for index in 0..70 {
            let proofs = [crate::intake::tests::proof(format!(
                "private-health-proof-{index}"
            ))];
            last = Some(retained.admit_at(
                &ValidatedIntake {
                    mint_id: &format!("mint-{index:03}"),
                    policy_version: "v1",
                    transport: Transport::Cashu,
                    source_ref: &format!("private-source-{index}"),
                    source_author: None,
                    proofs: &proofs,
                },
                100,
            )?);
        }
        let Some(crate::journal::Admission::New(id)) = last else {
            panic!("expected new receipt")
        };
        retained.prepare(id, b"synthetic original swap")?;
        retained.start_acquisition(id)?;
        retained.record_acquired(id, 7, 1, b"synthetic owned outputs")?;
        let status = crate::status::Status::read_at(&runtime.config, 200)?;
        assert_eq!(status.health.mints.len(), 64);
        assert!(status.health.mints_truncated);
        assert!(status.health.mints.iter().all(|m| !m.configured));
        assert_eq!(status.health.queue.pending_receipts, 70);
        assert_eq!(status.health.totals.unverified_input_sats, 552);
        assert_eq!(status.health.totals.journal_custody_sats, 7);
        assert!(
            status
                .health
                .mints
                .iter()
                .all(|m| m.funds.journal_custody_sats == 0)
        );
        let body = serde_json::to_string(&status)?;
        assert!(!body.contains("private-health-proof") && !body.contains("private-source"));
        Ok(())
    }

    #[tokio::test]
    async fn health_reports_retained_core_epochs_and_invalid_ack_json_without_leaks() -> Result<()>
    {
        use crate::journal::{Admission, Transport, ValidatedIntake};
        use lightning_goats_cashu_receipt::acknowledgment::{Acknowledgment, ImportResult};
        let directory = crate::test_directory()?;
        keys(directory.path())?;
        let mut config = configuration(directory.path(), Uuid::new_v4(), Uuid::new_v4())?;
        let expected_core = Uuid::new_v4();
        config.receipt_delivery = Some(crate::receipt_outbox::Config {
            collector_handoff: false,
            enabled: false,
            socket: "/run/core/receipts.sock".into(),
            core_epoch: expected_core,
            maximum_attempts: 0,
            deadline_seconds: 5,
        });
        let runtime = Runtime::initialize(config).await?;
        let proofs = [crate::intake::tests::proof("private-ack-proof".into())];
        let previous_core = Uuid::new_v4();
        let id = {
            let mut journal = retained_fixture_journal(&runtime)?;
            let Admission::New(id) = journal.admit_at(
                &ValidatedIntake {
                    mint_id: "fixture",
                    policy_version: "v1",
                    transport: Transport::Cashu,
                    source_ref: "private-ack-source",
                    source_author: None,
                    proofs: &proofs,
                },
                100,
            )?
            else {
                panic!("expected new receipt")
            };
            journal.prepare(id, b"fixture original swap")?;
            journal.start_acquisition(id)?;
            journal.record_acquired(id, 8, 0, b"fixture owned outputs")?;
            let receipt = journal.export_receipt(id)?;
            let wire = crate::receipt_wire::from_private(runtime.config.wallet_instance, &receipt)?;
            let ack = Acknowledgment {
                version: 1,
                wallet_instance: runtime.config.wallet_instance,
                wallet_epoch: runtime.config.wallet_epoch,
                core_epoch: previous_core,
                result: ImportResult {
                    retained_cashu: false,
                    acquisition_id: id,
                    receipt_digest: hex::encode(wire.digest()?),
                    event_seq: 1,
                    net_sats: 8,
                    original_feed_credit_sats: 8,
                },
            };
            journal.acknowledge_verified_credit(
                runtime.config.wallet_instance,
                previous_core,
                id,
                &ack.encode()?,
            )?;
            id
        };
        let status = crate::status::Status::read_at(&runtime.config, 200)?;
        assert_eq!(status.health.expected_core_epoch, Some(expected_core));
        assert_eq!(
            status.health.retained_acknowledgments_other_core_epochs,
            Some(1)
        );
        assert_eq!(status.health.malformed_retained_acknowledgments, 0);
        let db = rusqlite::Connection::open(directory.path().join("acquisition.sqlite"))?;
        // Disposable corruption fixture: malformed data must be counted, not echoed.
        db.execute(
            "UPDATE acquisitions SET core_ack=? WHERE id=?",
            rusqlite::params![b"private-invalid-ack".as_slice(), id.to_string()],
        )?;
        let status = crate::status::Status::read_at(&runtime.config, 200)?;
        assert_eq!(status.health.malformed_retained_acknowledgments, 1);
        let body = serde_json::to_string(&status)?;
        assert!(!body.contains("private-invalid-ack") && !body.contains("private-ack-proof"));
        Ok(())
    }

    #[tokio::test]
    async fn mismatched_wallet_health_is_fenced_and_cannot_resume_runtime() -> Result<()> {
        let directory = crate::test_directory()?;
        keys(directory.path())?;
        let original_epoch = Uuid::new_v4();
        let original_instance = Uuid::new_v4();
        let runtime = Runtime::initialize(configuration(
            directory.path(),
            original_instance,
            original_epoch,
        )?)
        .await?;
        let wrong = configuration(directory.path(), Uuid::new_v4(), Uuid::new_v4())?;
        let status = crate::status::Status::read_at(&wrong, 200)?;
        assert!(!status.registration_matches && status.recovery_required);
        assert_eq!(status.observed_wallet_epoch, original_epoch);
        assert_eq!(status.observed_wallet_instance, original_instance);
        drop(runtime);
        assert!(Runtime::open(wrong).await.is_err());
        Ok(())
    }

    #[tokio::test]
    async fn treasury_health_reports_current_stages_and_preserves_legacy_fee_history() -> Result<()>
    {
        use rusqlite::params;
        let directory = crate::test_directory()?;
        keys(directory.path())?;
        let mut config = configuration(directory.path(), Uuid::new_v4(), Uuid::new_v4())?;
        config.treasury_execution.maximum_attempts = 2;
        config.treasury_execution.fee_sats_per_24h = Some(5);
        let runtime = Runtime::initialize(config).await?;
        let db = rusqlite::Connection::open(directory.path().join("acquisition.sqlite"))?;
        let (_, mut issued) = crate::treasury::tests::fixture();
        issued.intent.maximum_fee_sats = 1;
        for index in 0..66 {
            issued.intent.id = Uuid::from_u128(100 + index);
            issued.receive_request_id = Uuid::from_u128(200 + index);
            issued.payment_hash = format!("{index:064x}");
            let state = match index {
                0 => "prepared",
                1 => "submitted",
                2 => "reconciled",
                _ => "bound",
            };
            db.execute(
                "INSERT INTO treasury_instructions(id,issued_json,receive_id,payment_hash,state)
                 VALUES(?,?,?,?,?)",
                params![
                    issued.intent.id.to_string(),
                    serde_json::to_vec(&issued)?,
                    issued.receive_request_id.to_string(),
                    issued.payment_hash,
                    state
                ],
            )?;
        }
        let prepared = Uuid::from_u128(100);
        db.execute(
            "INSERT INTO treasury_execution_admissions(intent_id,admitted_at,input_sats) VALUES(?,900,101)",
            [prepared.to_string()],
        )?;
        for (id, stage, due) in [
            (prepared, "reserve", 5), // Old stage lag must not leak into current status.
            (prepared, "submit", 900), // Exhausted retries have no scheduled wakeup.
            (Uuid::from_u128(101), "reconcile", 1200),
            (Uuid::from_u128(102), "cleanup", 950),
        ] {
            db.execute(
                "INSERT INTO treasury_work_schedule(intent_id,action,attempts,next_attempt) VALUES(?,?,2,?)",
                params![id.to_string(), stage, due],
            )?;
        }
        let status = crate::status::Status::read_at(&runtime.config, 1000)?;
        let treasury = &status.health.treasury;
        assert_eq!(treasury.pending_operations, 66);
        assert_eq!(treasury.submitted_outcomes_pending, 1);
        assert_eq!(treasury.paid_cleanups_pending, 1);
        assert_eq!(treasury.finite_retries_exhausted, 1);
        assert_eq!(treasury.operations.len(), 64);
        assert!(treasury.operations_truncated);
        assert_eq!(treasury.rolling_fee_commitment_sats, Some(1));
        assert_eq!(treasury.rolling_fee_remaining_sats, Some(4));
        assert_eq!(treasury.oldest_scheduled_due_at, Some(950));
        assert_eq!(status.health.queue.scheduled_lag_seconds, Some(50));
        assert_eq!(treasury.operations[0].stage, "reconcile");
        assert_eq!(treasury.operations[0].next_attempt_at, Some(1200));
        assert!(treasury.operations[0].continues_past_attempt_limit);
        assert!(!treasury.operations[0].retries_exhausted);
        assert_eq!(treasury.operations[1].stage, "cleanup");
        assert_eq!(treasury.operations[2].intent_id, prepared);
        assert_eq!(treasury.operations[2].stage, "submit");
        assert!(treasury.operations[2].retries_exhausted);
        let encoded = serde_json::to_string(&status)?;
        assert!(!encoded.contains(&issued.invoice));
        assert!(!encoded.contains(&issued.payment_hash));

        // Once cleanup is deferred, only the exhausted stage has a past due time.
        db.execute(
            "UPDATE treasury_work_schedule SET next_attempt=1200 WHERE action='cleanup'",
            [],
        )?;
        let deferred = crate::status::Status::read_at(&runtime.config, 1000)?;
        assert_eq!(deferred.health.treasury.oldest_scheduled_due_at, None);
        assert_eq!(deferred.health.queue.scheduled_lag_seconds, None);
        assert_eq!(deferred.health.treasury.finite_retries_exhausted, 1);

        // An older read-only image has no exact rolling fee observations.
        db.execute("DROP TABLE treasury_fee_completions", [])?;
        let legacy = crate::status::Status::read_at(&runtime.config, 1000)?;
        assert_eq!(legacy.health.treasury.rolling_fee_commitment_sats, None);
        assert_eq!(legacy.health.treasury.rolling_fee_remaining_sats, None);
        let recreated: bool = db.query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE name='treasury_fee_completions')",
            [],
            |row| row.get(0),
        )?;
        assert!(!recreated);
        Ok(())
    }

    #[tokio::test]
    async fn status_is_read_only_bounded_and_available_while_the_writer_is_active() -> Result<()> {
        use crate::journal::{HoldReason, Transport, ValidatedIntake};
        use cdk::{
            nuts::{Id, Proof},
            secret::Secret,
        };
        let dir = crate::test_directory()?;
        keys(dir.path())?;
        let mut config = configuration(dir.path(), Uuid::new_v4(), Uuid::new_v4())?;
        config.execution.maximum_attempts = 1;
        assert!(crate::status::Status::read(&config).is_err());
        assert!(!dir.path().join("acquisition.sqlite").exists());
        let runtime = Runtime::initialize(config).await?;
        let mut ids = Vec::new();
        let mut retained = retained_fixture_journal(&runtime)?;
        for index in 0..65 {
            let proofs = vec![Proof::new(
                8.into(),
                "00deadbeef123456".parse::<Id>()?,
                Secret::new(format!("private-proof-secret-{index}")),
                runtime.policy.receiver,
            )];
            let admitted = retained.admit_at(
                &ValidatedIntake {
                    mint_id: "fixture",
                    policy_version: "v1",
                    transport: Transport::Cashu,
                    source_ref: &format!("private-source-{index}"),
                    source_author: None,
                    proofs: &proofs,
                },
                100 + index,
            )?;
            if let crate::journal::Admission::New(id) = admitted {
                ids.push(id);
            }
        }
        runtime
            .journal
            .lock()
            .unwrap()
            .hold(ids[64], HoldReason::AmbiguousAcquisition)?;
        retained.claim_acquisition_work(100, true, false, 1, 5)?;
        std::fs::rename(
            dir.path().join("wallet-seed.bin"),
            dir.path().join("seed-hidden"),
        )?;
        let status = crate::status::Status::read_at(&runtime.config, 20000)?;
        assert_eq!(status.state_counts.get("validated"), Some(&64));
        assert_eq!(status.state_counts.get("held_for_reconciliation"), Some(&1));
        assert_eq!(status.exhausted_acquisitions, 1);
        assert!(
            !status
                .operations
                .iter()
                .any(|op| op.acquisition_inspection_only)
        );
        assert_eq!(status.health.queue.oldest_pending_age_seconds, Some(19900));
        assert_eq!(status.health.queue.scheduled_lag_seconds, Some(19890));
        assert_eq!(status.health.queue.pending_age_unknown, 0);
        assert_eq!(status.health.totals.unverified_input_sats, 520);
        assert_eq!(status.health.totals.journal_custody_sats, 0);
        assert!(status.registration_matches);
        assert_eq!(status.operations.len(), 64);
        assert!(status.operations_truncated);
        assert_eq!(status.operations[0].acquisition_id, ids[64]);
        assert_eq!(
            status.operations[0].hold_reason.as_deref(),
            Some("ambiguous_acquisition")
        );
        let json = serde_json::to_string(&status)?;
        assert!(!json.contains("private-proof-secret") && !json.contains("private-source"));
        assert_eq!(runtime.journal.lock().unwrap().state(ids[0])?, "validated");
        let repeated = crate::status::Status::read_at(&runtime.config, 20000)?;
        assert_eq!(serde_json::to_string(&repeated)?, json);
        assert!(!repeated.recovery_required);
        let marker = dir.path().join("CASHU_RECOVERY_REQUIRED");
        std::os::unix::fs::symlink("absent-recovery-target", &marker)?;
        let fenced = crate::status::Status::read_at(&runtime.config, 20000)?;
        assert!(fenced.recovery_required);
        assert!(std::fs::symlink_metadata(&marker)?.file_type().is_symlink());
        assert!(!dir.path().join("absent-recovery-target").exists());
        assert_eq!(runtime.journal.lock().unwrap().state(ids[0])?, "validated");
        let mut fenced_json = serde_json::to_value(&fenced)?;
        fenced_json["recovery_required"] = serde_json::json!(false);
        assert_eq!(fenced_json, serde_json::to_value(&repeated)?);

        std::fs::rename(
            dir.path().join("seed-hidden"),
            dir.path().join("wallet-seed.bin"),
        )?;
        Ok(())
    }

    #[tokio::test]
    async fn private_status_reports_unconfigured_partial_namespace_and_startup_stays_fenced()
    -> Result<()> {
        let directory = crate::test_directory()?;
        keys(directory.path())?;
        let instance = Uuid::new_v4();
        let epoch = Uuid::new_v4();
        let config = configuration(directory.path(), instance, epoch)?;
        let runtime =
            Runtime::initialize(configuration(directory.path(), instance, epoch)?).await?;
        let removed = crate::mint_policy::MintPolicy {
            id: "removed-mint".into(),
            advertised_url: "https://removed.example".into(),
            policy_version: "retained-v1".into(),
            receive_cashu: false,
            receive_nutzap: false,
            bolt11_exit_qualified: true,
        };
        assert!(
            runtime
                .journal
                .lock()
                .unwrap()
                .prepare_cdk_namespace(&removed)?
        );
        drop(runtime);
        let status = crate::status::Status::read(&config)?;
        assert_eq!(status.cdk_namespace_layout, Some(1));
        assert_eq!(status.incomplete_mint_namespaces, 1);
        assert_eq!(status.mint_namespaces.len(), 1);
        let retained = status
            .mint_namespaces
            .iter()
            .find(|m| m.mint_id == "removed-mint")
            .unwrap();
        assert!(!retained.ready && !retained.configured);
        assert!(Runtime::open(config).await.is_err());
        Ok(())
    }

    #[tokio::test]
    async fn combined_service_shutdown_releases_runtime_and_preserves_restart() -> Result<()> {
        let dir = crate::test_directory()?;
        let ipc = crate::test_directory()?;
        std::fs::set_permissions(ipc.path(), std::fs::Permissions::from_mode(0o710))?;
        keys(dir.path())?;
        let instance = Uuid::new_v4();
        let epoch = Uuid::new_v4();
        let mut config = configuration(dir.path(), instance, epoch)?;
        config.ipc_gid = rustix::process::getegid().as_raw();
        config.intake_socket = ipc.path().join("intake.sock");
        let socket = config.intake_socket.clone();
        let runtime = Runtime::initialize(config).await?;
        let (send, recv) = tokio::sync::oneshot::channel();
        let (served, client) = tokio::join!(
            runtime.serve(async {
                let _ = recv.await;
            }),
            async move {
                tokio::time::timeout(Duration::from_secs(5), async {
                    while !socket.exists() {
                        tokio::time::sleep(Duration::from_millis(5)).await;
                    }
                    let mut stream = tokio::net::UnixStream::connect(&socket).await?;
                    use tokio::io::AsyncReadExt;
                    let mut reply = [0; 1];
                    ensure!(
                        stream.read(&mut reply).await? == 0,
                        "unexpected same-UID reply"
                    );
                    Ok::<_, anyhow::Error>(())
                })
                .await??;
                let _ = send.send(());
                Ok::<_, anyhow::Error>(())
            }
        );
        client?;
        served?;
        assert!(
            runtime
                .journal
                .lock()
                .unwrap()
                .pending_operations(1)?
                .is_empty()
        );
        drop(runtime);
        drop(Runtime::open(configuration(dir.path(), instance, epoch)?).await?);
        Ok(())
    }

    #[tokio::test]
    async fn recovery_fence_blocks_initialization_and_restart_before_database_open() -> Result<()> {
        let dir = crate::test_directory()?;
        keys(dir.path())?;
        let instance = Uuid::new_v4();
        let epoch = Uuid::new_v4();
        let marker = dir.path().join("CASHU_RECOVERY_REQUIRED");
        std::fs::write(&marker, b"fixture restore requires reconciliation")?;
        let error = Runtime::initialize(configuration(dir.path(), instance, epoch)?)
            .await
            .err()
            .ok_or_else(|| anyhow::anyhow!("fenced initialization accepted"))?;
        assert!(
            error
                .to_string()
                .contains("recovery requires explicit reconciliation")
        );
        assert!(!dir.path().join("wallet.sqlite").exists());
        assert!(!dir.path().join("acquisition.sqlite").exists());
        assert!(!dir.path().join("wallet-writer.lock").exists());
        // Even incomplete restored images must stay byte-identical behind the fence.
        for name in ["wallet.sqlite", "acquisition.sqlite"] {
            std::fs::write(dir.path().join(name), b"retained incomplete restore")?;
        }
        let error = Runtime::open(configuration(dir.path(), instance, epoch)?)
            .await
            .err()
            .ok_or_else(|| anyhow::anyhow!("fenced restart accepted"))?;
        assert!(
            error
                .to_string()
                .contains("recovery requires explicit reconciliation")
        );
        for name in ["wallet.sqlite", "acquisition.sqlite"] {
            assert_eq!(
                std::fs::read(dir.path().join(name))?,
                b"retained incomplete restore"
            );
        }
        Ok(())
    }

    #[tokio::test]
    async fn scheduler_preserves_fenced_and_unconfigured_mint_attempts_across_restart() -> Result<()>
    {
        use crate::{
            journal::{Admission, HoldReason, Transport, ValidatedIntake},
            mint_policy::MintPolicy,
            scheduler::{Tick, Worker},
        };
        use cdk::{nuts::Proof, secret::Secret};
        let directory = crate::test_directory()?;
        keys(directory.path())?;
        let instance = Uuid::new_v4();
        let epoch = Uuid::new_v4();
        let make_config = || -> Result<Config> {
            let mut config = configuration(directory.path(), instance, epoch)?;
            config.execution.acquire = true;
            config.execution.reconcile = true;
            config.execution.maximum_attempts = 1;
            config.mints = ["mint-a", "mint-b"]
                .into_iter()
                .map(|id| MintPolicy {
                    id: id.into(),
                    advertised_url: format!("https://{id}.example"),
                    policy_version: "v1".into(),
                    receive_cashu: false,
                    receive_nutzap: false,
                    bolt11_exit_qualified: true,
                })
                .collect();
            config.custody_limits = Some(serde_json::from_value(serde_json::json!({
                "maximum_total_sats": 100, "maximum_per_mint_sats": {"mint-a": 40, "mint-b": 40},
            }))?);
            Ok(config)
        };
        let runtime = Runtime::initialize(make_config()?).await?;
        let (unconfigured, original, waiting, independent) = {
            let mut journal = retained_fixture_journal(&runtime)?;
            let mut retain = |mint: &str, name: &str| -> Result<Uuid> {
                let proof = Proof::new(
                    8.into(),
                    "00deadbeef123456".parse()?,
                    Secret::new(format!("synthetic-{name}")),
                    runtime.policy.receiver,
                );
                match journal.admit(&ValidatedIntake {
                    mint_id: mint,
                    policy_version: "v1",
                    transport: Transport::Cashu,
                    source_ref: name,
                    source_author: None,
                    proofs: &[proof],
                })? {
                    Admission::New(id) => Ok(id),
                    _ => anyhow::bail!("expected new synthetic work"),
                }
            };
            let ids = (
                retain("removed-mint", "unconfigured")?,
                retain("mint-a", "original")?,
                retain("mint-a", "waiting")?,
                retain("mint-b", "independent")?,
            );
            journal.prepare(ids.1, b"original retained before submission")?;
            assert!(
                journal
                    .claim_acquisition_work(90, false, true, 1, 5)?
                    .is_some_and(|work| work.id == ids.1)
            );
            journal.prepare(ids.3, b"other mint original with no retained outputs")?;
            journal.start_acquisition(ids.3)?;
            ids
        };
        let db = rusqlite::Connection::open(directory.path().join("acquisition.sqlite"))?;
        let attempts = |id: Uuid| -> Result<u32> {
            Ok(db.query_row(
                "SELECT COALESCE((SELECT attempts FROM acquisition_schedule WHERE acquisition_id=?),0)",
                [id.to_string()], |row| row.get(0),
            )?)
        };
        let mut worker = Worker::new(&runtime)?;
        assert_eq!(worker.tick(100).await?, Tick::RetryRetained);
        for now in [200, 300] {
            assert_eq!(worker.tick(now).await?, Tick::Idle);
            assert_eq!(attempts(waiting)?, 0);
            assert_eq!(attempts(independent)?, 1);
            assert_eq!(attempts(unconfigured)?, 0);
        }
        drop(worker);
        drop(runtime);
        let runtime = Runtime::open(make_config()?).await?;
        let mut worker = Worker::new(&runtime)?;
        assert_eq!(worker.tick(400).await?, Tick::Idle);
        assert_eq!(attempts(waiting)?, 0);
        let mut journal = runtime.journal.lock().unwrap();
        journal.hold(original, HoldReason::AmbiguousAcquisition)?;
        assert_eq!(
            journal.ready_acquisition_work(401, true, true, 1, Some(&["mint-a", "mint-b"]))?[0].id,
            waiting
        );
        assert_eq!(attempts(waiting)?, 0);
        assert_eq!(attempts(unconfigured)?, 0);
        Ok(())
    }

    #[tokio::test]
    async fn startup_requires_explicit_initialization_and_preserves_registration_on_restart()
    -> Result<()> {
        let dir = crate::test_directory()?;
        keys(dir.path())?;
        let instance = Uuid::new_v4();
        let epoch = Uuid::new_v4();
        assert!(
            Runtime::open(configuration(dir.path(), instance, epoch)?)
                .await
                .is_err()
        );
        assert!(!dir.path().join("wallet.sqlite").exists());
        assert!(!dir.path().join("acquisition.sqlite").exists());
        let runtime = Runtime::initialize(configuration(dir.path(), instance, epoch)?).await?;
        {
            let mut worker = crate::scheduler::Worker::new(&runtime)?;
            assert_eq!(worker.tick(100).await?, crate::scheduler::Tick::Idle);
            let delivery = crate::receipt_outbox::Worker::new(&runtime)?;
            assert_eq!(delivery.tick(100).await?, crate::receipt_outbox::Tick::Idle);
        }
        assert!(
            runtime
                .journal
                .lock()
                .unwrap()
                .pending_operations(1)?
                .is_empty()
        );
        assert!(
            Runtime::open(configuration(dir.path(), instance, epoch)?)
                .await
                .is_err()
        );
        drop(runtime);
        assert!(
            Runtime::initialize(configuration(dir.path(), instance, epoch)?)
                .await
                .is_err()
        );
        assert!(
            Runtime::open(configuration(dir.path(), Uuid::new_v4(), epoch)?)
                .await
                .is_err()
        );
        assert!(
            Runtime::open(configuration(dir.path(), instance, Uuid::new_v4())?)
                .await
                .is_err()
        );
        std::fs::write(dir.path().join("wallet-seed.bin"), [9; 64])?;
        assert!(
            Runtime::open(configuration(dir.path(), instance, epoch)?)
                .await
                .is_err()
        );
        std::fs::write(dir.path().join("wallet-seed.bin"), [7; 64])?;
        drop(Runtime::open(configuration(dir.path(), instance, epoch)?).await?);
        std::fs::rename(
            dir.path().join("wallet.sqlite"),
            dir.path().join("saved.sqlite"),
        )?;
        assert!(
            Runtime::open(configuration(dir.path(), instance, epoch)?)
                .await
                .is_err()
        );
        assert!(
            Runtime::initialize(configuration(dir.path(), instance, epoch)?)
                .await
                .is_err()
        );
        assert!(!dir.path().join("wallet.sqlite").exists());
        Ok(())
    }

    #[tokio::test]
    async fn orphan_sidecars_and_unregistered_existing_journals_are_not_fresh_wallets() -> Result<()>
    {
        let dir = crate::test_directory()?;
        keys(dir.path())?;
        let instance = Uuid::new_v4();
        let epoch = Uuid::new_v4();
        let sidecar = dir.path().join("wallet.sqlite-wal");
        std::os::unix::fs::symlink(dir.path().join("missing"), &sidecar)?;
        assert!(
            Runtime::initialize(configuration(dir.path(), instance, epoch)?)
                .await
                .is_err()
        );
        assert!(!dir.path().join("acquisition.sqlite").exists());
        std::fs::remove_file(sidecar)?;
        drop(Journal::open(
            &dir.path().join("acquisition.sqlite"),
            epoch,
            64,
        )?);
        drop(crate::wallet::open_wallet_storage(dir.path()).await?);
        assert!(
            Runtime::open(configuration(dir.path(), instance, epoch)?)
                .await
                .is_err()
        );
        assert!(
            Runtime::initialize(configuration(dir.path(), instance, epoch)?)
                .await
                .is_err()
        );
        Ok(())
    }
}
