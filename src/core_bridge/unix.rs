//! Unix peer credentials are the authority. Frames are bounded and each accepted
//! connection gets one request. The peer never receives a database path/handle,
//! signer credential, gateway client, grant method or feeder command.
use std::os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt};
use std::{io::ErrorKind, path::PathBuf, sync::Arc, time::Duration};

use anyhow::{Context, Result, bail};
use serde::{Serialize, de::DeserializeOwned};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{UnixListener, UnixStream},
    sync::Semaphore,
    time::{Instant, timeout},
};

use super::{
    AuthenticatedAllowance, AuthenticatedBatch, AuthenticatedFundingPage, AuthenticatedOutcome,
    AuthenticatedReservation, BridgeConfig, MAX_FRAME, Request, Response, Safety,
};
use crate::{config::RuntimeMode, ledger::LedgerStore};

pub fn self_credentials() -> Result<(u32, u32)> {
    let (one, _two) = UnixStream::pair()?;
    let peer = one.peer_cred()?;
    Ok((peer.uid(), peer.gid()))
}

pub struct Server {
    listener: UnixListener,
    peer_uid: u32,
    reward_policy: Option<crate::reward_funding::RewardPolicy>,
    payout_peer_uid: Option<u32>,
    payout_execution_enabled: bool,
    payout_account_id: Option<uuid::Uuid>,
}

struct Resources {
    reads: Arc<Semaphore>,
    verification: Arc<Semaphore>,
    reader: Option<Arc<crate::payout::outcome::Reader>>,
    account: Option<uuid::Uuid>,
}

#[derive(Clone, Copy)]
enum Role {
    Intake,
    Payout,
}

impl Role {
    fn permits(self, request: &Request) -> bool {
        match self {
            Self::Intake => matches!(
                request,
                Request::Events { .. }
                    | Request::MemberOverlay { .. }
                    | Request::RewardResults { .. }
                    | Request::Safety
                    | Request::RewardAllowance { .. }
                    | Request::ReserveReward { .. }
            ),
            Self::Payout => matches!(
                request,
                Request::PayoutFunding { .. }
                    | Request::ClaimPayoutExecution { .. }
                    | Request::PayoutExecutionState { .. }
                    | Request::PayoutOutcome { .. }
                    | Request::PayoutOutcomeProgress { .. }
            ),
        }
    }
}

struct ReadBudget {
    tokens: u8,
    last: Instant,
}

impl ReadBudget {
    fn new(now: Instant) -> Self {
        Self {
            tokens: 4,
            last: now,
        }
    }
    fn allow(&mut self, now: Instant) -> bool {
        let intervals = now
            .checked_duration_since(self.last)
            .unwrap_or_default()
            .as_millis()
            / 500;
        if intervals > 0 {
            self.tokens = self
                .tokens
                .saturating_add(u8::try_from(intervals).unwrap_or(4))
                .min(4);
            self.last = now;
        }
        if self.tokens == 0 {
            return false;
        }
        self.tokens -= 1;
        true
    }
}

