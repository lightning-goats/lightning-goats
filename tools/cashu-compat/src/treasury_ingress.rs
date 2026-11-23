//! Dedicated private treasury admission/status endpoint. Never invokes a mint.
use crate::{journal::Journal, treasury::TreasuryBinding, writer_lock::WriterLock};
use anyhow::{Result, ensure};
use lightning_goats_cashu_receipt::{treasury::MAX_TREASURY_BYTES, treasury_rpc::Request};
use std::{
    collections::BTreeMap,
    path::Path,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::UnixStream,
};

pub struct Intake<'a> {
    pub core_uid: u32,
    pub accept_new: bool,
    pub bindings: &'a BTreeMap<String, TreasuryBinding>,
    pub journal: Arc<Mutex<Journal>>,
    pub writer: &'a WriterLock,
}
impl Intake<'_> {
    pub async fn serve(
        &self,
        path: &Path,
        group: u32,
        deadline: Duration,
        connections_per_second: u32,
        shutdown: impl std::future::Future<Output = ()>,
    ) -> Result<()> {
        ensure!(
            (1..=64).contains(&connections_per_second),
            "invalid treasury connection budget"
        );
        self.check_identity()?;
        check_deadline(deadline)?;
        let listener = crate::private_ipc::bind(path, group).await?;
        let cooldown = Duration::from_secs(1) / connections_per_second;
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
    fn check_identity(&self) -> Result<()> {
        let uid = rustix::process::geteuid().as_raw();
        ensure!(
            uid > 0 && self.core_uid > 0 && uid != self.core_uid,
            "treasury IPC requires distinct non-root identities"
        );
        Ok(())
    }
    /// Authenticate before reading even the frame length. One request per connection.
    pub async fn serve_connection(
        &self,
        stream: &mut UnixStream,
        deadline: Duration,
    ) -> Result<()> {
        self.check_identity()?;
        ensure!(
            stream.peer_cred()?.uid() == self.core_uid,
            "treasury peer not authorized"
        );
        check_deadline(deadline)?;
        tokio::time::timeout(deadline, async {
            let length = usize::try_from(stream.read_u32().await?)?;
            ensure!(
                length > 0 && length <= MAX_TREASURY_BYTES,
                "treasury frame exceeds bound"
            );
            let mut bytes = vec![0; length];
            stream.read_exact(&mut bytes).await?;
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_secs();
            let reply = self
                .admit_frame(&bytes, now)
                .map_err(|_| anyhow::anyhow!("private treasury request rejected"))?;
            stream.write_u32(u32::try_from(reply.len())?).await?;
            stream.write_all(&reply).await?;
            stream.flush().await?;
            Ok(())
        })
        .await
        .map_err(|_| anyhow::anyhow!("treasury IPC deadline exceeded"))?
    }
    fn admit_frame(&self, bytes: &[u8], now: u64) -> Result<Vec<u8>> {
        let request = Request::decode(bytes)?;
        let binding = self
            .bindings
            .get(&request.intent().mint_id)
            .ok_or_else(|| anyhow::anyhow!("treasury mint is not configured"))?;
        let reply = self
            .journal
            .lock()
            .map_err(|_| anyhow::anyhow!("wallet journal unavailable"))?
            .treasury_rpc(binding, &request, self.accept_new, now)?;
        let bytes = serde_json::to_vec(&reply)?;
        ensure!(
            bytes.len() <= MAX_TREASURY_BYTES,
            "treasury reply exceeds bound"
        );
        Ok(bytes)
    }
}
fn check_deadline(deadline: Duration) -> Result<()> {
    ensure!(
        !deadline.is_zero() && deadline <= Duration::from_secs(60),
        "invalid treasury deadline"
    );
    Ok(())
}

#[cfg(test)]
mod cross_uid_fixture;

#[cfg(test)]
mod tests {
    use super::*;
    use lightning_goats_cashu_receipt::treasury_rpc::{Command, Phase, Reply};
    use uuid::Uuid;

    #[tokio::test]
    async fn treasury_admission_replays_offline_and_rejects_peers_before_read() -> Result<()> {
        let (binding, issued) = crate::treasury::tests::fixture();
        let dir = crate::test_directory()?;
        let writer = WriterLock::acquire(dir.path())?;
        let path = dir.path().join("journal.sqlite");
        let journal = Arc::new(Mutex::new(Journal::open(&path, binding.wallet_epoch, 8)?));
        let bindings = BTreeMap::from([(binding.mint_id.clone(), binding.clone())]);
        let mut intake = Intake {
            core_uid: rustix::process::geteuid().as_raw() + 1,
            accept_new: false,
            bindings: &bindings,
            journal: journal.clone(),
            writer: &writer,
        };
        let request = Request {
            version: 1,
            request_id: Uuid::from_u128(42),
            command: Command::Retain(Box::new(issued.clone())),
        };
        let bytes = request.encode()?;
        assert!(intake.admit_frame(&bytes, 1000).is_err());
        intake.accept_new = true;
        let reply = Reply::decode_for(&intake.admit_frame(&bytes, 1000)?, &request)?;
        assert!(reply.phase == Phase::Bound && reply.report.is_none());
        // Drop/reopen the journal, simulating loss of the first acknowledgment.
        intake.journal = Arc::new(Mutex::new(Journal::open(&path, binding.wallet_epoch, 8)?));
        intake.accept_new = false;
        let replay = intake.admit_frame(&bytes, 2000)?;
        Reply::decode_for(&replay, &request)?;
        let status = Request {
            version: 1,
            request_id: Uuid::from_u128(43),
            command: Command::Status(Box::new(issued.intent.clone())),
        };
        Reply::decode_for(&intake.admit_frame(&status.encode()?, 2000)?, &status)?;
        assert!(Reply::decode_for(&replay, &status).is_err());
        let mut altered = issued.clone();
        altered.invoice.push('x');
        let bad = Request {
            version: 1,
            request_id: request.request_id,
            command: Command::Retain(Box::new(altered)),
        };
        assert!(intake.admit_frame(&bad.encode()?, 2000).is_err());
        let mut foreign = issued.intent.clone();
        foreign.core_epoch = Uuid::from_u128(999);
        let bad = Request {
            version: 1,
            request_id: request.request_id,
            command: Command::Status(Box::new(foreign)),
        };
        assert!(intake.admit_frame(&bad.encode()?, 2000).is_err());
        assert!(
            intake
                .admit_frame(&vec![b' '; MAX_TREASURY_BYTES + 1], 1000)
                .is_err()
        );
        assert!(
            intake
                .admit_frame(br#"{"version":1,"command":{"action":"melt"}}"#, 1000)
                .is_err()
        );
        let (mut server, _client) = UnixStream::pair()?;
        // Client sends nothing. Refusal must precede waiting for any frame bytes.
        assert!(
            tokio::time::timeout(
                Duration::from_millis(200),
                intake.serve_connection(&mut server, Duration::from_secs(2))
            )
            .await?
            .is_err()
        );
        let db = rusqlite::Connection::open(path)?;
        let counts: (i64, i64, i64) = db.query_row("SELECT (SELECT COUNT(*) FROM treasury_instructions),(SELECT COUNT(*) FROM treasury_source_holds),(SELECT COUNT(*) FROM treasury_quote_requests)", [], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
        assert_eq!(counts, (1, 0, 0));
        Ok(())
    }
}
