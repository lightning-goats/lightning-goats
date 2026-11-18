//! Standalone purpose-limited broker startup; no implicit initialization or key generation.
use crate::{
    publication_broker::{Broker, Policy},
    publication_nak::{self, Nak},
};
use anyhow::{Result, ensure};
use rustix::fs::{self as posix, Mode, OFlags, StatVfsMountFlags};
use serde::Deserialize;
use std::{
    fs::File,
    io::Read,
    os::{fd::AsFd, unix::fs::MetadataExt},
    path::{Path, PathBuf},
};
use zeroize::Zeroizing;

const MAX_CONFIG: usize = 65_536;
const CLIENT_CREDENTIAL: &str = "cashu-nostr-client";
const SYSTEMD_CREDENTIALS: &str = "/run/credentials";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub version: u8,
    pub runtime_uid: u32,
    pub ipc_gid: u32,
    pub state_directory: PathBuf,
    pub socket: PathBuf,
    pub policy: Policy,
    pub nak: publication_nak::Config,
}
impl Config {
    pub fn load(path: &Path) -> Result<Self> {
        crate::private_ipc::protected_config(path)?;
        let mut bytes = Vec::new();
        File::open(path)?
            .take((MAX_CONFIG + 1) as u64)
            .read_to_end(&mut bytes)?;
        Self::decode(&bytes)
    }
    pub fn decode(bytes: &[u8]) -> Result<Self> {
        ensure!(
            !bytes.is_empty() && bytes.len() <= MAX_CONFIG,
            "signer configuration exceeds bound"
        );
        let value: Self = serde_json::from_slice(bytes)
            .map_err(|_| anyhow::anyhow!("invalid signer configuration"))?;
        value.validate()?;
        Ok(value)
    }
    pub fn validate(&self) -> Result<()> {
        self.policy.validate()?;
        self.nak.validate()?;
        crate::private_ipc::socket_path(&self.socket)?;
        ensure!(
            self.version == 1
                && self.runtime_uid > 0
                && self.runtime_uid != self.policy.wallet_uid
                && self.ipc_gid > 0
                && self.nak.owner == self.policy.owner
                && self.policy.receiving_key != self.policy.owner
                && self.nak.client_pubkey != self.policy.receiving_key,
            "invalid signer service registration"
        );
        ensure!(
            self.state_directory.is_absolute()
                && self.state_directory != Path::new("/")
                && self.state_directory.components().all(|c| matches!(
                    c,
                    std::path::Component::RootDir | std::path::Component::Normal(_)
                )),
            "invalid signer state path"
        );
        ensure!(
            !self.socket.starts_with(&self.state_directory)
                && !self.socket.starts_with(&self.nak.private_directory)
                && !self
                    .state_directory
                    .starts_with(&self.nak.private_directory)
                && !self
                    .nak
                    .private_directory
                    .starts_with(&self.state_directory),
            "signer state, helper state and socket must be separate"
        );
        Ok(())
    }
    fn require_identity(&self) -> Result<()> {
        self.validate()?;
        ensure!(
            rustix::process::geteuid().as_raw() == self.runtime_uid,
            "signer runtime identity differs from registration"
        );
        crate::private_ipc::private_state(&self.state_directory)
    }
    /// Creates only the explicitly registered journal; never contacts the bunker.
    pub fn initialize(self) -> Result<()> {
        self.require_identity()?;
        drop(Broker::initialize(&self.state_directory, self.policy)?);
        Ok(())
    }
    pub fn open(self) -> Result<Runtime> {
        self.require_identity()?;
        crate::writer_lock::check_recovery_marker(&self.state_directory)?;
        let backend = Nak::new(self.nak, load_client_credential()?)?;
        let broker = Broker::open(&self.state_directory, self.policy)?;
        Ok(Runtime {
            broker,
            backend,
            socket: self.socket,
            group: self.ipc_gid,
        })
    }
}

pub struct Runtime {
    broker: Broker,
    backend: Nak,
    socket: PathBuf,
    group: u32,
}
impl Runtime {
    pub async fn serve(mut self, shutdown: impl std::future::Future<Output = ()>) -> Result<()> {
        self.broker
            .serve(&self.socket, self.group, &self.backend, shutdown)
            .await
    }
}

