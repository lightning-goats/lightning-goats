//! Read-only exact-prefix verification; no migration or activation.
use super::{Manifest, Operator, database, files};
use anyhow::{Result, bail};
use std::path::Path;
impl Operator {
    pub async fn verify_inclusive_fee_upgrade_source(&self, bundle: &Path) -> Result<Manifest> {
        files::private_bundle(bundle)?;
        match std::fs::symlink_metadata(bundle.join("INCOMPLETE")) {
            Ok(_) => bail!("inclusive-fee upgrade input is incomplete"),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
        }
        let raw = files::read_private(&bundle.join("MANIFEST.json"), 65536)?;
        let manifest: Manifest = serde_json::from_slice(&raw)
            .map_err(|_| anyhow::anyhow!("invalid private source manifest"))?;
        if manifest.version != 1
            || manifest.id.is_nil()
            || manifest.created_at < 0
            || manifest.policy != self.policy
            || manifest.source_policy_sha256 != self.policy_hash
            || !manifest.recovery_required
        {
            bail!("inclusive-fee upgrade source scope changed");
        }
        files::check_images(bundle, &manifest.images, self.policy.max_component_bytes)?;
        if database::inspect_before_inclusive_fees(bundle, &self.policy).await? != manifest.report {
            bail!("inclusive-fee upgrade source report changed");
        }
        files::check_images(bundle, &manifest.images, self.policy.max_component_bytes)?;
        Ok(manifest)
    }
}
