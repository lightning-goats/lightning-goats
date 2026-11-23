//! Offline member-UID child. The root parent owns four-journal verification,
//! stopped-writer guards, fresh checkpoint, rehearsal and append-only comparison.
use super::{Config, Store, singleton};
use crate::{cyberherd::reward_cutover::Receipt, state_maintenance::RewardCutoverPreview};
use anyhow::{Result, ensure};
use sha2::{Digest as _, Sha256};
use std::{io::Read as _, os::unix::fs::MetadataExt as _, path::Path};

fn read_preview(path: &Path, expected: &str) -> Result<RewardCutoverPreview> {
    crate::private_ipc::protected_config(path)?;
    crate::nostr_event::validate_hex(expected, 32)?;
    let file = std::fs::File::open(path)?;
    let meta = file.metadata()?;
    const MAX: usize = 4 * 1024 * 1024 + 65536;
    ensure!(
        meta.mode() & 0o007 == 0 && meta.nlink() == 1 && meta.len() <= MAX as u64,
        "cutover preview must be private, bounded and singly linked"
    );
    let mut bytes = Vec::new();
    file.take(MAX as u64 + 1).read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() <= MAX && hex::encode(Sha256::digest(&bytes)) == expected,
        "cutover preview differs from reviewed digest"
    );
    let value: RewardCutoverPreview = serde_json::from_slice(&bytes)
        .map_err(|_| anyhow::anyhow!("invalid protected cutoff preview"))?;
    value.decision.validate()?;
    crate::nostr_event::validate_hex(&value.source_policy_sha256, 32)?;
    ensure!(
        value.version == 1
            && value.id == value.decision.id
            && !value.bundle_id.is_nil()
            && !value.cutover_applied
            && !value.activation_authorized
            && value.decision.decided_at <= crate::cyberherd::calendar::now()?,
        "cutover preview scope or authority differs"
    );
    Ok(value)
}

impl Config {
    /// No core/payout/signer database, socket, credential or network is opened.
    /// The caller must separately prove the complete fresh core inventory and
    /// stopped-writer checkpoint. This child grants no activation permission.
    pub async fn record_initial_reward_cutover(
        &self,
        config_path: &Path,
        preview_path: &Path,
        expected_sha256: &str,
        writers_stopped: bool,
    ) -> Result<Receipt> {
        self.validate()?;
        crate::private_ipc::protected_config(config_path)?;
        let uid = crate::private_ipc::uid()?;
        ensure!(
            writers_stopped
                && uid != 0
                && uid != self.core.core_uid
                && self.rewards.is_none()
                && *self == Config::load(config_path)?,
            "cutover requires unchanged rewards-disabled configuration, stopped writers and separate member UID"
        );
        crate::private_ipc::private_state(&self.state_dir)?;
        let _lock = singleton(&self.state_dir, uid)?;
        let preview = read_preview(preview_path, expected_sha256)?;
        ensure!(
            preview.owner == self.project_pubkey && preview.decision.epoch == self.core.epoch,
            "cutover belongs to another project or core epoch"
        );
        let url = format!("sqlite://{}", self.state_dir.join("cyberherd.db").display());
        let store = Store::open_existing_for_cutover(&url).await?;
        let result: Result<Receipt> = async {
            let cursor = store.core_cursor(&self.project_pubkey).await?;
            ensure!(
                cursor.binding == self.core,
                "cutover core binding differs from retained journal"
            );
            ensure!(
                *self == Config::load(config_path)?
                    && read_preview(preview_path, expected_sha256)? == preview,
                "cutover configuration or preview changed"
            );
            store
                .record_initial_reward_cutover(&self.project_pubkey, &preview.decision)
                .await
        }
        .await;
        store.close_after_initialization().await;
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt as _;

    #[tokio::test]
    async fn offline_open_requires_existing_private_current_journal_and_honors_recovery_fence() {
        if crate::private_ipc::uid().unwrap() == 0 {
            return; // This fixture specifically exercises non-root journal ownership.
        }
        let root = tempfile::tempdir().unwrap();
        std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let path = root.path().join("cyberherd.db");
        let url = format!("sqlite://{}", path.display());
        assert!(Store::open_existing_for_cutover(&url).await.is_err());
        assert!(!path.exists());
        let seed = Store::connect(&url).await.unwrap();
        seed.close_after_initialization().await;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        let store = Store::open_existing_for_cutover(&url).await.unwrap();
        store.close_after_initialization().await;
        std::fs::write(
            root.path().join(crate::state_maintenance::RECOVERY_MARKER),
            b"held",
        )
        .unwrap();
        assert!(Store::open_existing_for_cutover(&url).await.is_err());
        std::fs::remove_file(root.path().join(crate::state_maintenance::RECOVERY_MARKER)).unwrap();
        let pool = crate::sqlite::connect_existing_durable(&url, 1)
            .await
            .unwrap();
        sqlx::query("DELETE FROM _sqlx_migrations WHERE version=(SELECT MAX(version) FROM _sqlx_migrations)")
            .execute(&pool).await.unwrap();
        let remaining: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM _sqlx_migrations")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert!(Store::open_existing_for_cutover(&url).await.is_err());
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM _sqlx_migrations")
                .fetch_one(&pool)
                .await
                .unwrap(),
            remaining
        );
        pool.close().await;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(Store::open_existing_for_cutover(&url).await.is_err());
    }

    #[test]
    fn offline_preview_rejects_unprotected_input_without_reading_its_contents() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("preview.json");
        std::fs::write(&path, b"private-invalid-marker").unwrap();
        let error = read_preview(&path, &"ab".repeat(32)).unwrap_err();
        assert!(!format!("{error:#}").contains("private-invalid-marker"));
    }
}
