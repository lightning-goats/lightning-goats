//! Bounded core-to-wallet forwarding. Wallet admission never grants core credit.
use anyhow::{Result, ensure};
use lightning_goats_cashu_receipt::ingress::{MAX_FRAME_BYTES, Payload, Reply, Request};
use std::{path::PathBuf, time::Duration};
use tokio::{net::UnixStream, sync::Semaphore};
use uuid::Uuid;

/// Protected core configuration; public requests cannot choose these fields.
pub struct Policy {
    pub socket: PathBuf,
    pub wallet_uid: u32,
    pub wallet_instance: Uuid,
    pub wallet_epoch: Uuid,
    pub receive_cashu: bool,
    pub receive_nutzap: bool,
    pub body_bytes: usize,
    pub concurrent_requests: usize,
    pub deadline: Duration,
}

pub struct Forwarder {
    policy: Policy,
    slots: Semaphore,
}
impl Forwarder {
    pub fn ordinary_receiving_enabled(&self) -> bool {
        self.policy.receive_cashu
    }
    /// A read-only query shares the existing bounded IPC connection budget.
    pub async fn public_request(
        &self,
    ) -> Result<Option<lightning_goats_cashu_receipt::public_request::Descriptor>> {
        if !self.ordinary_receiving_enabled() {
            return Ok(None);
        }
        let _slot = self
            .slots
            .try_acquire()
            .map_err(|_| anyhow::anyhow!("wallet intake busy"))?;
        super::public_request::fetch(&self.policy).await
    }
    pub async fn submit_with_status(
        &self,
        delivery_id: Uuid,
        body: String,
    ) -> Result<lightning_goats_cashu_receipt::receive_status::AdmissionReply> {
        self.submit_admission(delivery_id, body, None).await
    }
    pub(super) async fn submit_admission(
        &self,
        delivery_id: Uuid,
        body: String,
        checkout: Option<lightning_goats_cashu_receipt::receive_status::FeedingCheckout>,
    ) -> Result<lightning_goats_cashu_receipt::receive_status::AdmissionReply> {
        use lightning_goats_cashu_receipt::receive_status::{
            AdmissionQuery, AdmissionReply, AdmitOperation,
        };
        ensure!(
            self.policy.receive_cashu
                && !delivery_id.is_nil()
                && !body.is_empty()
                && body.len() <= self.policy.body_bytes,
            "invalid ordinary admission bound or policy"
        );
        let query = AdmissionQuery {
            version: 1,
            operation: checkout.map_or(
                AdmitOperation::OrdinaryAdmit,
                AdmitOperation::FeedingCheckout,
            ),
            wallet_instance: self.policy.wallet_instance,
            wallet_epoch: self.policy.wallet_epoch,
            delivery_id,
            body,
        };
        let reply: AdmissionReply = self.receipt_status_exchange(&query).await?;
        reply.validate(&query)?;
        Ok(reply)
    }
    pub async fn receipt_status(
        &self,
        capability: lightning_goats_cashu_receipt::receive_status::Capability,
    ) -> Result<Option<lightning_goats_cashu_receipt::receive_status::Status>> {
        use lightning_goats_cashu_receipt::receive_status::{Query, QueryOperation, Reply};
        capability.validate()?;
        let query = Query {
            version: 1,
            operation: QueryOperation::ReceiveStatus,
            wallet_instance: self.policy.wallet_instance,
            wallet_epoch: self.policy.wallet_epoch,
            request_id: Uuid::new_v4(),
            capability,
        };
        let reply: Reply = self.receipt_status_exchange(&query).await?;
        reply.validate(&query)?;
        Ok(reply.status)
    }
    async fn receipt_status_exchange<Q: serde::Serialize, R: serde::de::DeserializeOwned>(
        &self,
        query: &Q,
    ) -> Result<R> {
        ensure!(
            serde_json::to_vec(query)?.len() <= MAX_FRAME_BYTES,
            "wallet receipt frame exceeds bound"
        );
        let _slot = self
            .slots
            .try_acquire()
            .map_err(|_| anyhow::anyhow!("wallet intake busy"))?;
        let uid = crate::private_ipc::uid()?;
        ensure!(
            uid > 0 && uid != self.policy.wallet_uid,
            "wallet requires a separate runtime identity"
        );
        // Status is entirely local. Admission only validates and journals; no
        // mint request is awaited on this public request's connection budget.
        tokio::time::timeout(self.policy.deadline.min(Duration::from_secs(2)), async {
            let mut stream = UnixStream::connect(&self.policy.socket).await?;
            ensure!(
                stream.peer_cred()?.uid() == self.policy.wallet_uid,
                "wallet peer changed"
            );
            crate::private_ipc::write(&mut stream, query, MAX_FRAME_BYTES).await?;
            crate::private_ipc::read(
                &mut stream,
                lightning_goats_cashu_receipt::receive_status::MAX_STATUS_BYTES,
            )
            .await
        })
        .await
        .map_err(|_| anyhow::anyhow!("wallet admission/status deadline exceeded"))?
        .map_err(|_| anyhow::anyhow!("wallet admission/status unavailable or invalid"))
    }
    pub fn body_limit(&self) -> usize {
        self.policy.body_bytes
    }

