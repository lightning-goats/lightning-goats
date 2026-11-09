//! Separate intake/membership process. No Strike credential, core DB handle,
//! Nostr signer or physical gateway is present. Funded payouts and public
//! delivery are deliberately separate capabilities, not inferred from a feed.
use anyhow::{Context, Result, bail};
use serde::Deserialize;
use std::{
    os::unix::fs::{MetadataExt, PermissionsExt},
    path::{Path, PathBuf},
    time::Duration,
};

use super::{
    calendar::{self, DayWindow},
    discovery::Discovery,
    ingress::{CoreBinding, Input},
    membership::{FeederAvailability, Settings},
    relay::Reader,
    social::SocialEvent,
    store::Store,
    targets::TargetPolicy,
    worker::{self, Environment, Step},
};
use crate::{
    core_bridge::{
        Availability,
        unix::{Client, self_credentials},
    },
    zap::ZapRequest,
};

#[derive(Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub enabled: bool,
    pub state_dir: PathBuf,
    pub core_socket: PathBuf,
    pub core: CoreBinding,
    pub project_pubkey: String,
    pub tracked_tags: Vec<String>,
    pub timezone: String,
    pub midnight_reset: bool,
    pub relays: Vec<String>,
    pub required_relays: usize,
    pub poll_seconds: u64,
    pub settings: Settings,
}

impl Config {
    pub fn load(path: &Path) -> Result<Self> {
        let raw = std::fs::read_to_string(path).context("cannot read CyberHerd configuration")?;
        if raw.len() > 65536 {
            bail!("CyberHerd configuration too large");
        }
        let config: Self = toml::from_str(&raw)?;
        config.validate()?;
        Ok(config)
    }
    pub fn validate(&self) -> Result<()> {
        self.core.validate()?;
        self.settings.validate()?;
        self.policy().validate()?;
        DayWindow::at(&self.timezone, calendar::now()?)?;
        let reader = Reader::new(self.relays.clone())?;
        if self.required_relays == 0
            || self.required_relays > reader.relay_count()
            || !(5..=60).contains(&self.poll_seconds)
            || !self.state_dir.is_absolute()
            || !self.core_socket.is_absolute()
            || self.core_socket.as_os_str().len() > 100
        {
            bail!("invalid CyberHerd relay/poll/path configuration");
        }
        Ok(())
    }
    fn policy(&self) -> TargetPolicy {
        TargetPolicy {
            project_pubkey: self.project_pubkey.clone(),
            tracked_tags: self.tracked_tags.clone(),
        }
    }
}

pub struct Runtime {
    store: Store,
    client: Client,
    discovery: Discovery<Reader>,
    config: Config,
    _singleton: std::os::unix::net::UnixListener,
}

impl Runtime {
    pub async fn open(config: Config, config_path: &Path) -> Result<Self> {
        config.validate()?;
        if !config.enabled {
            bail!("CyberHerd worker is disabled in configuration");
        }
        let (uid, _) = self_credentials()?;
        if uid == 0 || uid == config.core.core_uid {
            bail!("CyberHerd requires a separate non-root runtime UID");
        }
        let metadata = std::fs::symlink_metadata(config_path)?;
        if !metadata.is_file() || metadata.uid() != 0 || metadata.mode() & 0o022 != 0 {
            bail!("runtime configuration must be a root-owned protected regular file");
        }
        if config_path.canonicalize()? != config_path {
            bail!("runtime configuration path must be canonical");
        }
        for parent in config_path.ancestors().skip(1) {
            let metadata = std::fs::symlink_metadata(parent)?;
            if !metadata.is_dir() || metadata.uid() != 0 || metadata.mode() & 0o022 != 0 {
                bail!("configuration parents must be root-owned and protected from replacement");
            }
        }
        if Config::load(config_path)? != config {
            bail!("configuration changed during protected startup");
        }
        let metadata = std::fs::symlink_metadata(&config.state_dir)?;
        if !metadata.is_dir()
            || metadata.uid() != uid
            || metadata.mode() & 0o077 != 0
            || config.state_dir.canonicalize()? != config.state_dir
        {
            bail!("CyberHerd state directory must be canonical, runtime-owned and private");
        }
        let singleton = singleton(&config.state_dir, uid)?;
        let database = config.state_dir.join("cyberherd.db");
        let store = Store::connect(&format!(
            "sqlite://{}",
            database.to_str().context("state path must be UTF-8")?
        ))
        .await?;
        std::fs::set_permissions(&database, std::fs::Permissions::from_mode(0o600))?;
        let now = calendar::now()?;
        store
            .initialize(
                &config.project_pubkey,
                &config.settings,
                DayWindow::at(&config.timezone, now)?.start,
            )
            .await?;
        store
            .initialize_core_ingress(&config.project_pubkey, &config.core)
            .await?;
        store
            .seed_templates(
                &config.project_pubkey,
                &super::templates::Catalog::legacy()?,
            )
            .await?;
        let client = Client::new(config.core_socket.clone(), config.core.core_uid)?;
        let discovery =
            Discovery::new(Reader::new(config.relays.clone())?, config.required_relays)?;
        Ok(Self {
            store,
            client,
            discovery,
            config,
            _singleton: singleton,
        })
    }

