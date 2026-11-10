//! Pre-CyberHerd core upgrade evidence. No restore, migration or service activation.
use super::{Component, Image, Role, control::Control, files};
use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::Path};

mod rows;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Policy {
    pub version: u8,
    pub core: Component,
    pub max_component_bytes: u64,
}
impl Policy {
    pub fn load_for_check(path: &Path) -> Result<Self> {
        use std::io::Read as _;
        let mut bytes = Vec::new();
        std::fs::File::open(path)?
            .take(65537)
            .read_to_end(&mut bytes)?;
        if bytes.len() > 65536 {
            bail!("legacy core policy exceeds bound");
        }
        let value: Self = toml::from_str(std::str::from_utf8(&bytes)?)
            .map_err(|_| anyhow::anyhow!("invalid legacy core policy fields"))?;
        super::validate_component(&value.core)?;
        if value.version != 1
            || !(1024..=64 * 1024 * 1024 * 1024).contains(&value.max_component_bytes)
        {
            bail!("invalid legacy core policy");
        }
        Ok(value)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Table {
    pub columns: Vec<String>,
    pub rows: u64,
    pub sha256: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Report {
    pub engine: String,
    pub epoch: uuid::Uuid,
    pub migrations: Vec<i64>,
    pub tables: BTreeMap<String, Table>,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub version: u8,
    pub kind: String,
    pub id: uuid::Uuid,
    pub policy: Policy,
    pub policy_sha256: String,
    pub report: Report,
    images: Vec<Image>,
}
pub struct Operator {
    policy: Policy,
    policy_hash: String,
    control: Control,
}
impl Operator {
    pub fn load(path: &Path) -> Result<Self> {
        files::require_root()?;
        crate::private_ipc::protected_config(path)?;
        Ok(Self {
            policy: Policy::load_for_check(path)?,
            policy_hash: files::hash_bytes(&std::fs::read(path)?),
            control: Control::Systemd,
        })
    }
    pub async fn snapshot(&self, destination: &Path) -> Result<Manifest> {
        self.control.check_component(&self.policy.core).await?;
        let mut source = files::Source::open(&self.policy.core, self.policy.max_component_bytes)?;
        files::new_private_directory(destination)?;
        files::write_new(
            &destination.join("INCOMPLETE"),
            b"Unaccepted legacy core backup.\n",
        )?;
        let images = source.copy(destination, Role::Core)?;
        self.control.check_component(&self.policy.core).await?;
        source.unchanged()?;
        let report = rows::inspect(destination).await?;
        files::check_core_images(destination, &images, self.policy.max_component_bytes)?;
        self.control.check_component(&self.policy.core).await?;
        source.unchanged()?;
        let manifest = Manifest {
            version: 1,
            kind: "legacy_core_upgrade".into(),
            id: uuid::Uuid::new_v4(),
            policy: self.policy.clone(),
            policy_sha256: self.policy_hash.clone(),
            report,
            images,
        };
        let encoded = serde_json::to_vec_pretty(&manifest)?;
        if encoded.len() > 262144 {
            bail!("legacy core manifest exceeds verification bound");
        }
        files::write_new(&destination.join("MANIFEST.json"), &encoded)?;
        files::sync_directory(destination)?;
        std::fs::remove_file(destination.join("INCOMPLETE"))?;
        files::sync_directory(destination)?;
        Ok(manifest)
    }
    pub async fn verify(&self, bundle: &Path) -> Result<Manifest> {
        files::private_bundle(bundle)?;
        match std::fs::symlink_metadata(bundle.join("INCOMPLETE")) {
            Ok(_) => bail!("legacy core bundle is incomplete"),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
        }
        let raw = files::read_private(&bundle.join("MANIFEST.json"), 262144)?;
        let manifest: Manifest = serde_json::from_slice(&raw)
            .map_err(|_| anyhow::anyhow!("invalid legacy core manifest"))?;
        if manifest.version != 1
            || manifest.kind != "legacy_core_upgrade"
            || manifest.id.is_nil()
            || manifest.policy != self.policy
            || manifest.policy_sha256 != self.policy_hash
        {
            bail!("legacy core scope or policy changed");
        }
        files::check_core_images(bundle, &manifest.images, self.policy.max_component_bytes)?;
        if rows::inspect(bundle).await? != manifest.report {
            bail!("legacy core report changed");
        }
        files::check_core_images(bundle, &manifest.images, self.policy.max_component_bytes)?;
        Ok(manifest)
    }
    /// Only verifies a separate post-migration copy. Never opens/migrates the
    /// original journal or authorizes adopting a restored/rolled-back state.
    pub async fn compare_upgrade(&self, bundle: &Path, candidate: &Path) -> Result<Manifest> {
        let manifest = self.verify(bundle).await?;
        files::private_bundle(candidate)?;
        match std::fs::symlink_metadata(candidate.join("INCOMPLETE")) {
            Ok(_) => bail!("upgrade candidate copy is incomplete"),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
        }
        // Read private images before parsing; no unlisted input type or alias.
        let mut images = Vec::new();
        for name in ["core.db", "core.db-wal"] {
            let path = candidate.join(name);
            match std::fs::symlink_metadata(&path) {
                Ok(_) => images.push(files::private_image(
                    &path,
                    self.policy.max_component_bytes,
                )?),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound && name.ends_with("-wal") => {}
                Err(e) => return Err(e.into()),
            }
        }
        files::check_core_images(candidate, &images, self.policy.max_component_bytes)?;
        rows::compare(candidate, &manifest.report).await?;
        files::check_core_images(candidate, &images, self.policy.max_component_bytes)?;
        Ok(manifest)
    }
}

#[cfg(test)]
mod tests;