    /// Construction is offline. Callers must share one instance across ingress.
    pub fn new(policy: Policy) -> Result<Self> {
        crate::private_ipc::socket_path(&policy.socket)?;
        ensure!(
            policy.wallet_uid > 0
                && !policy.wallet_instance.is_nil()
                && !policy.wallet_epoch.is_nil(),
            "wallet registration is not configured"
        );
        ensure!(
            policy.body_bytes > 0
                && policy.body_bytes <= MAX_FRAME_BYTES
                && policy.concurrent_requests > 0
                && policy.concurrent_requests <= 64
                && !policy.deadline.is_zero()
                && policy.deadline <= Duration::from_secs(60),
            "invalid wallet forwarding limits"
        );
        let slots = Semaphore::new(policy.concurrent_requests);
        Ok(Self { policy, slots })
    }

    /// Reuse delivery IDs on transport retries. Proof dedupe remains authoritative
    /// even if an HTTP caller or relay supplies the same proofs in a new wrapper.
    pub async fn submit(&self, delivery_id: Uuid, payload: Payload) -> Result<Reply> {
        let body = match &payload {
            Payload::Cashu(body) => {
                ensure!(
                    self.policy.receive_cashu,
                    "ordinary Cashu receiving disabled"
                );
                body
            }
            Payload::Nutzap(body) => {
                ensure!(self.policy.receive_nutzap, "nutzap receiving disabled");
                body
            }
        };
        ensure!(
            !delivery_id.is_nil() && !body.is_empty() && body.len() <= self.policy.body_bytes,
            "invalid wallet delivery bound"
        );
        let _slot = self
            .slots
            .try_acquire()
            .map_err(|_| anyhow::anyhow!("wallet intake busy"))?;
        let runtime_uid = crate::private_ipc::uid()?;
        ensure!(
            runtime_uid > 0 && runtime_uid != self.policy.wallet_uid,
            "wallet requires a separate runtime identity"
        );
        let request = Request {
            version: 1,
            wallet_instance: self.policy.wallet_instance,
            wallet_epoch: self.policy.wallet_epoch,
            delivery_id,
            payload,
        };
        // Body length is bounded before serialization; JSON escaping is checked
        // before connecting so a valid raw size cannot exceed the wire budget.
        ensure!(
            serde_json::to_vec(&request)?.len() <= MAX_FRAME_BYTES,
            "wallet delivery exceeds frame bound"
        );
        let reply = tokio::time::timeout(self.policy.deadline, async {
            let mut stream = UnixStream::connect(&self.policy.socket).await?;
            ensure!(
                stream.peer_cred()?.uid() == self.policy.wallet_uid,
                "wallet socket peer identity changed"
            );
            crate::private_ipc::write(&mut stream, &request, MAX_FRAME_BYTES).await?;
            let reply: Reply = crate::private_ipc::read(&mut stream, 1024).await?;
            reply.validate_delivery(delivery_id)?;
            Ok::<_, anyhow::Error>(reply)
        })
        .await
        .map_err(|_| anyhow::anyhow!("wallet intake deadline exceeded"))?
        .map_err(|_| anyhow::anyhow!("wallet intake unavailable or invalid reply"))?;
        Ok(reply)
    }
}

