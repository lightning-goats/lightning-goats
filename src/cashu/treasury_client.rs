//! Core transport for retained treasury instructions. This never creates invoices
//! or credits a deposit; the existing ledger/provider verification remains authoritative.
use super::{
    receipt_authority::{KernelPeer, ReceiptPolicy},
    treasury::PeerMelt,
};
use anyhow::{Result, ensure};
use lightning_goats_cashu_receipt::{
    treasury::MAX_TREASURY_BYTES,
    treasury_rpc::{Reply, Request},
};
use std::{path::PathBuf, sync::Arc, time::Duration};
use tokio::{net::UnixStream, sync::Semaphore};

pub struct Client {
    socket: PathBuf,
    registration: Arc<ReceiptPolicy>,
    deadline: Duration,
    slots: Semaphore,
}
pub struct Response {
    pub reply: Reply,
    /// Present only after authentication of the socket's actual wallet peer.
    pub report: Option<PeerMelt>,
}
impl Client {
    pub fn new(
        socket: PathBuf,
        registration: Arc<ReceiptPolicy>,
        deadline: Duration,
        concurrency: usize,
    ) -> Result<Self> {
        crate::private_ipc::socket_path(&socket)?;
        ensure!(
            registration.wallet_uid > 0
                && !registration.wallet_instance.is_nil()
                && !registration.wallet_epoch.is_nil()
                && !registration.core_epoch.is_nil()
                && !deadline.is_zero()
                && deadline <= Duration::from_secs(60)
                && (1..=16).contains(&concurrency),
            "invalid treasury client registration/limits"
        );
        Ok(Self {
            socket,
            registration,
            deadline,
            slots: Semaphore::new(concurrency),
        })
    }
    /// Caller supplies only an instruction already frozen by the core ledger.
    /// A lost acknowledgment is retried with that same instruction and intent ID.
    pub async fn send(&self, request: &Request) -> Result<Response> {
        request.validate()?;
        let intent = request.intent();
        ensure!(
            intent.core_epoch == self.registration.core_epoch
                && intent.wallet_epoch == self.registration.wallet_epoch
                && intent.wallet_instance == self.registration.wallet_instance,
            "treasury request registration mismatch"
        );
        request.encode()?;
        let _slot = self
            .slots
            .try_acquire()
            .map_err(|_| anyhow::anyhow!("treasury wallet transport busy"))?;
        tokio::time::timeout(self.deadline, async {
            let mut stream = UnixStream::connect(&self.socket).await?;
            let peer = KernelPeer::from_socket(&stream)?;
            self.registration.check_peer(&peer)?;
            crate::private_ipc::write(&mut stream, request, MAX_TREASURY_BYTES).await?;
            let reply: Reply = crate::private_ipc::read(&mut stream, MAX_TREASURY_BYTES).await?;
            reply.validate_for(request)?;
            let report = reply
                .report
                .as_ref()
                .map(|data| {
                    PeerMelt::authenticate(&self.registration, &peer, &serde_json::to_vec(data)?)
                })
                .transpose()?;
            Ok::<_, anyhow::Error>(Response { reply, report })
        })
        .await
        .map_err(|_| anyhow::anyhow!("treasury wallet transport timed out"))?
        .map_err(|_| anyhow::anyhow!("treasury wallet unavailable or reply rejected"))
    }
}

#[cfg(test)]
mod cross_uid_fixture;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cashu::receipt_authority::ImportLimits;
    use lightning_goats_cashu_receipt::treasury_rpc::Command;
    use std::collections::BTreeMap;
    use tokio::io::AsyncReadExt;
    use uuid::Uuid;

    #[tokio::test]
    async fn wrong_wallet_peer_gets_no_treasury_instruction() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let socket = dir.path().join("treasury.sock");
        let listener = tokio::net::UnixListener::bind(&socket)?;
        let registration = Arc::new(ReceiptPolicy {
            wallet_uid: crate::private_ipc::uid()? + 1,
            wallet_instance: Uuid::from_u128(2),
            wallet_epoch: Uuid::from_u128(3),
            core_epoch: Uuid::from_u128(1),
            import_enabled: false,
            limits: ImportLimits::default(),
            approved: BTreeMap::new(),
        });
        let intent = serde_json::from_value(serde_json::json!({
            "version":1, "purpose":"cashu_treasury_settlement", "id":Uuid::from_u128(5),
            "core_epoch":registration.core_epoch, "wallet_instance":registration.wallet_instance,
            "wallet_epoch":registration.wallet_epoch, "mint_id":"fixture", "treasury_policy":"v1",
            "destination":{"origin":"https://api.example.com/","account_id":Uuid::from_u128(4),
                "credential_fingerprint":"ab".repeat(32),"network":"bitcoin"},
            "input_sats":8,"deposit_sats":7,"maximum_fee_sats":1,"invoice_expiry_seconds":300,
            "sources":[{"ledger_id":1,"acquisition_id":Uuid::from_u128(6),"receipt_digest":"cd".repeat(32),"input_sats":8}]
        }))?;
        let request = Request {
            version: 1,
            request_id: Uuid::from_u128(7),
            command: Command::Status(Box::new(intent)),
        };
        let client = Client::new(socket, registration, Duration::from_secs(2), 1)?;
        let server = async {
            let (mut stream, _) = listener.accept().await?;
            let mut first = [0u8; 1];
            assert_eq!(stream.read(&mut first).await?, 0);
            Ok::<_, anyhow::Error>(())
        };
        let (result, observed) = tokio::time::timeout(Duration::from_secs(3), async {
            tokio::join!(client.send(&request), server)
        })
        .await?;
        assert!(result.is_err());
        observed?;
        Ok(())
    }
}
