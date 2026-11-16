//! Existing private key material only: startup never silently generates replacements.
use anyhow::{Result, ensure};
use cdk::nuts::{PublicKey, SecretKey};
use rustix::fs::{Mode, OFlags};
use std::{fs::File, io::Read, os::unix::fs::MetadataExt, path::Path};
use zeroize::Zeroizing;

pub struct Keys {
    pub seed: Zeroizing<[u8; 64]>,
    pub receiving: SecretKey,
}

impl Keys {
    /// Files contain raw bytes, not hex, JSON, environment variables or CLI secrets.
    pub fn load(directory: &Path, expected_receiver: PublicKey) -> Result<Self> {
        crate::private_ipc::private_state(directory)?;
        let seed = read_secret::<64>(&directory.join("wallet-seed.bin"))?;
        let receiving_bytes = read_secret::<32>(&directory.join("receiving-key.bin"))?;
        let receiving = SecretKey::from_slice(receiving_bytes.as_ref())
            .map_err(|_| anyhow::anyhow!("invalid dedicated receiving key"))?;
        ensure!(
            receiving.public_key() == expected_receiver,
            "dedicated receiving key differs from protected configuration"
        );
        Ok(Self { seed, receiving })
    }
}

fn read_secret<const N: usize>(path: &Path) -> Result<Zeroizing<[u8; N]>> {
    // NONBLOCK prevents a substituted FIFO from hanging before the type check.
    let fd = rustix::fs::open(
        path,
        OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NOFOLLOW | OFlags::NONBLOCK,
        Mode::empty(),
    )?;
    let mut file = File::from(fd);
    let metadata = file.metadata()?;
    ensure!(
        metadata.is_file()
            && metadata.nlink() == 1
            && metadata.uid() == rustix::process::geteuid().as_raw()
            && metadata.mode() & 0o077 == 0
            && metadata.len() == N as u64,
        "wallet key file requires private ownership and exact size"
    );
    let mut value = Zeroizing::new([0; N]);
    file.read_exact(value.as_mut())?;
    let mut trailing = Zeroizing::new([0; 1]);
    ensure!(
        file.read(trailing.as_mut())? == 0,
        "wallet key file changed size"
    );
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn write(path: &Path, bytes: &[u8]) -> Result<()> {
        std::fs::write(path, bytes)?;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
        Ok(())
    }

    #[tokio::test]
    async fn loading_preserves_material_and_rejects_missing_mismatched_or_exposed_keys()
    -> Result<()> {
        let dir = crate::test_directory()?;
        let receiver = SecretKey::from_slice(&[3; 32])?.public_key();
        assert!(Keys::load(dir.path(), receiver).is_err());
        assert_eq!(std::fs::read_dir(dir.path())?.count(), 0);
        let seed = dir.path().join("wallet-seed.bin");
        let key = dir.path().join("receiving-key.bin");
        write(&seed, &[7; 64])?;
        write(&key, &[3; 32])?;
        let loaded = Keys::load(dir.path(), receiver)?;
        assert_eq!(*loaded.seed, [7; 64]);
        assert_eq!(loaded.receiving.public_key(), receiver);
        assert!(Keys::load(dir.path(), SecretKey::from_slice(&[4; 32])?.public_key()).is_err());
        std::fs::set_permissions(&key, std::fs::Permissions::from_mode(0o644))?;
        assert!(Keys::load(dir.path(), receiver).is_err());
        write(&key, &[3; 31])?;
        assert!(Keys::load(dir.path(), receiver).is_err());
        write(&key, &[0; 32])?;
        assert!(Keys::load(dir.path(), receiver).is_err());
        write(&key, &[3; 32])?;
        let alias = dir.path().join("alias");
        std::fs::hard_link(&key, &alias)?;
        assert!(Keys::load(dir.path(), receiver).is_err());
        std::fs::remove_file(&alias)?;
        std::fs::rename(&key, &alias)?;
        std::os::unix::fs::symlink(&alias, &key)?;
        assert!(Keys::load(dir.path(), receiver).is_err());
        assert_eq!(std::fs::read(&alias)?, [3; 32]);
        assert_eq!(std::fs::read(&seed)?, [7; 64]);
        Ok(())
    }
}