impl Server {
    pub async fn bind(config: &BridgeConfig) -> Result<Self> {
        config.validate()?;
        let (uid, _) = self_credentials()?;
        let parent = config
            .socket
            .parent()
            .context("bridge socket has no parent")?;
        let metadata = std::fs::symlink_metadata(parent)?;
        if !metadata.is_dir()
            || metadata.file_type().is_symlink()
            || metadata.uid() != uid
            || metadata.gid() != config.group_gid
            || metadata.mode() & 0o027 != 0
            || metadata.mode() & 0o010 == 0
            || parent.canonicalize()? != parent
        {
            bail!(
                "bridge parent must be canonical, core-owned and traverse-only/readable to the dedicated group"
            );
        }
        match std::fs::symlink_metadata(&config.socket) {
            Ok(old) => {
                if !old.file_type().is_socket()
                    || old.uid() != uid
                    || old.gid() != config.group_gid
                    || old.mode() & 0o007 != 0
                {
                    bail!("refusing to replace an unexpected bridge path");
                }
                match timeout(
                    Duration::from_millis(500),
                    UnixStream::connect(&config.socket),
                )
                .await
                {
                    Ok(Err(error)) if error.kind() == ErrorKind::ConnectionRefused => {}
                    _ => bail!("bridge socket is active or cannot be safely identified as stale"),
                }
                let current = std::fs::symlink_metadata(&config.socket)?;
                if current.ino() != old.ino()
                    || current.dev() != old.dev()
                    || !current.file_type().is_socket()
                {
                    bail!("bridge path changed during stale-socket check");
                }
                std::fs::remove_file(&config.socket)?;
            }
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        let listener = UnixListener::bind(&config.socket)?;
        // Changing only our own socket's group never exposes the core's private
        // state/config directories. Provisioning supplies the separate group.
        std::os::unix::fs::chown(&config.socket, None, Some(config.group_gid))?;
        std::fs::set_permissions(&config.socket, std::fs::Permissions::from_mode(0o660))?;
        Ok(Self {
            listener,
            peer_uid: config.peer_uid,
            reward_policy: config.reward_policy.clone(),
            payout_peer_uid: config.payout_peer_uid,
            payout_execution_enabled: config.payout_execution_enabled,
            payout_account_id: config.payout_account_id,
        })
    }

    pub async fn run(self, ledger: LedgerStore, mode: RuntimeMode) -> Result<()> {
        self.run_with_reader(ledger, mode, None).await
    }

    pub async fn run_with_reader(
        self,
        ledger: LedgerStore,
        mode: RuntimeMode,
        reader: Option<Arc<crate::payout::outcome::Reader>>,
    ) -> Result<()> {
        if reader
            .as_ref()
            .is_some_and(|reader| Some(reader.account_id()) != self.payout_account_id)
        {
            bail!("outcome reader does not match configured account");
        }
        let clients = Arc::new(Semaphore::new(8));
        // At most one bridge database operation competes for the financial pool,
        // including the explicitly enabled bounded reservation write.
        // Other clients get a bounded rejection, never an unbounded waiter queue.
        let resources = Arc::new(Resources {
            reads: Arc::new(Semaphore::new(1)),
            verification: Arc::new(Semaphore::new(1)),
            reader,
            account: self.payout_account_id,
        });
        let mut budget = ReadBudget::new(Instant::now());
        loop {
            let (stream, _) = self.listener.accept().await?;
            let uid = stream.peer_cred()?.uid();
            let role = if uid == self.peer_uid {
                Role::Intake
            } else if self.payout_peer_uid == Some(uid) {
                Role::Payout
            } else {
                continue;
            };
            // Admission precedes frame parsing/crypto so malformed requests
            // cannot transfer unbounded work into the financial runtime UID.
            if !budget.allow(Instant::now()) {
                continue;
            }
            let Ok(permit) = Arc::clone(&clients).try_acquire_owned() else {
                continue;
            };
            let ledger = ledger.clone();
            let resources = Arc::clone(&resources);
            let reward_policy = self.reward_policy.clone();
            let payout_execution_enabled = self.payout_execution_enabled;
            tokio::spawn(async move {
                let _permit = permit;
                let _ = timeout(
                    Duration::from_secs(if matches!(role, Role::Payout) { 22 } else { 10 }),
                    serve_one(
                        stream,
                        &ledger,
                        mode,
                        &resources,
                        reward_policy.as_ref(),
                        role,
                        payout_execution_enabled,
                    ),
                )
                .await;
            });
        }
    }
}

async fn serve_one(
    mut stream: UnixStream,
    ledger: &LedgerStore,
    mode: RuntimeMode,
    resources: &Resources,
    reward_policy: Option<&crate::reward_funding::RewardPolicy>,
    role: Role,
    payout_execution_enabled: bool,
) -> Result<()> {
    let request: Request = match timeout(Duration::from_secs(2), read_frame(&mut stream)).await {
        Ok(Ok(request)) => request,
        _ => return Ok(()),
    };
    if !role.permits(&request) {
        return write_frame(&mut stream, &Response::Rejected).await;
    }
    // Provider waits do not hold the ordinary event/safety IPC permit or a DB
    // transaction. At most one verification request exists, with no wait queue.
    if let Request::PayoutOutcome { packet } = &request {
        let response = match (
            &resources.reader,
            Arc::clone(&resources.verification).try_acquire_owned(),
        ) {
            (Some(reader), Ok(_permit)) => {
                match ledger.reconcile_payout_outcome(packet, reader).await {
                    Ok(progress) => Response::PayoutOutcome {
                        progress: Box::new(progress),
                    },
                    Err(_) => Response::Rejected,
                }
            }
            _ => Response::Rejected,
        };
        return write_frame(&mut stream, &response).await;
    }
    let response = if let Ok(_permit) = Arc::clone(&resources.reads).try_acquire_owned() {
        match request {
            Request::MemberOverlay { message } => {
                if reward_policy.is_some_and(|policy| policy.owner == message.owner) {
                    match ledger.accept_member_overlay(&message, mode).await {
                        Ok(seq) => Response::MemberOverlay {
                            identity: message.identity.clone(),
                            seq,
                        },
                        Err(_) => Response::Rejected,
                    }
                } else {
                    Response::Rejected
                }
            }
            Request::RewardResults { reservation, after } => {
                match ledger.reward_results(&reservation, after).await {
                    Ok(page) => Response::RewardResults {
                        page: page.map(Box::new),
                    },
                    Err(_) => Response::Rejected,
                }
            }
            Request::ClaimPayoutExecution { request } => match ledger
                .claim_payout_execution(
                    &request,
                    crate::payout::execution::Limits {
                        policy: reward_policy,
                        account: resources.account,
                        enabled: payout_execution_enabled && mode.feeder_enabled(),
                        nostr: mode.nostr_enabled(),
                    },
                )
                .await
            {
                Ok(lease) => Response::PayoutExecutionClaim {
                    lease: Box::new(lease),
                },
                Err(_) => Response::Rejected,
            },
            Request::PayoutExecutionState {
                expected_epoch,
                reservation_id,
                owner,
                member,
            } => match ledger
                .payout_execution_state(
                    expected_epoch,
                    reservation_id,
                    &owner,
                    &member,
                    crate::payout::execution::Limits {
                        policy: reward_policy,
                        account: resources.account,
                        enabled: payout_execution_enabled && mode.feeder_enabled(),
                        nostr: mode.nostr_enabled(),
                    },
                )
                .await
            {
                Ok(state) => Response::PayoutExecutionState {
                    state: Box::new(state),
                },
                Err(_) => Response::Rejected,
            },
            Request::PayoutOutcomeProgress { header } => {
                match ledger.payout_outcome_progress(&header).await {
                    Ok(progress) => Response::PayoutOutcome {
                        progress: Box::new(progress),
                    },
                    Err(_) => Response::Rejected,
                }
            }
            Request::PayoutOutcome { .. } => Response::Rejected,
            Request::Events {
                expected_epoch,
                after,
                limit,
            } => match ledger.core_event_batch(expected_epoch, after, limit).await {
                Ok(batch) => Response::Events {
                    batch: Box::new(batch),
                },
                Err(_) => Response::Rejected,
            },
            Request::Safety => match ledger.core_safety(mode).await {
                Ok(state) => Response::Safety { state },
                Err(_) => Response::Rejected,
            },
            Request::PayoutFunding {
                expected_epoch,
                reservation_id,
                owner,
                after,
            } => match ledger
                .payout_funding_page(
                    expected_epoch,
                    reservation_id,
                    &owner,
                    after,
                    reward_policy,
                    // Legacy clients may qualify funding but cannot execute
                    // without the new independently retained claim protocol.
                    false,
                )
                .await
            {
                Ok(mut page) => {
                    page.nostr_enabled = mode.nostr_enabled();
                    Response::PayoutFunding {
                        page: Box::new(page),
                    }
                }
                Err(_) => Response::Rejected,
            },
            Request::RewardAllowance {
                expected_epoch,
                request_id,
                owner,
            } => match reward_policy {
                Some(policy) if policy.owner == owner && mode.feeder_enabled() => match ledger
                    .reward_allowance(policy, expected_epoch, request_id)
                    .await
                {
                    Ok(allowance) => Response::Allowance {
                        allowance: Box::new(allowance),
                    },
                    Err(_) => Response::Rejected,
                },
                _ => Response::Rejected,
            },
            Request::ReserveReward {
                expected_epoch,
                request_id,
                owner,
                plan_hash,
                amount_sats,
            } => match reward_policy {
                Some(policy) if policy.owner == owner && mode.feeder_enabled() => match ledger
                    .reserve_reward(policy, expected_epoch, request_id, &plan_hash, amount_sats)
                    .await
                {
                    Ok(reservation) => Response::Reservation {
                        reservation: Box::new(reservation),
                    },
                    Err(_) => Response::Rejected,
                },
                _ => Response::Rejected,
            },
        }
    } else {
        Response::Rejected
    };
    write_frame(&mut stream, &response).await
}

pub struct Client {
    socket: PathBuf,
    expected_uid: u32,
}

impl Client {
    pub async fn member_overlay(
        &self,
        message: &crate::cyberherd::overlay_delivery::Message,
    ) -> Result<u64> {
        message.validate()?;
        match self
            .request(&Request::MemberOverlay {
                message: Box::new(message.clone()),
            })
            .await?
        {
            Response::MemberOverlay { identity, seq }
                if identity == message.identity && seq > 0 =>
            {
                Ok(seq)
            }
            _ => bail!("core rejected member overlay; retain original message for retry"),
        }
    }
    pub async fn reward_results(
        &self,
        reservation: &crate::reward_funding::Reservation,
        after: u16,
    ) -> Result<Option<super::AuthenticatedRewardResults>> {
        reservation.validate()?;
        if after > 1000 {
            bail!("invalid verified result cursor");
        }
        match self
            .request(&Request::RewardResults {
                reservation: Box::new(reservation.clone()),
                after,
            })
            .await?
        {
            Response::RewardResults { page: Some(page) } => {
                page.validate()?;
                if page.reservation != *reservation || page.after != after {
                    bail!("verified result response has wrong reservation/cursor");
                }
                Ok(Some(super::AuthenticatedRewardResults {
                    page: *page,
                    core_uid: self.expected_uid,
                }))
            }
            Response::RewardResults { page: None } => Ok(None),
            _ => bail!("core rejected verified result read"),
        }
    }
    pub async fn claim_execution(
        &self,
        request: &crate::payout::execution::Request,
    ) -> Result<super::AuthenticatedExecutionLease> {
        request.quote()?;
        match self
            .request(&Request::ClaimPayoutExecution {
                request: Box::new(request.clone()),
            })
            .await?
        {
            Response::PayoutExecutionClaim { lease } => {
                lease.validate()?;
                if lease.record.request != *request {
                    bail!("core returned a different execution claim");
                }
                Ok(super::AuthenticatedExecutionLease {
                    lease: *lease,
                    core_uid: self.expected_uid,
                })
            }
            _ => bail!("core rejected execution identity; retain original work for reconciliation"),
        }
    }
    pub async fn execution_state(
        &self,
        epoch: uuid::Uuid,
        id: uuid::Uuid,
        owner: &str,
        member: &str,
    ) -> Result<super::AuthenticatedExecutionState> {
        crate::nostr_event::validate_public_key(owner)?;
        crate::nostr_event::validate_public_key(member)?;
        match self
            .request(&Request::PayoutExecutionState {
                expected_epoch: epoch,
                reservation_id: id,
                owner: owner.into(),
                member: member.into(),
            })
            .await?
        {
            Response::PayoutExecutionState { state } => {
                state.validate(epoch, id, owner, member)?;
                let claim = state.claim.map(|lease| super::AuthenticatedExecutionLease {
                    lease,
                    core_uid: self.expected_uid,
                });
                let page = AuthenticatedFundingPage {
                    page: state.page,
                    core_uid: self.expected_uid,
                };
                Ok(super::AuthenticatedExecutionState { page, claim })
            }
            _ => bail!("core did not return execution state"),
        }
    }
    pub fn new(socket: PathBuf, expected_uid: u32) -> Result<Self> {
        if !socket.is_absolute() || socket.as_os_str().len() > 100 {
            bail!("invalid core socket path");
        }
        Ok(Self {
            socket,
            expected_uid,
        })
    }
    async fn request(&self, request: &Request) -> Result<Response> {
        timeout(
            Duration::from_secs(if matches!(request, Request::PayoutOutcome { .. }) {
                25
            } else {
                12
            }),
            async {
                let mut stream = UnixStream::connect(&self.socket).await?;
                if stream.peer_cred()?.uid() != self.expected_uid {
                    bail!("core socket peer UID does not match expected identity");
                }
                write_frame(&mut stream, request).await?;
                read_frame(&mut stream).await
            },
        )
        .await
        .context("core request deadline exceeded")?
    }
    pub async fn payout_outcome(
        &self,
        packet: &crate::payout::outcome::Packet,
    ) -> Result<AuthenticatedOutcome> {
        packet.validate()?;
        self.outcome_request(
            &Request::PayoutOutcome {
                packet: Box::new(packet.clone()),
            },
            &packet.header,
        )
        .await
    }
    pub async fn payout_outcome_progress(
        &self,
        header: &crate::payout::outcome::Header,
    ) -> Result<AuthenticatedOutcome> {
        header.validate()?;
        self.outcome_request(
            &Request::PayoutOutcomeProgress {
                header: Box::new(header.clone()),
            },
            header,
        )
        .await
    }
    async fn outcome_request(
        &self,
        request: &Request,
        header: &crate::payout::outcome::Header,
    ) -> Result<AuthenticatedOutcome> {
        match self.request(request).await? {
            Response::PayoutOutcome { progress } => {
                progress.validate(header)?;
                Ok(AuthenticatedOutcome {
                    progress: *progress,
                    core_uid: self.expected_uid,
                })
            }
            _ => bail!("core did not verify payout outcome"),
        }
    }
    pub async fn events(
        &self,
        expected_epoch: Option<uuid::Uuid>,
        after: u64,
        limit: u16,
    ) -> Result<AuthenticatedBatch> {
        match self
            .request(&Request::Events {
                expected_epoch,
                after,
                limit,
            })
            .await?
        {
            Response::Events { batch } => {
                batch.validate(expected_epoch, after)?;
                Ok(AuthenticatedBatch {
                    batch: *batch,
                    core_uid: self.expected_uid,
                })
            }
            _ => bail!("core did not return an authenticated event batch"),
        }
    }
    pub async fn safety(&self) -> Result<Safety> {
        match self.request(&Request::Safety).await? {
            Response::Safety { mut state } => {
                state.availability = state.current_availability(crate::cyberherd::calendar::now()?);
                Ok(state)
            }
            _ => bail!("core did not return authenticated safety state"),
        }
    }
    pub async fn reserve_reward(
        &self,
        epoch: uuid::Uuid,
        request_id: uuid::Uuid,
        owner: &str,
        plan_hash: &str,
        amount_sats: u64,
    ) -> Result<AuthenticatedReservation> {
        crate::nostr_event::validate_public_key(owner)?;
        crate::nostr_event::validate_hex(plan_hash, 32)?;
        match self
            .request(&Request::ReserveReward {
                expected_epoch: epoch,
                request_id,
                owner: owner.into(),
                plan_hash: plan_hash.into(),
                amount_sats,
            })
            .await?
        {
            Response::Reservation { reservation } => {
                reservation.validate()?;
                if reservation.epoch != epoch
                    || reservation.request_id != request_id
                    || reservation.owner != owner
                    || reservation.plan_hash != plan_hash
                    || reservation.reserved_sats != amount_sats
                {
                    bail!("core reservation does not match frozen request");
                }
                Ok(AuthenticatedReservation {
                    reservation: *reservation,
                    core_uid: self.expected_uid,
                })
            }
            _ => bail!("core did not authorize this reward reservation"),
        }
    }

