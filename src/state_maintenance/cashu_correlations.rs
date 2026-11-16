//! Read-only bridge between native four-journal checks and a Cashu recovery set.
//! This distinct input/report cannot enter ordinary backup or restore staging.
use super::{Image, Operator, Policy, Report, Role, database, files};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::path::Path;
use uuid::Uuid;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    version: u8,
    purpose: String,
    id: Uuid,
    policy: Policy,
    source_policy_sha256: String,
    cashu_inventory_sha256: String,
    images: Vec<Image>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CashuCorrelationReview {
    pub version: u8,
    pub id: Uuid,
    pub input_manifest_sha256: String,
    pub policy_sha256: String,
    pub cashu_inventory_sha256: String,
    pub core_image_sha256: String,
    pub four_journal_correlations: Report,
    pub cashu_wallet_verified: bool,
    pub combined_restore_authorized: bool,
    pub resume_authorized: bool,
}

impl Operator {
    /// Verify complete, frozen main-only copies of the existing four journals.
    /// The linked Cashu inventory is a digest binding, not a verification claim.
    /// A combined verifier must verify that inventory and the identical core bytes.
    pub async fn review_cashu_correlations(
        &self,
        bundle: &Path,
        destination: &Path,
    ) -> Result<CashuCorrelationReview> {
        files::private_bundle(bundle)?;
        files::absolute(destination)?;
        ensure!(
            !destination.starts_with(bundle),
            "correlation report must be outside its input bundle"
        );
        let parent = destination.parent().context("report parent missing")?;
        files::private_bundle(parent)?;
        match std::fs::symlink_metadata(bundle.join("INCOMPLETE")) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
            Ok(_) => anyhow::bail!("correlation input is incomplete"),
        }
        let manifest_path = bundle.join("MANIFEST.json");
        let raw = files::read_private(&manifest_path, 65536)?;
        let input: Input = serde_json::from_slice(&raw)
            .map_err(|_| anyhow::anyhow!("invalid Cashu correlation input"))?;
        ensure!(
            input.version == 2
                && input.purpose == "cashu_four_journal_correlation_review"
                && !input.id.is_nil()
                && input.policy == self.policy
                && input.source_policy_sha256 == self.policy_hash,
            "Cashu correlation input scope changed"
        );
        crate::nostr_event::validate_hex(&input.cashu_inventory_sha256, 32)?;
        // The Cashu inventory currently accepts frozen main images, not WAL sets.
        // Requiring the same here makes a single core-image digest meaningful.
        ensure!(
            input.images.len() == Role::ALL.len()
                && input
                    .images
                    .iter()
                    .all(|image| Role::ALL.iter().any(|role| image.name == role.file())),
            "correlation review requires all four main-only journal images"
        );
        files::check_images(bundle, &input.images, self.policy.max_component_bytes)?;
        let correlations = database::inspect_cashu_correlations(bundle, &self.policy).await?;
        ensure!(
            !correlations.resume_authorized,
            "correlation review cannot authorize resumption"
        );
        files::check_images(bundle, &input.images, self.policy.max_component_bytes)?;
        ensure!(
            files::read_private(&manifest_path, 65536)? == raw,
            "correlation manifest changed during review"
        );
        let core = input
            .images
            .iter()
            .find(|image| image.name == Role::Core.file())
            .context("core correlation image missing")?;
        let review = CashuCorrelationReview {
            version: 1,
            id: input.id,
            input_manifest_sha256: files::hash_bytes(&raw),
            policy_sha256: self.policy_hash.clone(),
            cashu_inventory_sha256: input.cashu_inventory_sha256,
            core_image_sha256: core.sha256.clone(),
            four_journal_correlations: correlations,
            cashu_wallet_verified: false,
            combined_restore_authorized: false,
            resume_authorized: false,
        };
        files::write_new(destination, &serde_json::to_vec_pretty(&review)?)?;
        files::sync_directory(parent)?;
        Ok(review)
    }
}

#[cfg(test)]
mod tests;
