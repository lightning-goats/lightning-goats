//! One bounded, authenticated private receiving connection. No mint calls here.
use crate::{
    descriptor::RetiredDescriptor,
    intake::{
        DescriptorAcceptance, Limits, NutzapReceivers, PinnedKeys, ValidatedContribution,
        nutzap_json_at, ordinary_json_at,
    },
    journal::{Admission, Journal, custody::AdmissionCapacity},
    mint_policy::MintRegistry,
    writer_lock::WriterLock,
};
use anyhow::{Result, ensure};
use cdk::nuts::PublicKey;
use lightning_goats_cashu_receipt::ingress::{
    Disposition, MAX_FRAME_BYTES, Payload, Reply, Request,
};
use lightning_goats_cashu_receipt::receive_status::http_delivery_id;
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::UnixStream,
};
use uuid::Uuid;

/// Values come from protected service configuration, never public input.
/// The service's bounded accept loop must serialize calls and retain WriterLock.
pub struct Intake<'a> {
    pub core_uid: u32,
    pub wallet_instance: Uuid,
    pub wallet_epoch: Uuid,
    pub receive_cashu: bool,
    pub receive_nutzap: bool,
    pub registry: &'a MintRegistry,
    pub keys: &'a PinnedKeys,
    pub limits: &'a Limits,
    pub receiver: PublicKey,
    pub recipient: &'a str,
    pub descriptors: &'a [String],
    pub retired_descriptors: &'a [RetiredDescriptor],
    pub retained_receivers: &'a [crate::service_config::RetainedReceiver],
    pub public_request: Option<&'a crate::descriptor::Descriptor>,
    pub journal: Arc<Mutex<Journal>>,
    pub writer: &'a WriterLock,
}
impl Intake<'_> {
    fn ordinary(&self, body: &[u8], source_ref: &str) -> Result<ValidatedContribution> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs();
        ordinary_json_at(
            body,
            source_ref,
            DescriptorAcceptance {
                active: self.descriptors,
                retired: self.retired_descriptors,
                now,
            },
            self.registry,
            self.keys,
            self.limits,
            self.receiver,
        )
    }

    /// Bind only through the repository's protected runtime-directory checks.
    /// Process one connection at a time; rejected peers/payloads do not stop intake.
    pub async fn serve(
        &self,
        path: &std::path::Path,
        group: u32,
        deadline: Duration,
        maximum_connections_per_second: u32,
        shutdown: impl std::future::Future<Output = ()>,
    ) -> Result<()> {
        ensure!(
            maximum_connections_per_second > 0 && maximum_connections_per_second <= 64,
            "invalid private intake connection budget"
        );
        ensure!(
            !deadline.is_zero() && deadline <= Duration::from_secs(60),
            "invalid private intake deadline"
        );
        let runtime_uid = rustix::process::geteuid().as_raw();
        ensure!(
            runtime_uid > 0
                && self.core_uid > 0
                && runtime_uid != self.core_uid
                && !self.wallet_instance.is_nil()
                && !self.wallet_epoch.is_nil(),
            "private intake registration is not configured"
        );
        self.journal
            .lock()
            .map_err(|_| anyhow::anyhow!("wallet journal unavailable"))?
            .require_epoch(self.wallet_epoch)?;
        let listener = crate::private_ipc::bind(path, group).await?;
        let cooldown = Duration::from_secs(1) / maximum_connections_per_second;
        tokio::pin!(shutdown);
        loop {
            let (mut stream, _) = tokio::select! {
                _ = &mut shutdown => return Ok(()),
                accepted = listener.accept() => accepted?,
            };
            tokio::select! {
                _ = &mut shutdown => return Ok(()),
                _ = self.serve_connection(&mut stream, deadline) => {},
            }
            drop(stream);
            tokio::select! {
                _ = &mut shutdown => return Ok(()),
                _ = tokio::time::sleep(cooldown) => {},
            }
        }
    }

    /// Authenticate kernel credentials before reading even the frame length.
    /// One frame/reply per connection; errors deliberately contain no payload.
    pub async fn serve_connection(
        &self,
        stream: &mut UnixStream,
        deadline: Duration,
    ) -> Result<()> {
        let runtime_uid = rustix::process::geteuid().as_raw();
        ensure!(
            self.core_uid > 0
                && runtime_uid > 0
                && self.core_uid != runtime_uid
                && stream.peer_cred()?.uid() == self.core_uid,
            "private intake peer not authorized"
        );
        ensure!(
            !deadline.is_zero() && deadline <= Duration::from_secs(60),
            "invalid intake deadline"
        );
        tokio::time::timeout(deadline, async {
            let length = usize::try_from(stream.read_u32().await?)?;
            ensure!(
                length > 0 && length <= MAX_FRAME_BYTES,
                "private intake frame exceeds bound"
            );
            let mut bytes = vec![0; length];
            stream.read_exact(&mut bytes).await?;
            let encoded = self
                .reply_frame(&bytes)
                .map_err(|_| anyhow::anyhow!("private intake rejected"))?;
            ensure!(
                encoded.len() <= lightning_goats_cashu_receipt::public_request::MAX_REPLY_BYTES,
                "private intake reply exceeds bound"
            );
            stream.write_u32(u32::try_from(encoded.len())?).await?;
            stream.write_all(&encoded).await?;
            stream.flush().await?;
            Ok(())
        })
        .await
        .map_err(|_| anyhow::anyhow!("private intake deadline exceeded"))?
    }

    pub(crate) fn reply_frame(&self, bytes: &[u8]) -> Result<Vec<u8>> {
        use lightning_goats_cashu_receipt::receive_status as status;
        if let Ok(query) = status::Query::decode(bytes, self.wallet_instance, self.wallet_epoch) {
            let journal = self
                .journal
                .lock()
                .map_err(|_| anyhow::anyhow!("wallet journal unavailable"))?;
            journal.require_epoch(self.wallet_epoch)?;
            return Ok(serde_json::to_vec(&status::Reply {
                version: 1,
                wallet_instance: self.wallet_instance,
                wallet_epoch: self.wallet_epoch,
                request_id: query.request_id,
                status: journal.receipt_status(&query.capability)?,
            })?);
        }
        if let Ok(query) =
            status::AdmissionQuery::decode(bytes, self.wallet_instance, self.wallet_epoch)
        {
            ensure!(self.receive_cashu, "ordinary Cashu receiving disabled");
            let body = query.body.as_bytes();
            let is_content_id = query.delivery_id.get_version_num() == 8;
            ensure!(
                !is_content_id || query.delivery_id == http_delivery_id(body),
                "invalid content-bound delivery ID"
            );
            let source_ref = format!("http:{}", query.delivery_id);
            let mut journal = self
                .journal
                .lock()
                .map_err(|_| anyhow::anyhow!("wallet journal unavailable"))?;
            journal.require_epoch(self.wallet_epoch)?;
            let existing = if is_content_id && body.len() <= self.limits.body_bytes {
                journal.existing_http_delivery(&source_ref)?
            } else {
                None
            };
            // An exact, previously admitted HTTP POST can recover its status
            // after a descriptor expires. Only new admissions use today's policy.
            let id = match existing {
                Some(id) => Some(id),
                None => match self.ordinary(body, &source_ref) {
                    Err(_) => None,
                    Ok(contribution) => match journal.admit(&contribution.intake()) {
                        Ok(Admission::New(id) | Admission::Duplicate(id)) => Some(id),
                        Err(error)
                            if error.downcast_ref::<AdmissionCapacity>()
                                == Some(&AdmissionCapacity::TooLarge) =>
                        {
                            None
                        }
                        // Saturation is retryable and uses the existing HTTP 503
                        // path. There is no admission or capability to return.
                        Err(error) => return Err(error),
                    },
                },
            };
            let result = match id {
                None => status::AdmissionOutcome::Rejected,
                Some(id) => {
                    let capability = journal.receipt_capability(id)?;
                    let state = journal
                        .receipt_status(&capability)?
                        .ok_or_else(|| anyhow::anyhow!("admitted receipt status missing"))?;
                    status::AdmissionOutcome::Admitted {
                        capability,
                        status: state,
                    }
                }
            };
            return Ok(serde_json::to_vec(&status::AdmissionReply {
                version: 1,
                wallet_instance: self.wallet_instance,
                wallet_epoch: self.wallet_epoch,
                delivery_id: query.delivery_id,
                result,
            })?);
        }
        use lightning_goats_cashu_receipt::public_request::{Query, Reply};
        if let Ok(query) = Query::decode(bytes, self.wallet_instance, self.wallet_epoch) {
            self.journal
                .lock()
                .map_err(|_| anyhow::anyhow!("wallet journal unavailable"))?
                .require_epoch(self.wallet_epoch)?;
            let descriptor = if self.receive_cashu {
                self.public_request
                    .map(|request| {
                        ensure!(
                            self.descriptors.iter().any(|id| id == request.id()),
                            "public descriptor no longer registered"
                        );
                        request.public_metadata()
                    })
                    .transpose()?
            } else {
                None
            };
            return Ok(serde_json::to_vec(&Reply {
                version: 1,
                wallet_instance: self.wallet_instance,
                wallet_epoch: self.wallet_epoch,
                request_id: query.request_id,
                descriptor,
            })?);
        }
        Ok(serde_json::to_vec(&self.admit_frame(bytes)?)?)
    }

    pub(crate) fn admit_frame(&self, bytes: &[u8]) -> Result<Reply> {
        let request = Request::decode(bytes, self.wallet_instance, self.wallet_epoch)?;
        let contribution = match request.payload {
            Payload::Cashu(body) => {
                ensure!(self.receive_cashu, "ordinary Cashu receiving disabled");
                ensure!(
                    request.delivery_id.get_version_num() != 8
                        || request.delivery_id == http_delivery_id(body.as_bytes()),
                    "invalid content-bound delivery ID"
                );
                self.ordinary(body.as_bytes(), &format!("http:{}", request.delivery_id))?
            }
            Payload::Nutzap(body) => {
                ensure!(self.receive_nutzap, "nutzap receiving disabled");
                nutzap_json_at(
                    &body,
                    self.recipient,
                    self.registry,
                    self.keys,
                    self.limits,
                    NutzapReceivers {
                        current: self.receiver,
                        retained: self.retained_receivers,
                        now: std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)?
                            .as_secs(),
                    },
                )?
            }
        };
        let mut journal = self
            .journal
            .lock()
            .map_err(|_| anyhow::anyhow!("wallet journal unavailable"))?;
        journal.require_epoch(self.wallet_epoch)?;
        let (acquisition_id, disposition) = match journal.admit(&contribution.intake())? {
            Admission::New(id) => (id, Disposition::Admitted),
            Admission::Duplicate(id) => (id, Disposition::Existing),
        };
        Ok(Reply {
            version: 1,
            delivery_id: request.delivery_id,
            acquisition_id,
            disposition,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::{MetadataExt, PermissionsExt};

    #[tokio::test]
    async fn listener_survives_rejected_peers_and_reopens_only_a_stale_owned_socket() -> Result<()>
    {
        let state = crate::test_directory()?;
        let runtime = crate::test_directory()?;
        std::fs::set_permissions(runtime.path(), std::fs::Permissions::from_mode(0o710))?;
        let path = runtime.path().join("intake.sock");
        let gid = std::fs::metadata(runtime.path())?.gid();
        let writer = WriterLock::acquire(state.path())?;
        let epoch = Uuid::new_v4();
        let journal = Arc::new(Mutex::new(Journal::open(
            &state.path().join("journal.sqlite"),
            epoch,
            8,
        )?));
        let registry = MintRegistry::new(vec![])?;
        let keys = PinnedKeys::default();
        let intake = Intake {
            core_uid: rustix::process::geteuid().as_raw().checked_add(1).unwrap(),
            wallet_instance: Uuid::new_v4(),
            wallet_epoch: epoch,
            receive_cashu: false,
            receive_nutzap: false,
            registry: &registry,
            keys: &keys,
            limits: &Limits {
                body_bytes: 1024,
                proof_count: 8,
                receipt_sats: 1000,
            },
            receiver: cdk::nuts::SecretKey::from_slice(&[1; 32])?.public_key(),
            recipient: "",
            descriptors: &[],
            retired_descriptors: &[],
            retained_receivers: &[],
            public_request: None,
            journal,
            writer: &writer,
        };
        let (stop, stopped) = tokio::sync::oneshot::channel();
        let service = intake.serve(&path, gid, Duration::from_secs(1), 64, async {
            let _ = stopped.await;
        });
        let client = async {
            tokio::time::timeout(Duration::from_secs(5), async {
                while !path.exists() {
                    tokio::time::sleep(Duration::from_millis(5)).await;
                }
                assert_eq!(
                    std::fs::metadata(&path)?.permissions().mode() & 0o777,
                    0o660
                );
                assert!(crate::private_ipc::bind(&path, gid).await.is_err());
                for _ in 0..2 {
                    let mut socket = UnixStream::connect(&path).await?;
                    let mut byte = [0];
                    assert_eq!(socket.read(&mut byte).await?, 0);
                }
                Ok::<_, anyhow::Error>(())
            })
            .await??;
            let _ = stop.send(());
            Ok::<_, anyhow::Error>(())
        };
        let (served, checked) = tokio::join!(service, client);
        checked?;
        served?;
        // Shutdown leaves a known socket; only the canonical stale-owner check may replace it.
        let reopened = crate::private_ipc::bind(&path, gid).await?;
        drop(reopened);
        std::fs::remove_file(&path)?;
        std::fs::write(&path, b"preserve unexpected file")?;
        assert!(crate::private_ipc::bind(&path, gid).await.is_err());
        assert_eq!(std::fs::read(path)?, b"preserve unexpected file");
        Ok(())
    }
}

#[cfg(test)]
mod public_request_fixture;