    pub async fn payout_funding(
        &self,
        epoch: uuid::Uuid,
        reservation_id: uuid::Uuid,
        owner: &str,
        after: i64,
    ) -> Result<AuthenticatedFundingPage> {
        crate::nostr_event::validate_public_key(owner)?;
        if epoch.is_nil() || reservation_id.is_nil() || after < 0 {
            bail!("invalid payout funding read scope");
        }
        match self
            .request(&Request::PayoutFunding {
                expected_epoch: epoch,
                reservation_id,
                owner: owner.into(),
                after,
            })
            .await?
        {
            Response::PayoutFunding { mut page } => {
                page.validate()?;
                page.execution_enabled =
                    page.execution_permitted(crate::cyberherd::calendar::now()?);
                if page.reservation.epoch != epoch
                    || page.reservation.id != reservation_id
                    || page.reservation.owner != owner
                    || page.after != after
                {
                    bail!("core returned a different payout funding scope");
                }
                Ok(AuthenticatedFundingPage {
                    page: *page,
                    core_uid: self.expected_uid,
                })
            }
            _ => bail!("core did not authorize payout funding read"),
        }
    }

    pub async fn reward_allowance(
        &self,
        epoch: uuid::Uuid,
        request_id: uuid::Uuid,
        owner: &str,
    ) -> Result<AuthenticatedAllowance> {
        crate::nostr_event::validate_public_key(owner)?;
        match self
            .request(&Request::RewardAllowance {
                expected_epoch: epoch,
                request_id,
                owner: owner.into(),
            })
            .await?
        {
            Response::Allowance { allowance } => {
                allowance.validate()?;
                if allowance.epoch != epoch
                    || allowance.request_id != request_id
                    || allowance.owner != owner
                {
                    bail!("core allowance does not match confirmed feeding scope");
                }
                Ok(AuthenticatedAllowance {
                    allowance: *allowance,
                    core_uid: self.expected_uid,
                })
            }
            _ => bail!("core did not authorize reward planning"),
        }
    }
}

async fn read_frame<T: DeserializeOwned>(stream: &mut UnixStream) -> Result<T> {
    let length = usize::try_from(stream.read_u32().await?)?;
    if length == 0 || length > MAX_FRAME {
        bail!("core frame exceeds bound");
    }
    let mut bytes = vec![0; length];
    stream.read_exact(&mut bytes).await?;
    serde_json::from_slice(&bytes).context("invalid core frame schema")
}

async fn write_frame<T: Serialize>(stream: &mut UnixStream, value: &T) -> Result<()> {
    let bytes = serde_json::to_vec(value)?;
    if bytes.is_empty() || bytes.len() > MAX_FRAME {
        bail!("core frame exceeds bound");
    }
    stream.write_u32(u32::try_from(bytes.len())?).await?;
    stream.write_all(&bytes).await?;
    stream.flush().await?;
    Ok(())
}

/// Bridge failure must not stop core settlement/feeding workers. No optional
/// config means no socket or credential capability is created.
pub async fn run_optional(config: Option<BridgeConfig>, ledger: LedgerStore, mode: RuntimeMode) {
    run_optional_with_reader(config, ledger, mode, None).await;
}

pub async fn run_optional_with_reader(
    config: Option<BridgeConfig>,
    ledger: LedgerStore,
    mode: RuntimeMode,
    reader: Option<crate::payout::outcome::Reader>,
) {
    let reader = reader.map(Arc::new);
    let Some(config) = config else {
        std::future::pending::<()>().await;
        return;
    };
    let Ok((uid, gid)) = self_credentials() else {
        return;
    };
    if uid == 0 || config.peer_uid == uid || config.group_gid == gid {
        tracing::error!(
            "core bridge disabled: runtime, peer and bridge-group identities must be separate"
        );
        return;
    }
    loop {
        if config.reward_policy.is_some() && ledger.initialize_reward_funding().await.is_err() {
            tracing::warn!(
                "reward funding unavailable; core financial workers continue independently"
            );
            tokio::time::sleep(Duration::from_secs(5)).await;
            continue;
        }
        match Server::bind(&config).await {
            Ok(server) => {
                let _ = server
                    .run_with_reader(ledger.clone(), mode, reader.clone())
                    .await;
            }
            Err(_) => tracing::warn!(
                "core bridge unavailable; financial processing continues independently"
            ),
        }
        tokio::time::sleep(Duration::from_secs(5)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{core_bridge::EventBody, domain::payment::SettledPayment};

    #[test]
    fn read_request_budget_has_finite_burst_and_monotonic_refill() {
        let now = Instant::now();
        let mut budget = ReadBudget::new(now);
        for _ in 0..4 {
            assert!(budget.allow(now));
        }
        assert!(!budget.allow(now));
        assert!(!budget.allow(now + Duration::from_millis(499)));
        assert!(budget.allow(now + Duration::from_millis(500)));
        assert!(!budget.allow(now + Duration::from_millis(500)));
        for _ in 0..4 {
            assert!(budget.allow(now + Duration::from_secs(100)));
        }
        assert!(!budget.allow(now + Duration::from_secs(100)));
    }

    async fn fixture(peer_offset: u32) -> (tempfile::TempDir, BridgeConfig, LedgerStore) {
        let root = tempfile::TempDir::new().unwrap();
        std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o750)).unwrap();
        let (uid, gid) = self_credentials().unwrap();
        let config = BridgeConfig {
            socket: root.path().join("core.sock"),
            peer_uid: uid + peer_offset,
            group_gid: gid,
            reward_policy: None,
            payout_peer_uid: None,
            payout_execution_enabled: false,
            payout_account_id: None,
        };
        let ledger = LedgerStore::connect(&format!(
            "sqlite://{}",
            root.path().join("core.db").display()
        ))
        .await
        .unwrap();
        ledger
            .record_payment(&SettledPayment {
                source: "strike".into(),
                source_id: "synthetic-private-core".into(),
                payment_hash: None,
                address_user: "herd".into(),
                credit_pool: "herd".into(),
                amount_msat: 1_000_000,
                settled_at: None,
                context_json: None,
            })
            .await
            .unwrap();
        (root, config, ledger)
    }

    #[tokio::test]
    async fn actual_unix_peer_credentials_protect_reads_on_both_ends() {
        let (_root, config, ledger) = fixture(0).await;
        let server = Server::bind(&config).await.unwrap();
        let task = tokio::spawn(server.run(ledger.clone(), RuntimeMode::Active));
        let uid = self_credentials().unwrap().0;
        let client = Client::new(config.socket.clone(), uid).unwrap();
        let batch = client.events(None, 0, 16).await.unwrap();
        assert_eq!(batch.events.len(), 1);
        assert!(matches!(batch.events[0].body, EventBody::Payment(_)));
        let wrong = Client::new(config.socket.clone(), uid + 1).unwrap();
        assert!(wrong.events(None, 0, 16).await.is_err());
        assert!(
            client
                .events(Some(uuid::Uuid::new_v4()), 0, 16)
                .await
                .is_err()
        );
        assert_eq!(ledger.feed_credit_sats().await.unwrap(), 1000);
        assert_eq!(
            client.safety().await.unwrap().availability,
            super::super::Availability::Unknown
        );
        task.abort();
    }

    #[tokio::test]
    async fn server_rejects_the_wrong_peer_before_parsing_a_request() {
        let (_root, config, ledger) = fixture(1).await;
        let server = Server::bind(&config).await.unwrap();
        let task = tokio::spawn(server.run(ledger.clone(), RuntimeMode::Active));
        let client = Client::new(config.socket, self_credentials().unwrap().0).unwrap();
        assert!(client.events(None, 0, 16).await.is_err());
        assert_eq!(ledger.feed_credit_sats().await.unwrap(), 1000);
        task.abort();
    }

    #[tokio::test]
    async fn oversized_frames_and_forged_mutation_methods_never_reach_financial_state() {
        let (_root, config, ledger) = fixture(0).await;
        let server = Server::bind(&config).await.unwrap();
        let task = tokio::spawn(server.run(ledger.clone(), RuntimeMode::Active));
        for request in [
            serde_json::json!({"method":"credit","amount_sats":1000000}),
            serde_json::json!({"method":"events","after":0,"limit":1,"sql":"DELETE FROM ledger_entries"}),
        ] {
            let mut stream = UnixStream::connect(&config.socket).await.unwrap();
            write_frame(&mut stream, &request).await.unwrap();
            assert!(
                timeout(Duration::from_secs(2), read_frame::<Response>(&mut stream))
                    .await
                    .unwrap()
                    .is_err()
            );
        }
        let mut stream = UnixStream::connect(&config.socket).await.unwrap();
        stream.write_u32((MAX_FRAME + 1) as u32).await.unwrap();
        assert!(
            timeout(Duration::from_secs(2), read_frame::<Response>(&mut stream))
                .await
                .unwrap()
                .is_err()
        );
        assert_eq!(ledger.feed_credit_sats().await.unwrap(), 1000);
        assert_eq!(ledger.latest_event_seq().await.unwrap(), 1);
        task.abort();
    }

    #[tokio::test]
    async fn binding_preserves_live_or_unexpected_paths_and_only_reclaims_stale_owned_socket() {
        let (root, config, _ledger) = fixture(0).await;
        let first = Server::bind(&config).await.unwrap();
        assert!(Server::bind(&config).await.is_err());
        drop(first);
        let replacement = Server::bind(&config).await.unwrap();
        drop(replacement);
        std::fs::remove_file(&config.socket).unwrap();
        std::fs::write(&config.socket, b"must remain").unwrap();
        assert!(Server::bind(&config).await.is_err());
        assert_eq!(std::fs::read(&config.socket).unwrap(), b"must remain");
        std::fs::remove_file(&config.socket).unwrap();
        let target = root.path().join("target");
        std::fs::write(&target, b"preserve").unwrap();
        std::os::unix::fs::symlink(&target, &config.socket).unwrap();
        assert!(Server::bind(&config).await.is_err());
        assert_eq!(std::fs::read(target).unwrap(), b"preserve");
        std::fs::remove_file(&config.socket).unwrap();
        std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o770)).unwrap();
        assert!(Server::bind(&config).await.is_err());
    }

    #[tokio::test]
    async fn typed_allowance_requires_policy_mode_owner_epoch_and_actual_confirmation() {
        use crate::{nostr_event::test_event, reward_funding::RewardPolicy};
        for (enabled, mode) in [
            (false, RuntimeMode::Active),
            (true, RuntimeMode::Shadow),
            (true, RuntimeMode::Active),
        ] {
            let (_root, mut config, ledger) = fixture(0).await;
            ledger.initialize_reward_funding().await.unwrap();
            let id = ledger.begin_feed_attempt(1000).await.unwrap().unwrap();
            ledger.confirm_feed_attempt(id).await.unwrap();
            let epoch = ledger.overlay_stream_id().await.unwrap();
            let owner = test_event(1, 1, 100, "", vec![]).pubkey;
            if enabled {
                config.reward_policy = Some(RewardPolicy {
                    owner: owner.clone(),
                    member_percent_ceiling: 10,
                    max_sats_per_feed: 100,
                    max_sats_per_24h: 1000,
                });
            }
            let server = Server::bind(&config).await.unwrap();
            let task = tokio::spawn(server.run(ledger.clone(), mode));
            let uid = self_credentials().unwrap().0;
            let client = Client::new(config.socket.clone(), uid).unwrap();
            let value = client.reward_allowance(epoch, id, &owner).await;
            if enabled && mode != RuntimeMode::Shadow {
                let value = value.unwrap();
                assert_eq!(value.core_uid(), uid);
                assert_eq!(
                    (
                        value.allowance().funding.strike_sats,
                        value.allowance().threshold_sats,
                        value.allowance().available_sats
                    ),
                    (1000, 1000, 1000)
                );
                let wrong = test_event(2, 1, 100, "", vec![]).pubkey;
                assert!(client.reward_allowance(epoch, id, &wrong).await.is_err());
                assert!(
                    client
                        .reward_allowance(uuid::Uuid::new_v4(), id, &owner)
                        .await
                        .is_err()
                );
                assert!(
                    client
                        .reward_allowance(epoch, uuid::Uuid::new_v4(), &owner)
                        .await
                        .is_err()
                );
            } else {
                assert!(value.is_err());
            }
            assert_eq!(ledger.feed_credit_sats().await.unwrap(), 0);
            task.abort();
            let _ = task.await;
        }
    }

    #[tokio::test]
    async fn typed_reward_reservations_require_root_policy_mode_owner_and_exact_frozen_plan() {
        use crate::{nostr_event::test_event, reward_funding::RewardPolicy};
        for (enabled, mode) in [
            (false, RuntimeMode::Active),
            (true, RuntimeMode::Shadow),
            (true, RuntimeMode::Active),
        ] {
            let (root, mut config, ledger) = fixture(0).await;
            ledger.initialize_reward_funding().await.unwrap();
            let id = ledger.begin_feed_attempt(1000).await.unwrap().unwrap();
            ledger.confirm_feed_attempt(id).await.unwrap();
            let epoch = ledger.overlay_stream_id().await.unwrap();
            let owner = test_event(1, 1, 100, "", vec![]).pubkey;
            if enabled {
                config.reward_policy = Some(RewardPolicy {
                    owner: owner.clone(),
                    member_percent_ceiling: 10,
                    max_sats_per_feed: 100,
                    max_sats_per_24h: 1000,
                });
            }
            let server = Server::bind(&config).await.unwrap();
            let task = tokio::spawn(server.run(ledger.clone(), mode));
            let uid = self_credentials().unwrap().0;
            let client = Client::new(config.socket.clone(), uid).unwrap();
            let hash = "ab".repeat(32);
            if !enabled || mode == RuntimeMode::Shadow {
                assert!(
                    client
                        .reserve_reward(epoch, id, &owner, &hash, 100)
                        .await
                        .is_err()
                );
                let pool = sqlx::SqlitePool::connect(&format!(
                    "sqlite://{}",
                    root.path().join("core.db").display()
                ))
                .await
                .unwrap();
                let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM reward_reservations")
                    .fetch_one(&pool)
                    .await
                    .unwrap();
                assert_eq!(n, 0);
            } else {
                let wrong = test_event(2, 1, 100, "", vec![]).pubkey;
                assert!(
                    client
                        .reserve_reward(epoch, id, &wrong, &hash, 100)
                        .await
                        .is_err()
                );
                let first = client
                    .reserve_reward(epoch, id, &owner, &hash, 100)
                    .await
                    .unwrap();
                assert_eq!(first.core_uid(), uid);
                assert_eq!(first.reservation().reserved_sats, 100);
                assert_eq!(
                    client
                        .reserve_reward(epoch, id, &owner, &hash, 100)
                        .await
                        .unwrap()
                        .reservation(),
                    first.reservation()
                );
                assert!(
                    client
                        .reserve_reward(epoch, id, &owner, &"cd".repeat(32), 100)
                        .await
                        .is_err()
                );
            }
            assert_eq!(ledger.feed_credit_sats().await.unwrap(), 0);
            task.abort();
            let _ = task.await;
        }
    }
}
