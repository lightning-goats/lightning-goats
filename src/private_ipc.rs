//! Shared Unix transport/startup checks for separate CyberHerd identities.
//! Files and kernel credentials, never a claimed UID in JSON, are authority.
use anyhow::{Context, Result, bail};
use serde::{Serialize, de::DeserializeOwned};
use std::{
    io::ErrorKind,
    os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt},
    path::{Component, Path},
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{UnixListener, UnixStream},
    time::timeout,
};

pub fn uid() -> Result<u32> {
    Ok(UnixStream::pair()?.0.peer_cred()?.uid())
}
pub fn socket_path(path: &Path) -> Result<()> {
    if !path.is_absolute()
        || path.as_os_str().len() > 100
        || path.file_name().is_none()
        || path
            .components()
            .any(|p| !matches!(p, Component::RootDir | Component::Normal(_)))
    {
        bail!("invalid protected socket path");
    }
    Ok(())
}
pub fn protected_config(path: &Path) -> Result<()> {
    if path.canonicalize()? != path {
        bail!("configuration must have a canonical path");
    }
    for (i, part) in path.ancestors().enumerate() {
        let meta = std::fs::symlink_metadata(part)?;
        if meta.uid() != 0
            || meta.mode() & 0o022 != 0
            || if i == 0 {
                !meta.is_file()
            } else {
                !meta.is_dir()
            }
        {
            bail!("configuration and parents must be root-owned protected files/directories");
        }
    }
    Ok(())
}
pub fn private_state(path: &Path) -> Result<()> {
    let meta = std::fs::symlink_metadata(path)?;
    if uid()? == 0
        || !meta.is_dir()
        || meta.uid() != uid()?
        || meta.mode() & 0o077 != 0
        || path.canonicalize()? != path
    {
        bail!(
            "state directory requires a separate non-root owner, canonical path and private permissions"
        );
    }
    Ok(())
}
pub fn runtime_group(path: &Path, gid: u32) -> Result<()> {
    let meta = std::fs::symlink_metadata(path)?;
    if gid == 0
        || !meta.is_dir()
        || meta.uid() != uid()?
        || meta.mode() & 0o027 != 0
        || path.canonicalize()? != path
    {
        bail!("runtime directory must be owned and protected before assigning IPC group");
    }
    // An unprivileged owner can choose only a group it already belongs to.
    std::os::unix::fs::chown(path, None, Some(gid))?;
    Ok(())
}
pub async fn bind(path: &Path, gid: u32) -> Result<UnixListener> {
    socket_path(path)?;
    let owner = uid()?;
    let parent = path.parent().context("socket parent missing")?;
    let meta = std::fs::symlink_metadata(parent)?;
    if owner == 0
        || gid == 0
        || !meta.is_dir()
        || meta.uid() != owner
        || meta.gid() != gid
        || meta.mode() & 0o027 != 0
        || meta.mode() & 0o010 == 0
        || parent.canonicalize()? != parent
    {
        bail!("socket parent requires canonical runtime ownership and protected group traversal");
    }
    match std::fs::symlink_metadata(path) {
        Ok(old) => {
            if !old.file_type().is_socket()
                || old.uid() != owner
                || old.gid() != gid
                || old.mode() & 0o007 != 0
            {
                bail!("unexpected private socket path");
            }
            match timeout(Duration::from_millis(500), UnixStream::connect(path)).await {
                Ok(Err(e)) if e.kind() == ErrorKind::ConnectionRefused => {}
                _ => bail!("private socket is active or cannot be identified as stale"),
            }
            let current = std::fs::symlink_metadata(path)?;
            if current.ino() != old.ino()
                || current.dev() != old.dev()
                || !current.file_type().is_socket()
            {
                bail!("socket changed during stale check");
            }
            std::fs::remove_file(path)?;
        }
        Err(e) if e.kind() == ErrorKind::NotFound => {}
        Err(e) => return Err(e.into()),
    }
    let listener = UnixListener::bind(path)?;
    std::os::unix::fs::chown(path, None, Some(gid))?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o660))?;
    Ok(listener)
}
pub async fn read<T: DeserializeOwned>(stream: &mut UnixStream, max: usize) -> Result<T> {
    let length = usize::try_from(stream.read_u32().await?)?;
    if length == 0 || length > max {
        bail!("private frame exceeds limit");
    }
    let mut bytes = vec![0; length];
    stream.read_exact(&mut bytes).await?;
    serde_json::from_slice(&bytes).map_err(|_| anyhow::anyhow!("invalid private frame schema"))
}
pub async fn write<T: Serialize>(stream: &mut UnixStream, value: &T, max: usize) -> Result<()> {
    let raw = serde_json::to_vec(value)?;
    if raw.is_empty() || raw.len() > max {
        bail!("private frame exceeds limit");
    }
    stream.write_u32(raw.len().try_into()?).await?;
    stream.write_all(&raw).await?;
    stream.flush().await?;
    Ok(())
}
pub async fn request<Q: Serialize, R: DeserializeOwned>(
    path: &Path,
    peer_uid: u32,
    value: &Q,
    max: usize,
    seconds: u64,
) -> Result<R> {
    timeout(Duration::from_secs(seconds), async {
        let mut stream = UnixStream::connect(path).await?;
        if stream.peer_cred()?.uid() != peer_uid {
            bail!("private socket peer identity changed");
        }
        write(&mut stream, value, max).await?;
        read(&mut stream, max).await
    })
    .await
    .context("private request deadline exceeded")?
}
#[cfg(target_os = "linux")]
pub fn singleton(label: &str, state: &Path) -> Result<std::os::unix::net::UnixListener> {
    use sha2::{Digest, Sha256};
    use std::os::linux::net::SocketAddrExt;
    let name = format!(
        "{label}-{}-{}",
        uid()?,
        hex::encode(Sha256::digest(state.as_os_str().as_encoded_bytes()))
    );
    let address = std::os::unix::net::SocketAddr::from_abstract_name(name.as_bytes())?;
    std::os::unix::net::UnixListener::bind_addr(&address)
        .context("state is already owned by another worker")
}
#[cfg(not(target_os = "linux"))]
pub fn singleton(_label: &str, _state: &Path) -> Result<std::os::unix::net::UnixListener> {
    bail!("runtime singleton requires Linux")
}
