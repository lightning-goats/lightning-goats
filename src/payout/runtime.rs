//! Standalone payout process: no public listener, core DB, feeder, Nostr key or
//! signing credential. Intake and recovery run independently and remain bounded.
use super::{
    funding::{ExpectedPlan, Session, SourceVerifier},
    provider::Client as Sender,
    signer::Client as Signer,
    store::{Policy, Store},
    worker::Worker,
};
use crate::{
    core_bridge::unix::Client as Core,
    cyberherd::{calendar, payout_bridge::Client as Plans},
    private_ipc as ipc,
};
use anyhow::{Context, Result, bail};
use serde::Deserialize;
use std::{
    path::{Path, PathBuf},
    time::Duration,
};

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub enabled: bool,
    pub state_dir: PathBuf,
    pub core_socket: PathBuf,
    pub intake_socket: PathBuf,
    pub intake_uid: u32,
    pub signer_socket: PathBuf,
    pub signer_uid: u32,
    pub poll_millis: u64,
    pub intake_poll_seconds: u64,
    pub policy: Policy,
}
impl Config {
    pub fn load(path: &Path) -> Result<Self> {
        let raw = std::fs::read_to_string(path)?;
        if raw.len() > 65536 {
            bail!("payout configuration exceeds limit");
        }
        let config: Self = toml::from_str(&raw)
            .map_err(|_| anyhow::anyhow!("invalid payout configuration fields"))?;
        config.validate()?;
        Ok(config)
    }
    pub fn validate(&self) -> Result<()> {
        self.policy.validate()?;
        for socket in [&self.core_socket, &self.intake_socket, &self.signer_socket] {
            ipc::socket_path(socket)?;
        }
        let identities = [self.intake_uid, self.signer_uid, self.policy.scope.core_uid];
        if !self.state_dir.is_absolute()
            || identities.contains(&0)
            || identities
                .iter()
                .collect::<std::collections::BTreeSet<_>>()
                .len()
                != 3
            || !(500..=60000).contains(&self.poll_millis)
            || !(5..=60).contains(&self.intake_poll_seconds)
            || self.core_socket == self.intake_socket
            || self.core_socket == self.signer_socket
            || self.intake_socket == self.signer_socket
        {
            bail!("payout requires separate identities/sockets and bounded polls");
        }
        Ok(())
    }
    pub async fn open(self, path: &Path) -> Result<Runtime> {
        self.validate()?;
        if !self.enabled {
            bail!("payout worker is disabled");
        }
        ipc::protected_config(path)?;
        ipc::private_state(&self.state_dir)?;
        if [self.intake_uid, self.signer_uid, self.policy.scope.core_uid].contains(&ipc::uid()?) {
            bail!("payout requires its own runtime UID");
        }
        let again = Config::load(path)?;
        if self != again {
            bail!("payout configuration changed during startup");
        }
        let singleton = ipc::singleton("cyberherd-payout", &self.state_dir)?;
        let db = self.state_dir.join("payout.db");
        let store = Store::connect(
            &format!(
                "sqlite://{}",
                db.to_str().context("state path must be UTF-8")?
            ),
            self.policy.clone(),
        )
        .await?;
        store.initialize_intake(self.intake_uid).await?;
        let key = crate::secrets::read_systemd_credential("strike-payout-api-key").await?;
        let provider = &self.policy.scope.provider;
        let verifier = SourceVerifier::new(
            provider.account_id,
            &provider.credential_fingerprint,
            key.clone(),
        )?;
        let sender = Sender::new(provider.clone(), key)?;
        let worker = Worker {
            store: store.clone(),
            core: Core::new(self.core_socket.clone(), self.policy.scope.core_uid)?,
            signer: Signer::new(self.signer_socket.clone(), self.signer_uid)?,
            sender,
            presentation: Some(Plans::new(
                self.intake_socket.clone(),
                self.intake_uid,
                self.policy.scope.owner.clone(),
            )?),
        };
        let intake = Intake {
            store,
            core: Core::new(self.core_socket.clone(), self.policy.scope.core_uid)?,
            plans: Plans::new(
                self.intake_socket.clone(),
                self.intake_uid,
                self.policy.scope.owner.clone(),
            )?,
            verifier,
        };
        Ok(Runtime {
            worker,
            intake,
            config: self,
            _singleton: singleton,
        })
    }
}
pub struct Intake {
    pub store: Store,
    pub core: Core,
    pub plans: Plans,
    pub verifier: SourceVerifier,
}
impl Intake {
    pub async fn tick(&self) -> Result<bool> {
        let Some(authenticated) = self.plans.next(self.store.intake_cursor().await?).await? else {
            return Ok(false);
        };
        let export = authenticated.export();
        let policy = self.store.policy();
        if export.plan.owner != policy.scope.owner
            || export.plan.epoch != policy.scope.epoch
            || export.plan.core_uid != policy.scope.core_uid
        {
            bail!("plan export has wrong core/owner scope");
        }
        if let Some(reservation) = &export.reservation {
            if self.store.grant(reservation.id).await?.is_none() {
                let mut page = self
                    .core
                    .payout_funding(policy.scope.epoch, reservation.id, &policy.scope.owner, 0)
                    .await?;
                let expected = ExpectedPlan {
                    core_uid: policy.scope.core_uid,
                    epoch: policy.scope.epoch,
                    owner: policy.scope.owner.clone(),
                    plan_hash: export.hash.clone(),
                    member_percent_ceiling: policy.member_percent_ceiling,
                    per_feed_sats: policy.max_sats_per_feed,
                    max_parts: policy.max_source_parts,
                };
                let mut session = Session::new(expected, &page)?;
                loop {
                    session.accept(&page, &self.verifier).await?;
                    if session.is_complete() {
                        break;
                    }
                    page = self
                        .core
                        .payout_funding(
                            policy.scope.epoch,
                            reservation.id,
                            &policy.scope.owner,
                            session.after(),
                        )
                        .await?;
                }
                self.store
                    .admit(&export.plan, &session.finish()?, calendar::now()?)
                    .await?;
            }
        }
        // Admission and this receipt may be separated by a crash. Exact retry
        // uses the existing admitted grant, not another reservation/payment.
        self.store.acknowledge_intake(&authenticated).await?;
        Ok(true)
    }
}
pub struct Runtime {
    worker: Worker,
    intake: Intake,
    config: Config,
    _singleton: std::os::unix::net::UnixListener,
}
impl Runtime {
    pub async fn run(self) -> Result<()> {
        let _singleton = self._singleton;
        let mut tasks = tokio::task::JoinSet::new();
        let intake = self.intake;
        let intake_seconds = self.config.intake_poll_seconds;
        tasks.spawn(async move {
            let mut ticks = tokio::time::interval(Duration::from_secs(intake_seconds));
            ticks.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                ticks.tick().await;
                if intake.tick().await.is_err() {
                    tracing::warn!("payout intake held; retaining cursor and admitted grants");
                }
            }
        });
        let worker = self.worker;
        let millis = self.config.poll_millis;
        tasks.spawn(async move {
            let mut ticks = tokio::time::interval(Duration::from_millis(millis));
            ticks.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                ticks.tick().await;
                if worker.tick().await.is_err() {
                    tracing::warn!("payout work held; retaining original journal identity");
                }
            }
        });
        tokio::select! {
            _=tokio::signal::ctrl_c()=>Ok(()),
            _=tasks.join_next()=>bail!("payout child worker stopped unexpectedly"),
        }
    }
}
