//! One bounded receipt delivery. The core is authenticated before any receipt bytes leave.
use crate::journal::Journal;
use anyhow::{Result, ensure};
use lightning_goats_cashu_receipt::{
    WireReceipt,
    acknowledgment::{Acknowledgment, MAX_ACK_BYTES},
};
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::UnixStream,
};
use uuid::Uuid;

#[cfg(test)]
mod cross_uid_fixture;

pub struct Client {
    socket: PathBuf,
    core_uid: u32,
    core_epoch: Uuid,
    deadline: Duration,
}
impl Client {
    pub fn new(
        socket: PathBuf,
        core_uid: u32,
        core_epoch: Uuid,
        deadline: Duration,
    ) -> Result<Self> {
        crate::private_ipc::socket_path(&socket)?;
        ensure!(
            core_uid > 0
                && !core_epoch.is_nil()
                && !deadline.is_zero()
                && deadline <= Duration::from_secs(60),
            "invalid core receipt endpoint"
        );
        Ok(Self {
            socket,
            core_uid,
            core_epoch,
            deadline,
        })
    }
    pub async fn send(&self, receipt: &WireReceipt) -> Result<Acknowledgment> {
        let uid = rustix::process::geteuid().as_raw();
        ensure!(
            uid > 0 && uid != self.core_uid,
            "receipt delivery requires distinct runtime identities"
        );
        let bytes = receipt.encode()?;
        tokio::time::timeout(self.deadline, async {
            let mut stream = UnixStream::connect(&self.socket).await?;
            ensure!(
                stream.peer_cred()?.uid() == self.core_uid,
                "core receipt peer not authorized"
            );
            stream.write_u32(u32::try_from(bytes.len())?).await?;
            stream.write_all(&bytes).await?;
            stream.flush().await?;
            let length = usize::try_from(stream.read_u32().await?)?;
            ensure!(
                length > 0 && length <= MAX_ACK_BYTES,
                "invalid core acknowledgment frame"
            );
            let mut bytes = vec![0; length];
            stream.read_exact(&mut bytes).await?;
            Acknowledgment::decode_for(&bytes, receipt, self.core_epoch)
        })
        .await
        .map_err(|_| anyhow::anyhow!("core receipt delivery timed out"))?
    }

    /// Lost/malformed acknowledgment leaves export pending, never invents a core grant.
    pub async fn deliver(
        &self,
        journal: &Arc<Mutex<Journal>>,
        wallet_instance: Uuid,
        id: Uuid,
    ) -> Result<()> {
        let receipt = journal
            .lock()
            .map_err(|_| anyhow::anyhow!("wallet journal unavailable"))?
            .export_receipt(id)?;
        let wire = crate::receipt_wire::from_private(wallet_instance, &receipt)?;
        let ack = self.send(&wire).await?;
        journal
            .lock()
            .map_err(|_| anyhow::anyhow!("wallet journal unavailable"))?
            .acknowledge_verified_credit(wallet_instance, self.core_epoch, id, &ack.encode()?)
    }
}

impl Journal {
    /// Call only after authenticating the configured core peer. Canonicalize before storage.
    pub(crate) fn acknowledge_verified_credit(
        &mut self,
        instance: Uuid,
        core_epoch: Uuid,
        id: Uuid,
        bytes: &[u8],
    ) -> Result<()> {
        let receipt = self.export_receipt(id)?;
        let wire = crate::receipt_wire::from_private(instance, &receipt)?;
        let ack = Acknowledgment::decode_for(bytes, &wire, core_epoch)?;
        self.acknowledge_credit(id, &ack.encode()?, ack.result.retained_cashu)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn wrong_core_uid_receives_no_receipt_bytes() -> Result<()> {
        let dir = crate::test_directory()?;
        let socket = dir.path().join("receipt.sock");
        let listener = tokio::net::UnixListener::bind(&socket)?;
        let uid = rustix::process::geteuid().as_raw();
        let client = Client::new(socket, uid + 1, Uuid::new_v4(), Duration::from_secs(2))?;
        let receipt = WireReceipt::validate(lightning_goats_cashu_receipt::ReceiptData {
            schema_version: 1,
            wallet_instance: Uuid::new_v4(),
            wallet_epoch: Uuid::new_v4(),
            acquisition_id: Uuid::new_v4(),
            transport: "cashu".into(),
            source_ref: "private-fixture".into(),
            source_author: None,
            mint_id: "fixture".into(),
            unit: "sat".into(),
            gross_sats: 8,
            receive_fee_sats: 1,
            net_sats: 7,
            acquisition_evidence_digest: "12".repeat(32),
            policy_version: "fixture-v1".into(),
            credit_pool: "herd".into(),
        })?;
        let (sent, received) = tokio::join!(client.send(&receipt), async {
            let (mut stream, _) = listener.accept().await?;
            let mut bytes = Vec::new();
            stream.read_to_end(&mut bytes).await?;
            Ok::<_, anyhow::Error>(bytes)
        });
        assert!(sent.is_err());
        assert!(received?.is_empty());
        Ok(())
    }
}