fn load_client_credential() -> Result<Zeroizing<String>> {
    let directory = PathBuf::from(
        std::env::var_os("CREDENTIALS_DIRECTORY")
            .ok_or_else(|| anyhow::anyhow!("signer credential directory unavailable"))?,
    );
    ensure!(
        directory.is_absolute() && directory.canonicalize()? == directory,
        "invalid signer credential directory"
    );
    let uid = rustix::process::geteuid().as_raw();
    ensure!(uid > 0, "signer credentials require a non-root runtime");
    for path in directory.ancestors().skip(1) {
        let metadata = std::fs::symlink_metadata(path)?;
        ensure!(
            metadata.is_dir() && metadata.uid() == 0 && metadata.mode() & 0o022 == 0,
            "unprotected signer credential directory"
        );
    }
    let systemd = directory.parent() == Some(Path::new(SYSTEMD_CREDENTIALS))
        && directory
            .extension()
            .is_some_and(|suffix| suffix == "service");
    let handle = File::from(posix::open(
        &directory,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC | OFlags::NOFOLLOW | OFlags::NONBLOCK,
        Mode::empty(),
    )?);
    ensure!(
        private_credential_mode(&handle, uid, 0o077, systemd, 5)?,
        "unprotected signer credential directory"
    );
    read_client_credential(&handle, Path::new(CLIENT_CREDENTIAL), uid, systemd)
}

fn private_credential_mode(
    file: &File,
    uid: u32,
    private_mask: u32,
    systemd: bool,
    rights: u16,
) -> Result<bool> {
    let metadata = file.metadata()?;
    if (metadata.uid() == uid || metadata.uid() == 0) && metadata.mode() & private_mask == 0 {
        return Ok(true);
    }
    // systemd can preserve root ownership while giving this service read access
    // through a named-user ACL. st_mode exposes the ACL mask as group bits.
    let mode = u32::from(rights) * 0o110;
    if !systemd
        || metadata.uid() != 0
        || metadata.gid() != 0
        || metadata.mode() & 0o7777 != mode
        || !posix::fstatvfs(file)?
            .f_flag
            .contains(StatVfsMountFlags::RDONLY)
    {
        return Ok(false);
    }
    let mut acl = [0; 44];
    let Ok(length) = posix::fgetxattr(file, "system.posix_acl_access", &mut acl[..]) else {
        return Ok(false);
    };
    Ok(exact_systemd_acl(&acl[..length], uid, rights))
}

fn exact_systemd_acl(raw: &[u8], uid: u32, rights: u16) -> bool {
    // Linux ACL xattr version 2 has an ordered eight-byte entry for each tag.
    let entries = [
        (0x01_u16, rights, u32::MAX), // owner
        (0x02, rights, uid),          // dedicated runtime UID
        (0x04, 0, u32::MAX),          // owning group
        (0x10, rights, u32::MAX),     // effective ACL mask
        (0x20, 0, u32::MAX),          // other
    ];
    raw.len() == 44
        && raw[..4] == 2_u32.to_le_bytes()
        && raw[4..]
            .chunks_exact(8)
            .zip(entries)
            .all(|(entry, (tag, mode, id))| {
                entry[..2] == tag.to_le_bytes()
                    && entry[2..4] == mode.to_le_bytes()
                    && entry[4..] == id.to_le_bytes()
            })
}

