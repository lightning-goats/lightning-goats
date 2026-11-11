//! Offline four-journal operations. Never sends payments, signs or starts services.
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};
use uuid::Uuid;

mod contact_upgrade;
mod control;
mod database;
mod files;
pub mod legacy;

pub use database::Report;

#[cfg(test)]
pub(crate) async fn inspect_fixture(directory: &Path, policy: &Policy) -> Result<Report> {
    policy.validate()?;
    database::inspect(directory, policy).await
}

pub const RECOVERY_MARKER: &str = "CYBERHERD_RECOVERY_REQUIRED";

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Core,
    Membership,
    Payout,
    Signer,
}
impl Role {
    const ALL: [Self; 4] = [Self::Core, Self::Membership, Self::Payout, Self::Signer];
    fn name(self) -> &'static str {
        match self {
            Self::Core => "core",
            Self::Membership => "membership",
            Self::Payout => "payout",
            Self::Signer => "signer",
        }
    }
    fn file(self) -> String {
        format!("{}.db", self.name())
    }
    fn restore_file(self) -> &'static str {
        match self {
            Self::Core => "lightning-goats.db",
            Self::Membership => "cyberherd.db",
            Self::Payout => "payout.db",
            Self::Signer => "signer.db",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Component {
    pub database: PathBuf,
    pub uid: u32,
    pub gid: u32,
    pub unit: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Policy {
    pub version: u8,
    pub owner: String,
    pub epoch: Uuid,
    pub provider: crate::payout::funding::ProviderScope,
    pub max_component_bytes: u64,
    pub components: BTreeMap<Role, Component>,
}
impl Policy {
    pub fn load_for_check(path: &Path) -> Result<Self> {
        use std::io::Read as _;
        let mut raw = String::new();
        std::fs::File::open(path)?
            .take(65537)
            .read_to_string(&mut raw)?;
        if raw.len() > 65536 {
            bail!("maintenance policy exceeds bound");
        }
        let value: Self = toml::from_str(&raw)
            .map_err(|_| anyhow::anyhow!("invalid maintenance policy fields"))?;
        value.validate()?;
        Ok(value)
    }
    fn validate(&self) -> Result<()> {
        crate::nostr_event::validate_public_key(&self.owner)?;
        crate::nostr_event::validate_hex(&self.provider.credential_fingerprint, 32)?;
        if self.version != 1
            || self.epoch.is_nil()
            || self.provider.account_id.is_nil()
            || self.provider.environment != "strike-production"
            || self.components.len() != 4
            || !(1024..=64 * 1024 * 1024 * 1024).contains(&self.max_component_bytes)
        {
            bail!("invalid maintenance scope or byte limit");
        }
        let mut uids = std::collections::BTreeSet::new();
        let mut paths = std::collections::BTreeSet::new();
        let mut units = std::collections::BTreeSet::new();
        for role in Role::ALL {
            let c = self
                .components
                .get(&role)
                .context("all four journals are required")?;
            validate_component(c)?;
            if !uids.insert(c.uid) || !paths.insert(&c.database) || !units.insert(&c.unit) {
                bail!("maintenance requires distinct non-admin owners, paths and units");
            }
        }
        Ok(())
    }
}

fn validate_component(c: &Component) -> Result<()> {
    files::absolute(&c.database)?;
    if c.uid == 0
        || c.gid == 0
        || !c.unit.ends_with(".service")
        || c.unit.len() > 128
        || !c.unit.starts_with(|c: char| c.is_ascii_alphanumeric())
        || !c
            .unit
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"@_.-".contains(&c))
    {
        bail!("invalid non-admin journal owner, path or service");
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct Image {
    name: String,
    bytes: u64,
    sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub version: u8,
    pub id: Uuid,
    pub created_at: i64,
    pub policy: Policy,
    pub source_policy_sha256: String,
    pub recovery_required: bool,
    images: Vec<Image>,
    pub report: Report,
}

/// Constructed only by root with a canonical protected policy. No runtime/RPC
/// caller can gain financial or service-start capability from this object.
pub struct Operator {
    policy: Policy,
    policy_hash: String,
    control: control::Control,
}
impl Operator {
    pub fn load(path: &Path) -> Result<Self> {
        files::require_root()?;
        crate::private_ipc::protected_config(path)?;
        let policy = Policy::load_for_check(path)?;
        Ok(Self {
            policy_hash: files::hash_bytes(&std::fs::read(path)?),
            policy,
            control: control::Control::Systemd,
        })
    }
    pub async fn snapshot(&self, destination: &Path) -> Result<Manifest> {
        self.control.check(&self.policy).await?;
        // Open and validate every source before creating the new output directory.
        let mut sources = Vec::new();
        for role in Role::ALL {
            let c = &self.policy.components[&role];
            sources.push((
                role,
                files::Source::open(c, self.policy.max_component_bytes)?,
            ));
        }
        files::new_private_directory(destination)?;
        files::write_new(
            &destination.join("INCOMPLETE"),
            b"Not an accepted backup.\n",
        )?;
        let mut images = Vec::new();
        for (role, source) in &mut sources {
            self.control.check(&self.policy).await?;
            images.extend(source.copy(destination, *role)?);
        }
        self.control.check(&self.policy).await?;
        for (_, source) in &sources {
            source.unchanged()?;
        }
        let report = database::inspect(destination, &self.policy).await?;
        files::check_images(destination, &images, self.policy.max_component_bytes)?;
        self.control.check(&self.policy).await?;
        for (_, source) in &sources {
            source.unchanged()?;
        }
        let manifest = Manifest {
            version: 1,
            id: Uuid::new_v4(),
            created_at: crate::cyberherd::calendar::now()?,
            policy: self.policy.clone(),
            source_policy_sha256: self.policy_hash.clone(),
            recovery_required: true,
            images,
            report,
        };
        files::write_new(
            &destination.join("MANIFEST.json"),
            &serde_json::to_vec_pretty(&manifest)?,
        )?;
        files::sync_directory(destination)?;
        std::fs::remove_file(destination.join("INCOMPLETE"))?;
        files::sync_directory(destination)?;
        Ok(manifest)
    }
    /// Read an accepted private bundle, recheck every byte and native database,
    /// and return a private report. It is never permission to resume transfers.
    pub async fn verify(&self, bundle: &Path) -> Result<Manifest> {
        files::private_bundle(bundle)?;
        match std::fs::symlink_metadata(bundle.join("INCOMPLETE")) {
            Ok(_) => bail!("bundle is incomplete"),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        let raw = files::read_private(&bundle.join("MANIFEST.json"), 65536)?;
        let manifest: Manifest = serde_json::from_slice(&raw)
            .map_err(|_| anyhow::anyhow!("invalid private bundle manifest"))?;
        if manifest.version != 1
            || manifest.id.is_nil()
            || manifest.created_at < 0
            || manifest.policy != self.policy
            || manifest.source_policy_sha256 != self.policy_hash
            || !manifest.recovery_required
        {
            bail!("bundle scope or recovery boundary changed");
        }
        files::check_images(bundle, &manifest.images, self.policy.max_component_bytes)?;
        let report = database::inspect(bundle, &self.policy).await?;
        if report != manifest.report {
            bail!("bundle report differs from retained images");
        }
        files::check_images(bundle, &manifest.images, self.policy.max_component_bytes)?;
        Ok(manifest)
    }
    /// Stage all four journals together in NEW directories. Does not overwrite,
    /// rebind an epoch, change configuration, install files or start a service.
    pub async fn stage_restore(&self, bundle: &Path, destination: &Path) -> Result<Manifest> {
        self.control.check(&self.policy).await?;
        let manifest = self.verify(bundle).await?;
        files::new_private_directory(destination)?;
        files::write_new(
            &destination.join("INCOMPLETE"),
            b"Restore staging is incomplete.\n",
        )?;
        for role in Role::ALL {
            let directory = destination.join(role.name());
            files::new_private_directory(&directory)?;
            // The marker is created before a database can become startable.
            files::write_new(&directory.join(RECOVERY_MARKER),
                b"External provider/gateway reconciliation and explicit operator activation are required.\n")?;
            for image in manifest
                .images
                .iter()
                .filter(|i| i.name == role.file() || i.name == format!("{}-wal", role.file()))
            {
                let suffix = if image.name.ends_with("-wal") {
                    "-wal"
                } else {
                    ""
                };
                files::copy_private(
                    &bundle.join(&image.name),
                    &directory.join(format!("{}{suffix}", role.restore_file())),
                    image,
                )?;
            }
            let c = &self.policy.components[&role];
            for entry in std::fs::read_dir(&directory)? {
                let path = entry?.path();
                if path.file_name() != Some(std::ffi::OsStr::new(RECOVERY_MARKER)) {
                    std::os::unix::fs::chown(path, Some(c.uid), Some(c.gid))?;
                }
            }
            files::sync_directory(&directory)?;
            std::os::unix::fs::chown(&directory, Some(c.uid), Some(c.gid))?;
        }
        files::write_new(
            &destination.join("RESTORE.json"),
            &serde_json::to_vec_pretty(&manifest)?,
        )?;
        self.control.check(&self.policy).await?;
        self.verify(bundle).await?;
        files::sync_directory(destination)?;
        std::fs::remove_file(destination.join("INCOMPLETE"))?;
        files::sync_directory(destination)?;
        Ok(manifest)
    }
}

#[cfg(test)]
mod tests;