/// Optional configuration; omitting this section leaves both receiving paths off.
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub socket: PathBuf,
    pub wallet_uid: u32,
    pub wallet_instance: Uuid,
    pub wallet_epoch: Uuid,
    #[serde(default)]
    pub receive_cashu: bool,
    #[serde(default)]
    pub receive_nutzap: bool,
    pub body_bytes: usize,
    pub concurrent_requests: usize,
    pub deadline_seconds: u64,
}
impl Config {
    pub fn forwarder(&self) -> Result<Forwarder> {
        Forwarder::new(Policy {
            socket: self.socket.clone(),
            wallet_uid: self.wallet_uid,
            wallet_instance: self.wallet_instance,
            wallet_epoch: self.wallet_epoch,
            receive_cashu: self.receive_cashu,
            receive_nutzap: self.receive_nutzap,
            body_bytes: self.body_bytes,
            concurrent_requests: self.concurrent_requests,
            deadline: Duration::from_secs(self.deadline_seconds),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn policy() -> Policy {
        Policy {
            socket: "/run/lightning-goats-cashu/intake.sock".into(),
            wallet_uid: 12345,
            wallet_instance: Uuid::new_v4(),
            wallet_epoch: Uuid::new_v4(),
            receive_cashu: false,
            receive_nutzap: false,
            body_bytes: 64,
            concurrent_requests: 1,
            deadline: Duration::from_secs(1),
        }
    }
    #[tokio::test]
    async fn unexpected_wallet_uid_receives_no_payload() -> Result<()> {
        use tokio::{io::AsyncReadExt, net::UnixListener};
        let directory = tempfile::tempdir()?;
        let socket_path = directory.path().join("wallet.sock");
        let listener = UnixListener::bind(&socket_path)?;
        let mut configured = policy();
        configured.socket = socket_path;
        configured.wallet_uid = crate::private_ipc::uid()?.checked_add(1).unwrap();
        configured.receive_cashu = true;
        let forwarder = Forwarder::new(configured)?;
        let server = async {
            tokio::time::timeout(Duration::from_secs(2), async {
                let (mut stream, _) = listener.accept().await?;
                let mut byte = [0];
                ensure!(
                    stream.read(&mut byte).await? == 0,
                    "unauthenticated peer received payload"
                );
                Ok::<_, anyhow::Error>(())
            })
            .await?
        };
        let client = forwarder.submit(Uuid::new_v4(), Payload::Cashu("private-fixture".into()));
        let (observed, response) = tokio::join!(server, client);
        observed?;
        assert!(response.is_err());
        Ok(())
    }

    #[tokio::test]
    async fn disabled_oversized_and_busy_requests_fail_before_socket_access() -> Result<()> {
        let forwarder = Forwarder::new(policy())?;
        assert_eq!(
            forwarder
                .submit(Uuid::new_v4(), Payload::Cashu("fixture".into()))
                .await
                .err()
                .unwrap()
                .to_string(),
            "ordinary Cashu receiving disabled"
        );
        assert_eq!(
            forwarder
                .submit(Uuid::new_v4(), Payload::Nutzap("fixture".into()))
                .await
                .err()
                .unwrap()
                .to_string(),
            "nutzap receiving disabled"
        );
        let mut enabled = policy();
        enabled.receive_cashu = true;
        let forwarder = Forwarder::new(enabled)?;
        assert_eq!(
            forwarder
                .submit(Uuid::new_v4(), Payload::Cashu("x".repeat(65)))
                .await
                .err()
                .unwrap()
                .to_string(),
            "invalid wallet delivery bound"
        );
        let _busy = forwarder.slots.try_acquire()?;
        assert_eq!(
            forwarder
                .submit(Uuid::new_v4(), Payload::Cashu("fixture".into()))
                .await
                .err()
                .unwrap()
                .to_string(),
            "wallet intake busy"
        );
        Ok(())
    }
}