fn read_client_credential(
    directory: impl AsFd,
    path: &Path,
    uid: u32,
    systemd: bool,
) -> Result<Zeroizing<String>> {
    let fd = posix::openat(
        directory,
        path,
        OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NOFOLLOW | OFlags::NONBLOCK,
        Mode::empty(),
    )?;
    let mut file = File::from(fd);
    let metadata = file.metadata()?;
    ensure!(
        metadata.is_file()
            && metadata.nlink() == 1
            && private_credential_mode(&file, uid, 0o177, systemd, 4)?
            && (64..=65).contains(&metadata.len()),
        "invalid signer client credential file"
    );
    let mut bytes = Zeroizing::new(Vec::new());
    file.by_ref().take(66).read_to_end(&mut bytes)?;
    if bytes.last() == Some(&b'\n') {
        bytes.pop();
    }
    ensure!(
        bytes.len() == 64 && bytes.iter().all(u8::is_ascii_hexdigit),
        "invalid signer client credential encoding"
    );
    Ok(Zeroizing::new(String::from_utf8(bytes.to_vec()).map_err(
        |_| anyhow::anyhow!("invalid signer client credential encoding"),
    )?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    fn fixture(directory: &Path) -> serde_json::Value {
        let key = |byte| {
            cdk::nuts::SecretKey::from_slice(&[byte; 32])
                .unwrap()
                .public_key()
                .x_only_public_key()
                .to_string()
        };
        let uid = rustix::process::geteuid().as_raw();
        serde_json::json!({"version":1,"runtime_uid":uid,"ipc_gid":uid,
            "state_directory":directory,"socket":"/run/cashu-signer/control.sock",
            "policy":{"broker_instance":uuid::Uuid::from_u128(1),"broker_epoch":uuid::Uuid::from_u128(2),
                "wallet_instance":uuid::Uuid::from_u128(3),"wallet_epoch":uuid::Uuid::from_u128(4),
                "wallet_uid":uid+1,"owner":key(4),"receiving_key":key(1),
                "mints":["https://mint.example"],"relays":["wss://relay.example"]},
            "nak":{"binary":"/usr/local/bin/nak-cashu-stdin","binary_sha256":"ab".repeat(32),
                "private_directory":"/var/lib/cashu-nak","owner":key(4),"client_pubkey":key(2),
                "relays":["wss://relay.example"],"deadline_seconds":30}})
    }
    #[test]
    fn configuration_binds_separate_identities_paths_and_default_off_policy() -> Result<()> {
        let temp = crate::test_directory()?;
        let original = fixture(temp.path());
        let config = Config::decode(&serde_json::to_vec(&original)?)?;
        assert!(!config.policy.announcements && !config.policy.redemptions);
        assert!(!temp.path().join("cashu-signing.sqlite").exists());
        for (pointer, value) in [
            ("/version", serde_json::json!(2)),
            ("/runtime_uid", original["policy"]["wallet_uid"].clone()),
            ("/nak/owner", original["nak"]["client_pubkey"].clone()),
            ("/policy/receiving_key", original["policy"]["owner"].clone()),
            (
                "/nak/client_pubkey",
                original["policy"]["receiving_key"].clone(),
            ),
            (
                "/nak/private_directory",
                original["state_directory"].clone(),
            ),
            (
                "/socket",
                serde_json::json!(temp.path().join("control.sock")),
            ),
            ("/policy/announcements", serde_json::json!(true)),
        ] {
            let mut changed = original.clone();
            let (parent, field) = pointer.rsplit_once('/').unwrap();
            changed.pointer_mut(parent).unwrap()[field] = value;
            assert!(Config::decode(&serde_json::to_vec(&changed)?).is_err());
        }
        let mut unknown = original.clone();
        unknown["secret"] = "forbidden".into();
        assert!(Config::decode(&serde_json::to_vec(&unknown)?).is_err());
        assert!(Config::decode(&vec![b' '; MAX_CONFIG + 1]).is_err());
        Ok(())
    }
    #[tokio::test]
    async fn initialization_is_explicit_and_never_needs_a_helper_or_credential() -> Result<()> {
        let temp = crate::test_directory()?;
        let bytes = serde_json::to_vec(&fixture(temp.path()))?;
        Config::decode(&bytes)?.initialize()?;
        let database = temp.path().join("cashu-signing.sqlite");
        let before = std::fs::read(&database)?;
        assert!(Config::decode(&bytes)?.initialize().is_err());
        assert_eq!(std::fs::read(&database)?, before);
        let mut wrong = fixture(temp.path());
        wrong["runtime_uid"] = (rustix::process::geteuid().as_raw() + 2).into();
        assert!(
            Config::decode(&serde_json::to_vec(&wrong)?)?
                .initialize()
                .is_err()
        );
        assert_eq!(std::fs::read(&database)?, before);
        Ok(())
    }
    #[test]
    fn credential_is_bounded_private_and_never_follows_aliases() -> Result<()> {
        let temp = crate::test_directory()?;
        let path = temp.path().join("client");
        let value = "02".repeat(32);
        std::fs::write(&path, format!("{value}\n"))?;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
        let uid = rustix::process::geteuid().as_raw();
        let load = |file: &Path| read_client_credential(posix::CWD, file, uid, false);
        assert_eq!(load(&path)?.as_str(), value);
        let alias = temp.path().join("alias");
        std::os::unix::fs::symlink(&path, &alias)?;
        assert!(load(&alias).is_err());
        std::fs::remove_file(&alias)?;
        std::fs::hard_link(&path, &alias)?;
        assert!(load(&path).is_err());
        std::fs::remove_file(&alias)?;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o640))?;
        assert!(load(&path).is_err());
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
        for invalid in [
            "x".repeat(64),
            "0".repeat(63),
            "0".repeat(66),
            format!("{}\r\n", "0".repeat(64)),
        ] {
            std::fs::write(&path, invalid)?;
            assert!(load(&path).is_err());
        }
        Ok(())
    }

    #[test]
    fn systemd_named_acl_grants_only_the_registered_uid() -> Result<()> {
        let mut acl = hex::decode(concat!(
            "02000000",
            "01000400ffffffff",
            "0200040034120000",
            "04000000ffffffff",
            "10000400ffffffff",
            "20000000ffffffff"
        ))?;
        assert!(exact_systemd_acl(&acl, 0x1234, 4));
        assert!(!exact_systemd_acl(&acl, 0x1235, 4));
        assert!(!exact_systemd_acl(&acl, 0x1234, 5));
        acl[22] = 4; // Give the owning group read permission too.
        assert!(!exact_systemd_acl(&acl, 0x1234, 4));
        Ok(())
    }
}
