//! One process owns a protected wallet state directory. No lease expires on timeout.
use anyhow::{Result, ensure};
use rustix::fs::{FlockOperation, Mode, OFlags};
use std::{
    fs::File,
    os::unix::fs::{MetadataExt, PermissionsExt},
    path::Path,
};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Authority {
    Acquisition(u64),
    Treasury,
    TreasuryRecovery,
    Split,
    SplitPlanning,
    SplitRecovery,
}
#[derive(Clone, Copy)]
pub(crate) struct Operation {
    pub id: uuid::Uuid,
    pub authority: Authority,
}
pub(crate) type ActiveOperation = std::sync::Arc<std::sync::Mutex<Option<Operation>>>;

pub struct WriterLock {
    _file: File,
    active: ActiveOperation,
}
pub(crate) fn private_state_file(path: &Path) -> Result<File> {
    let directory = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("missing wallet directory"))?;
    let owner = rustix::process::geteuid().as_raw();
    let metadata = std::fs::symlink_metadata(directory)?;
    ensure!(
        metadata.is_dir()
            && !metadata.file_type().is_symlink()
            && metadata.uid() == owner
            && metadata.permissions().mode() & 0o077 == 0,
        "wallet directory must be private and owned by runtime UID"
    );
    let fd = rustix::fs::open(
        path,
        OFlags::CREATE | OFlags::RDWR | OFlags::CLOEXEC | OFlags::NOFOLLOW,
        Mode::RUSR | Mode::WUSR,
    )?;
    let file = File::from(fd);
    let metadata = file.metadata()?;
    ensure!(
        metadata.is_file()
            && metadata.nlink() == 1
            && metadata.uid() == owner
            && metadata.permissions().mode() & 0o077 == 0,
        "unsafe wallet state file"
    );
    Ok(file)
}

impl WriterLock {
    pub(crate) fn coordinator(&self) -> ActiveOperation {
        self.active.clone()
    }
    pub fn acquire(directory: &Path) -> Result<Self> {
        // Restore tooling leaves this marker until external state and both
        // ledger epochs are reconciled. Even a dangling alias blocks startup.
        match std::fs::symlink_metadata(directory.join("CASHU_RECOVERY_REQUIRED")) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
            Ok(_) => anyhow::bail!("Cashu recovery requires explicit reconciliation"),
        }
        let file = private_state_file(&directory.join("wallet-writer.lock"))?;
        rustix::fs::flock(&file, FlockOperation::NonBlockingLockExclusive)
            .map_err(|_| anyhow::anyhow!("wallet writer already active"))?;
        Ok(Self {
            _file: file,
            active: std::sync::Arc::new(std::sync::Mutex::new(None)),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn concurrent_writer_is_refused_until_owner_drops() -> Result<()> {
        let dir = crate::test_directory()?;
        let first = WriterLock::acquire(dir.path())?;
        assert!(WriterLock::acquire(dir.path()).is_err());
        drop(first);
        let _second = WriterLock::acquire(dir.path())?;
        Ok(())
    }
    #[test]
    fn recovery_marker_blocks_writer_without_creating_or_changing_state() -> Result<()> {
        let dir = crate::test_directory()?;
        let database = dir.path().join("wallet.sqlite");
        std::fs::write(&database, b"retained custody evidence")?;
        let marker = dir.path().join("CASHU_RECOVERY_REQUIRED");
        std::fs::write(&marker, b"reconcile original operations and ledger epochs")?;
        assert!(WriterLock::acquire(dir.path()).is_err());
        assert!(!dir.path().join("wallet-writer.lock").exists());
        assert_eq!(std::fs::read(&database)?, b"retained custody evidence");
        // These are disposable fixtures, not authorization to clear a real fence.
        std::fs::remove_file(&marker)?;
        std::os::unix::fs::symlink(dir.path().join("absent"), &marker)?;
        assert!(!marker.exists());
        assert!(WriterLock::acquire(dir.path()).is_err());
        assert!(!dir.path().join("wallet-writer.lock").exists());
        assert_eq!(std::fs::read(&database)?, b"retained custody evidence");
        Ok(())
    }
    #[test]
    fn broad_directory_or_symlink_lock_is_refused() -> Result<()> {
        let dir = crate::test_directory()?;
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o755))?;
        assert!(WriterLock::acquire(dir.path()).is_err());
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700))?;
        let target = dir.path().join("other");
        std::fs::write(&target, b"unchanged")?;
        std::os::unix::fs::symlink(&target, dir.path().join("wallet-writer.lock"))?;
        assert!(WriterLock::acquire(dir.path()).is_err());
        assert_eq!(std::fs::read(target)?, b"unchanged");
        Ok(())
    }
}
