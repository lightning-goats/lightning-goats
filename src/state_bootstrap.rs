//! New, disabled journals only. This is not migration or financial recovery.
use crate::private_ipc as ipc;
use anyhow::{Context, Result, bail};
use std::{
    fs::{File, OpenOptions},
    os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
};

pub(crate) struct NewJournal {
    state: PathBuf,
    scratch: PathBuf,
    final_database: PathBuf,
    _singleton: std::os::unix::net::UnixListener,
}

impl NewJournal {
    pub(crate) fn begin(
        state: &Path,
        name: &str,
        singleton: std::os::unix::net::UnixListener,
    ) -> Result<Self> {
        ipc::private_state(state)?;
        let (_, gid) = crate::core_bridge::unix::self_credentials()?;
        if gid == 0 || std::fs::metadata(state)?.gid() != gid {
            bail!("initialization requires the runtime's own non-root primary group");
        }
        for parent in state.ancestors().skip(1) {
            let m = std::fs::symlink_metadata(parent)?;
            if !m.is_dir() || m.uid() != 0 || m.mode() & 0o022 != 0 {
                bail!("initialization requires protected root-owned state ancestors");
            }
        }
        if !matches!(name, "cyberherd.db" | "payout.db" | "signer.db") {
            bail!("unsupported new journal");
        }
        let final_database = state.join(name);
        absent_final(&final_database)?;
        let scratch = state.join(format!(".initialize-{}", uuid::Uuid::new_v4()));
        std::fs::DirBuilder::new().mode(0o700).create(&scratch)?;
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(scratch.join("initial.db"))?
            .sync_all()?;
        Ok(Self {
            state: state.to_owned(),
            scratch,
            final_database,
            _singleton: singleton,
        })
    }
    pub(crate) fn url(&self) -> Result<String> {
        Ok(format!(
            "sqlite://{}",
            self.scratch
                .join("initial.db")
                .to_str()
                .context("initial state path must be UTF-8")?
        ))
    }
    /// Call only after the new store's initialization and connections close.
    /// VACUUM INTO freezes all committed WAL pages before exclusive publication.
    pub(crate) async fn publish(self) -> Result<()> {
        let image = self.scratch.join("complete.db");
        let pool = crate::sqlite::connect_existing_durable(&self.url()?, 1).await?;
        let result: Result<()> = async {
            let engine: String = sqlx::query_scalar("SELECT sqlite_version()")
                .fetch_one(&pool)
                .await?;
            if engine != "3.54.1" {
                bail!("initialization requires the qualified native SQLite engine");
            }
            sqlx::query("VACUUM INTO ?")
                .bind(image.to_str().context("initial image path must be UTF-8")?)
                .execute(&pool)
                .await?;
            Ok(())
        }
        .await;
        pool.close().await;
        result?;
        std::fs::set_permissions(&image, std::fs::Permissions::from_mode(0o600))?;
        let m = std::fs::symlink_metadata(&image)?;
        if !m.is_file() || m.uid() != ipc::uid()? || m.nlink() != 1 || m.len() == 0 {
            bail!("new initial image identity changed");
        }
        File::open(&image)?.sync_all()?;
        ipc::private_state(&self.state)?;
        absent_final(&self.final_database)?;
        // Hard-link publication is atomic and fails if ANY destination exists,
        // including an empty file or dangling symlink. No rename overwrite.
        std::fs::hard_link(&image, &self.final_database)?;
        File::open(&self.state)?.sync_all()?;
        std::fs::remove_file(&image)?;
        File::open(&self.state)?.sync_all()?;
        if std::fs::remove_dir_all(&self.scratch).is_err() {
            tracing::warn!("initialization completed; private scratch cleanup remains");
        }
        Ok(())
    }
}

fn absent_final(database: &Path) -> Result<()> {
    let parent = database.parent().context("new database parent missing")?;
    let mut paths = vec![database.to_owned()];
    for suffix in ["-wal", "-shm", "-journal"] {
        let mut path = database.as_os_str().to_owned();
        path.push(suffix);
        paths.push(PathBuf::from(path));
    }
    paths.push(parent.join(crate::state_maintenance::RECOVERY_MARKER));
    for path in paths {
        match std::fs::symlink_metadata(path) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
            Ok(_) => {
                bail!("initialization refuses existing journals, sidecars or recovery markers")
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
