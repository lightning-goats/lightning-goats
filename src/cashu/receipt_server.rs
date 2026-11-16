//! Private wallet receipt import. Kernel authentication precedes framing and credit.
use super::receipt_authority::{
    ImportLimits, KernelPeer, MintImportLimit, PeerReceipt, ReceiptPolicy,
};
use crate::ledger::LedgerStore;
use anyhow::{Result, ensure};
use lightning_goats_cashu_receipt::{
    MAX_RECEIPT_BYTES,
    acknowledgment::{Acknowledgment, MAX_ACK_BYTES},
};
use serde::Deserialize;
use std::{collections::BTreeMap, path::PathBuf, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::UnixStream,
};
use uuid::Uuid;

#[cfg(test)]
mod cross_uid_fixture;

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub socket: PathBuf,
    pub ipc_gid: u32,
    pub wallet_uid: u32,
    pub wallet_instance: Uuid,
    pub wallet_epoch: Uuid,
    pub core_epoch: Uuid,
    #[serde(default)]
    pub import_enabled: bool,
    #[serde(default)]
    pub receipts_per_24h: u64,
    #[serde(default)]
    pub credit_sats_per_24h: u64,
    #[serde(default)]
    pub maximum_custody_sats: u64,
    #[serde(default)]
    pub approved: Vec<Mint>,
    pub deadline_seconds: u64,
    pub connections_per_second: u32,
}
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Mint {
    pub mint_id: String,
    pub policy_version: String,
    pub maximum_gross_sats: u64,
    pub maximum_net_sats: u64,
}
impl Config {
    pub fn validate(&self) -> Result<()> {
        self.policy().map(|_| ())
    }
    pub(crate) fn policy(&self) -> Result<ReceiptPolicy> {
        crate::private_ipc::socket_path(&self.socket)?;
        ensure!(
            self.ipc_gid > 0
                && self.wallet_uid > 0
                && !self.wallet_instance.is_nil()
                && !self.wallet_epoch.is_nil()
                && !self.core_epoch.is_nil(),
            "invalid core receipt registration"
        );
        ensure!(
            (1..=60).contains(&self.deadline_seconds)
                && (1..=64).contains(&self.connections_per_second),
            "invalid core receipt connection bounds"
        );
        ensure!(
            self.approved.len() <= 128
                && [
                    self.receipts_per_24h,
                    self.credit_sats_per_24h,
                    self.maximum_custody_sats
                ]
                .iter()
                .all(|n| *n <= i64::MAX as u64),
            "invalid core receipt budgets"
        );
        let mut approved = BTreeMap::new();
        for mint in &self.approved {
            ensure!(
                [&mint.mint_id, &mint.policy_version]
                    .iter()
                    .all(|s| !s.is_empty() && s.len() <= 128 && !s.chars().any(char::is_control))
                    && mint.maximum_net_sats > 0
                    && mint.maximum_net_sats <= mint.maximum_gross_sats
                    && mint.maximum_gross_sats <= i64::MAX as u64,
                "invalid approved mint receipt limit"
            );
            ensure!(
                approved
                    .insert(
                        (mint.mint_id.clone(), mint.policy_version.clone()),
                        MintImportLimit {
                            maximum_gross_sats: mint.maximum_gross_sats,
                            maximum_net_sats: mint.maximum_net_sats
                        }
                    )
                    .is_none(),
                "duplicate approved mint policy"
            );
        }
        ensure!(
            !self.import_enabled
                || (!approved.is_empty()
                    && self.receipts_per_24h > 0
                    && self.credit_sats_per_24h > 0
                    && self.maximum_custody_sats > 0),
            "new receipt import requires explicit policy and budgets"
        );
        Ok(ReceiptPolicy {
            wallet_uid: self.wallet_uid,
            wallet_instance: self.wallet_instance,
            wallet_epoch: self.wallet_epoch,
            core_epoch: self.core_epoch,
            import_enabled: self.import_enabled,
            limits: ImportLimits {
                receipts_per_24h: self.receipts_per_24h,
                credit_sats_per_24h: self.credit_sats_per_24h,
                maximum_custody_sats: self.maximum_custody_sats,
            },
            approved,
        })
    }
}

