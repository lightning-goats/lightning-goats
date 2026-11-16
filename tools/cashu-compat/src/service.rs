//! Protected startup assembly. No mint requests, core credit or automatic initialization.
use crate::{
    ingress::Intake,
    journal::Journal,
    service_config::{Config, Prepared},
    service_keys::Keys,
    writer_lock::WriterLock,
};
use anyhow::{Result, ensure};
use sha2::{Digest, Sha256};
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
            result = async { tokio::try_join!(self.serve_intake(std::future::pending()), self.serve_treasury(std::future::pending()), worker.run(), delivery.run(), relays.run(), signing.run(), publishing.run(), treasury.run()) } => result.map(|_| ()),
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
        let mut hash = Sha256::new();
        hash.update(b"lightning-goats-cashu-runtime-keys-v1\0");
        hash.update(&keys.seed[..]);
        hash.update(keys.receiving.public_key().to_bytes());
        let binding: [u8; 32] = hash.finalize().into();
        if !initialize {
            journal.runtime_registration(config.wallet_instance, &binding, false)?;
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
        for index in 0..65 {
            let proofs = vec![Proof::new(
                8.into(),
                "00deadbeef123456".parse::<Id>()?,
                Secret::new(format!("private-proof-secret-{index}")),
                runtime.policy.receiver,
            )];
            let admitted = runtime.journal.lock().unwrap().admit(&ValidatedIntake {
                mint_id: "fixture",
                policy_version: "v1",
                transport: Transport::Cashu,
                source_ref: &format!("private-source-{index}"),
                source_author: None,
                proofs: &proofs,
            })?;
            if let crate::journal::Admission::New(id) = admitted {
                ids.push(id);
            }
        }
        runtime
            .journal
            .lock()
            .unwrap()
            .hold(ids[64], HoldReason::AmbiguousAcquisition)?;
        runtime
            .journal
            .lock()
            .unwrap()
            .claim_acquisition_work(100, true, false, 1, 5)?;
        std::fs::rename(
            dir.path().join("wallet-seed.bin"),
            dir.path().join("seed-hidden"),
        )?;
        let status = crate::status::Status::read(&runtime.config)?;
        assert_eq!(status.state_counts.get("validated"), Some(&64));
        assert_eq!(status.state_counts.get("held_for_reconciliation"), Some(&1));
        assert_eq!(status.exhausted_acquisitions, 1);
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
        let repeated = crate::status::Status::read(&runtime.config)?;
        assert_eq!(serde_json::to_string(&repeated)?, json);
        assert!(!repeated.recovery_required);
        let marker = dir.path().join("CASHU_RECOVERY_REQUIRED");
        std::os::unix::fs::symlink("absent-recovery-target", &marker)?;
        let fenced = crate::status::Status::read(&runtime.config)?;
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