    pub async fn tick(&mut self) -> Result<Step> {
        let owner = &self.config.project_pubkey;
        let now = calendar::now()?;
        self.store
            .catch_up_calendar(
                owner,
                &self.config.timezone,
                self.config.midnight_reset,
                now,
            )
            .await?;
        draining(worker::poll_core(&self.store, owner, &self.client).await)?;
        if self.availability().await != FeederAvailability::Ready {
            self.store.defer_pending_payments(owner).await?;
        }
        let policy = self.config.policy();
        let day = DayWindow::at(&self.config.timezone, now)?;
        let (ids, pubkey) = self.head_references(now).await?;
        let refreshed = self
            .discovery
            .refresh(&self.store, &policy, &day, &ids, now)
            .await?;
        if refreshed.available {
            let cycle = self.store.snapshot(owner).await?;
            draining(
                self.discovery
                    .backfill(
                        &self.store,
                        owner,
                        &refreshed.targets,
                        cycle.cycle_start,
                        now,
                    )
                    .await,
            )?;
        }
        // Profile lookup follows durable staging. The result is needed only
        // for this head; a slow/missing profile cannot move another input ahead.
        let profile = match pubkey {
            Some(pubkey) => {
                self.discovery
                    .refresh_profile(&self.store, owner, &pubkey, now)
                    .await?
            }
            None => None,
        };
        let applied_at = calendar::now()?;
        if !day.contains(applied_at) {
            return Ok(Step::Waiting);
        }
        // Reproject at the actual processing time so expired streams/provider
        // proofs cannot remain active just because earlier network work was slow.
        let targets = self
            .store
            .target_projection(
                &policy,
                &day,
                &ids,
                std::collections::BTreeSet::new(),
                applied_at,
            )
            .await?;
        let feeder = self.availability().await;
        if feeder != FeederAvailability::Ready {
            self.store.defer_pending_payments(owner).await?;
        }
        worker::step(
            &self.store,
            owner,
            Environment {
                targets: &targets,
                discovery_available: refreshed.available,
                feeder,
                profile: profile.as_ref(),
                now: applied_at,
            },
        )
        .await
    }

    async fn availability(&self) -> FeederAvailability {
        match self.client.safety().await {
            Ok(state) => match state.current_availability(calendar::now().unwrap_or(-1)) {
                Availability::Ready => FeederAvailability::Ready,
                Availability::Paused => FeederAvailability::Paused,
                Availability::Unknown => FeederAvailability::Unknown,
            },
            Err(_) => FeederAvailability::Unknown,
        }
    }

    async fn head_references(&self, now: i64) -> Result<(Vec<String>, Option<String>)> {
        let owner = &self.config.project_pubkey;
        let Some(head) = self.store.ingress_head(owner).await? else {
            return Ok((vec![], None));
        };
        match head.input {
            Input::Social(id) => {
                let event = self
                    .store
                    .observed_event(owner, &id)
                    .await?
                    .context("missing staged social event")?;
                let social = SocialEvent::parse(event, owner, now)?
                    .context("invalid staged social input")?;
                Ok((
                    social.event_id().map(str::to_owned).into_iter().collect(),
                    Some(social.event().event().pubkey.clone()),
                ))
            }
            Input::Payment(payment) => {
                let Some(zap) = &payment.zap else {
                    return Ok((vec![], None));
                };
                let request = ZapRequest::validate(
                    &zap.request_json,
                    &zap.payee_pubkey,
                    &zap.receipt_pubkey,
                    payment.amount_msat,
                )?;
                Ok((
                    request
                        .event()
                        .singleton_tag("e", false)?
                        .map(str::to_owned)
                        .into_iter()
                        .collect(),
                    Some(request.event().event().pubkey.clone()),
                ))
            }
            Input::Feeding(_) => Ok((vec![], None)),
        }
    }

    pub async fn run(mut self) -> Result<()> {
        let mut ticks = tokio::time::interval(Duration::from_secs(self.config.poll_seconds));
        ticks.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tokio::select! {
                _=tokio::signal::ctrl_c()=>return Ok(()),
                _=ticks.tick()=>{
                    if let Err(error)=self.tick().await {
                        // Do not print private core proof/request/source fields.
                        tracing::warn!(error_type=%error.root_cause(),"CyberHerd intake held; retained cursor/work for retry");
                    }
                }
            }
        }
    }
}

#[cfg(target_os = "linux")]
fn singleton(state: &Path, uid: u32) -> Result<std::os::unix::net::UnixListener> {
    use std::os::linux::net::SocketAddrExt;
    let name = format!(
        "cyberherdd-{}-{}",
        uid,
        super::ingress::hash(&state.to_string_lossy())?
    );
    let address = std::os::unix::net::SocketAddr::from_abstract_name(name.as_bytes())?;
    // Kernel lifetime releases this lock after a crash; no stale PID file or
    // destructive stale-socket replacement. It carries no command protocol.
    std::os::unix::net::UnixListener::bind_addr(&address)
        .context("CyberHerd state is already owned by another worker")
}

#[cfg(not(target_os = "linux"))]
fn singleton(_state: &Path, _uid: u32) -> Result<std::os::unix::net::UnixListener> {
    bail!("CyberHerd runtime singleton currently requires Linux")
}

fn draining<T>(result: Result<T>) -> Result<()> {
    match result {
        Ok(_) => Ok(()),
        Err(error) if error.is::<super::ingress::IngressFull>() => Ok(()),
        Err(error) => Err(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_typed_backpressure_allows_existing_work_to_drain() {
        assert!(draining::<()>(Err(super::super::ingress::IngressFull.into())).is_ok());
        assert!(draining::<()>(Err(anyhow::anyhow!("epoch changed"))).is_err());
    }

    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn singleton_is_state_and_uid_scoped_and_released_without_stale_files() {
        let root = tempfile::TempDir::new().unwrap();
        let uid = self_credentials().unwrap().0;
        let first = singleton(root.path(), uid).unwrap();
        assert!(singleton(root.path(), uid).is_err());
        let different = singleton(root.path(), uid + 1).unwrap();
        drop(first);
        assert!(singleton(root.path(), uid).is_ok());
        drop(different);
        assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
    }
}