pub struct Server {
    config: Config,
    policy: ReceiptPolicy,
    ledger: LedgerStore,
}
impl Server {
    pub fn new(config: Config, ledger: LedgerStore) -> Result<Self> {
        let policy = config.policy()?;
        let uid = crate::private_ipc::uid()?;
        ensure!(
            uid > 0 && uid != config.wallet_uid,
            "receipt server requires a separate non-root identity"
        );
        Ok(Self {
            config,
            policy,
            ledger,
        })
    }
    async fn commit(&self, receipt: &PeerReceipt) -> Result<Acknowledgment> {
        let result = self
            .ledger
            .import_cashu_receipt(&self.policy, receipt)
            .await?;
        let ack = Acknowledgment {
            version: 1,
            wallet_instance: self.policy.wallet_instance,
            wallet_epoch: self.policy.wallet_epoch,
            core_epoch: self.policy.core_epoch,
            result,
        };
        ack.validate_for(receipt.receipt(), self.policy.core_epoch)?;
        Ok(ack)
    }
    pub async fn serve_connection(&self, stream: &mut UnixStream) -> Result<()> {
        let peer = KernelPeer::from_socket(stream)?;
        self.policy.check_peer(&peer)?;
        tokio::time::timeout(Duration::from_secs(self.config.deadline_seconds), async {
            let length = usize::try_from(stream.read_u32().await?)?;
            ensure!(
                length > 0 && length <= MAX_RECEIPT_BYTES,
                "receipt frame exceeds bound"
            );
            let mut bytes = vec![0; length];
            stream.read_exact(&mut bytes).await?;
            let receipt = self.policy.authenticate(&peer, &bytes)?;
            let ack = self.commit(&receipt).await?.encode()?;
            ensure!(ack.len() <= MAX_ACK_BYTES, "acknowledgment exceeds bound");
            stream.write_u32(u32::try_from(ack.len())?).await?;
            stream.write_all(&ack).await?;
            stream.flush().await?;
            Ok::<_, anyhow::Error>(())
        })
        .await
        .map_err(|_| anyhow::anyhow!("private receipt deadline exceeded"))?
    }
    pub async fn serve(&self, shutdown: impl std::future::Future<Output = ()>) -> Result<()> {
        ensure!(
            self.ledger.overlay_stream_id().await? == self.policy.core_epoch,
            "core epoch mismatch before receipt listener startup"
        );
        let listener = crate::private_ipc::bind(&self.config.socket, self.config.ipc_gid).await?;
        tokio::pin!(shutdown);
        loop {
            let (mut stream, _) = tokio::select! { _ = &mut shutdown => return Ok(()), accepted = listener.accept() => accepted? };
            tokio::select! { _ = &mut shutdown => return Ok(()), _ = self.serve_connection(&mut stream) => {} }
            drop(stream);
            tokio::select! { _ = &mut shutdown => return Ok(()), _ = tokio::time::sleep(Duration::from_secs(1)/self.config.connections_per_second) => {} }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cashu::receipt_authority::fixture_authenticate;
    use lightning_goats_cashu_receipt::{ReceiptData, WireReceipt};

    async fn fixture() -> Result<(tempfile::TempDir, Server)> {
        let dir = tempfile::tempdir()?;
        let ledger = LedgerStore::connect(&format!(
            "sqlite://{}",
            dir.path().join("core.sqlite").display()
        ))
        .await?;
        ledger.initialize_reward_funding().await?;
        ledger.configure_feeder_credit_cap(1000).await?;
        let config = Config {
            socket: dir.path().join("receipts.sock"),
            ipc_gid: 12347,
            wallet_uid: crate::private_ipc::uid()? + 1,
            wallet_instance: Uuid::new_v4(),
            wallet_epoch: Uuid::new_v4(),
            core_epoch: ledger.overlay_stream_id().await?,
            import_enabled: true,
            receipts_per_24h: 10,
            credit_sats_per_24h: 10000,
            maximum_custody_sats: 10000,
            approved: vec![Mint {
                mint_id: "fixture".into(),
                policy_version: "v1".into(),
                maximum_gross_sats: 100,
                maximum_net_sats: 100,
            }],
            deadline_seconds: 1,
            connections_per_second: 8,
        };
        Ok((dir, Server::new(config, ledger)?))
    }
    #[tokio::test]
    async fn wrong_wallet_peer_is_rejected_before_reading_even_a_length() -> Result<()> {
        let (_dir, server) = fixture().await?;
        let (mut stream, _peer) = UnixStream::pair()?;
        assert!(
            tokio::time::timeout(
                Duration::from_millis(100),
                server.serve_connection(&mut stream)
            )
            .await?
            .is_err()
        );
        Ok(())
    }
    #[tokio::test]
    async fn committed_reply_matches_original_after_restart_and_disabled_import() -> Result<()> {
        let (dir, server) = fixture().await?;
        let receipt = WireReceipt::validate(ReceiptData {
            schema_version: 1,
            wallet_instance: server.policy.wallet_instance,
            wallet_epoch: server.policy.wallet_epoch,
            acquisition_id: Uuid::new_v4(),
            transport: "cashu".into(),
            source_ref: "fixture".into(),
            source_author: None,
            mint_id: "fixture".into(),
            unit: "sat".into(),
            gross_sats: 8,
            receive_fee_sats: 1,
            net_sats: 7,
            acquisition_evidence_digest: "12".repeat(32),
            policy_version: "v1".into(),
            credit_pool: "herd".into(),
        })?;
        let authenticated = fixture_authenticate(&server.policy, &receipt.encode()?)?;
        let ack = server.commit(&authenticated).await?;
        ack.validate_for(&receipt, server.policy.core_epoch)?;
        let bytes = ack.encode()?;
        let mut config = server.config.clone();
        drop(server);
        config.import_enabled = false;
        config.receipts_per_24h = 0;
        config.credit_sats_per_24h = 0;
        config.maximum_custody_sats = 0;
        let server = Server::new(
            config,
            LedgerStore::connect(&format!(
                "sqlite://{}",
                dir.path().join("core.sqlite").display()
            ))
            .await?,
        )?;
        let authenticated = fixture_authenticate(&server.policy, &receipt.encode()?)?;
        assert_eq!(server.commit(&authenticated).await?.encode()?, bytes);
        Ok(())
    }
    #[tokio::test]
    async fn offline_config_rejects_implicit_import_budgets_and_duplicate_policies() -> Result<()> {
        let (_dir, server) = fixture().await?;
        let mut config = server.config;
        config.receipts_per_24h = 0;
        assert!(config.validate().is_err());
        config.import_enabled = false;
        config.validate()?;
        config.approved.push(config.approved[0].clone());
        assert!(config.validate().is_err());
        config.approved.pop();
        config.wallet_epoch = Uuid::nil();
        assert!(config.validate().is_err());
        Ok(())
    }
}
