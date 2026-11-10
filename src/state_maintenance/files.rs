use super::{Component, Image, Role};
use anyhow::{Context, Result, bail};
use sha2::{Digest, Sha256};
use std::{
    fs::{File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt},
    path::{Component as PathComponent, Path, PathBuf},
};

pub(super) fn require_root() -> Result<()> {
    if crate::private_ipc::uid()? != 0 {
        bail!("state maintenance requires the local root operator");
    }
    Ok(())
}
pub(super) fn absolute(path: &Path) -> Result<()> {
    if !path.is_absolute()
        || path.file_name().is_none()
        || path
            .components()
            .any(|c| !matches!(c, PathComponent::RootDir | PathComponent::Normal(_)))
    {
        bail!("state paths must be absolute with no aliases");
    }
    Ok(())
}
fn root_parents(path: &Path) -> Result<()> {
    if path.canonicalize()? != path {
        bail!("protected path is not canonical");
    }
    for part in path.ancestors() {
        let m = std::fs::symlink_metadata(part)?;
        if !m.is_dir() || m.uid() != 0 || m.mode() & 0o022 != 0 {
            bail!("maintenance parent must be root-owned and protected");
        }
    }
    Ok(())
}
pub(super) fn new_private_directory(path: &Path) -> Result<()> {
    absolute(path)?;
    root_parents(path.parent().context("output parent missing")?)?;
    std::fs::DirBuilder::new().mode(0o700).create(path)?;
    sync_directory(path)?;
    sync_directory(path.parent().context("output parent missing")?)
}
pub(super) fn private_bundle(path: &Path) -> Result<()> {
    absolute(path)?;
    root_parents(path)?;
    if std::fs::metadata(path)?.mode() & 0o077 != 0 {
        bail!("maintenance bundle must be private");
    }
    Ok(())
}
pub(super) fn write_new(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}
pub(super) fn sync_directory(path: &Path) -> Result<()> {
    File::open(path)?.sync_all()?;
    Ok(())
}
pub(super) fn hash_bytes(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Fingerprint {
    dev: u64,
    ino: u64,
    bytes: u64,
    uid: u32,
    gid: u32,
    mode: u32,
    ctime: i64,
    ctime_ns: i64,
    mtime: i64,
    mtime_ns: i64,
    nlink: u64,
}
impl Fingerprint {
    fn get(m: &std::fs::Metadata) -> Self {
        Self {
            dev: m.dev(),
            ino: m.ino(),
            bytes: m.len(),
            uid: m.uid(),
            gid: m.gid(),
            mode: m.mode(),
            ctime: m.ctime(),
            ctime_ns: m.ctime_nsec(),
            mtime: m.mtime(),
            mtime_ns: m.mtime_nsec(),
            nlink: m.nlink(),
        }
    }
}
struct OpenImage {
    path: PathBuf,
    file: File,
    original: Fingerprint,
    max: u64,
}
impl OpenImage {
    fn open(path: PathBuf, uid: u32, gid: Option<u32>, max: u64) -> Result<Self> {
        absolute(&path)?;
        let metadata = std::fs::symlink_metadata(&path)?;
        if !metadata.is_file()
            || metadata.uid() != uid
            || gid.is_some_and(|g| g != metadata.gid())
            || metadata.mode() & 0o077 != 0
            || metadata.nlink() != 1
            || metadata.len() > max
            || path.canonicalize()? != path
        {
            bail!("journal file identity, privacy or size differs");
        }
        let file = File::open(&path)?;
        let original = Fingerprint::get(&metadata);
        if Fingerprint::get(&file.metadata()?) != original {
            bail!("journal changed during open");
        }
        Ok(Self {
            path,
            file,
            original,
            max,
        })
    }
    fn unchanged(&self) -> Result<()> {
        if Fingerprint::get(&self.file.metadata()?) != self.original
            || Fingerprint::get(&std::fs::symlink_metadata(&self.path)?) != self.original
            || self.path.canonicalize()? != self.path
        {
            bail!("journal changed while taking the snapshot");
        }
        Ok(())
    }
    fn digest(&mut self, mut output: Option<&mut File>) -> Result<(u64, String)> {
        self.unchanged()?;
        self.file.seek(SeekFrom::Start(0))?;
        let mut hash = Sha256::new();
        let mut total = 0u64;
        let mut buffer = [0u8; 65536];
        loop {
            let n = self.file.read(&mut buffer)?;
            if n == 0 {
                break;
            }
            total = total
                .checked_add(n as u64)
                .context("snapshot size overflow")?;
            if total > self.max {
                bail!("journal grew beyond snapshot limit");
            }
            hash.update(&buffer[..n]);
            if let Some(writer) = output.as_deref_mut() {
                writer.write_all(&buffer[..n])?;
            }
        }
        if total != self.original.bytes {
            bail!("journal length changed while copying");
        }
        self.unchanged()?;
        Ok((total, hex::encode(hash.finalize())))
    }
    fn copy(&mut self, destination: &Path, name: String) -> Result<Image> {
        let mut writer = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(destination.join(&name))?;
        let (bytes, sha256) = self.digest(Some(&mut writer))?;
        writer.sync_all()?;
        Ok(Image {
            name,
            bytes,
            sha256,
        })
    }
}

pub(super) struct Source {
    component: Component,
    main: OpenImage,
    wal: Option<OpenImage>,
    shm: Option<OpenImage>,
}
impl Source {
    pub(super) fn open(c: &Component, max: u64) -> Result<Self> {
        Self::check_parent(c)?;
        let main = OpenImage::open(c.database.clone(), c.uid, Some(c.gid), max)?;
        let side = |suffix: &str| -> Result<Option<OpenImage>> {
            let path = sidecar(&c.database, suffix);
            match std::fs::symlink_metadata(&path) {
                Ok(_) => Ok(Some(OpenImage::open(path, c.uid, Some(c.gid), max)?)),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
                Err(e) => Err(e.into()),
            }
        };
        let wal = side("-wal")?;
        let shm = side("-shm")?;
        Ok(Self {
            component: c.clone(),
            main,
            wal,
            shm,
        })
    }
    fn check_parent(c: &Component) -> Result<()> {
        let parent = c.database.parent().context("source parent missing")?;
        let m = std::fs::symlink_metadata(parent)?;
        if !m.is_dir()
            || m.uid() != c.uid
            || m.gid() != c.gid
            || m.mode() & 0o077 != 0
            || parent.canonicalize()? != parent
        {
            bail!("source state directory differs from the private runtime owner");
        }
        root_parents(parent.parent().context("source ancestor missing")?)?;
        Ok(())
    }
    pub(super) fn unchanged(&self) -> Result<()> {
        Self::check_parent(&self.component)?;
        self.main.unchanged()?;
        for (image, suffix) in [(&self.wal, "-wal"), (&self.shm, "-shm")] {
            if let Some(image) = image {
                image.unchanged()?;
            } else {
                match std::fs::symlink_metadata(sidecar(&self.main.path, suffix)) {
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                    Err(error) => return Err(error.into()),
                    Ok(_) => bail!("source sidecar appeared during snapshot"),
                }
            }
        }
        Ok(())
    }
    pub(super) fn copy(&mut self, destination: &Path, role: Role) -> Result<Vec<Image>> {
        self.unchanged()?;
        let mut result = vec![self.main.copy(destination, role.file())?];
        if let Some(wal) = &mut self.wal {
            result.push(wal.copy(destination, format!("{}-wal", role.file()))?);
        }
        self.unchanged()?;
        Ok(result)
    }
}
pub(super) fn sidecar(path: &Path, suffix: &str) -> PathBuf {
    let mut s = path.as_os_str().to_os_string();
    s.push(suffix);
    s.into()
}

pub(super) fn read_private(path: &Path, max: u64) -> Result<Vec<u8>> {
    let mut image = OpenImage::open(path.to_owned(), 0, None, max)?;
    let mut bytes = Vec::new();
    Read::by_ref(&mut image.file)
        .take(max + 1)
        .read_to_end(&mut bytes)?;
    image.unchanged()?;
    if bytes.len() as u64 > max {
        bail!("private metadata exceeds limit");
    }
    Ok(bytes)
}
pub(super) fn private_image(path: &Path, max: u64) -> Result<Image> {
    let mut image = OpenImage::open(path.to_owned(), 0, None, max)?;
    let (bytes, sha256) = image.digest(None)?;
    Ok(Image {
        name: path
            .file_name()
            .context("image filename missing")?
            .to_str()
            .context("image name must be UTF-8")?
            .to_owned(),
        bytes,
        sha256,
    })
}
pub(super) fn check_images(directory: &Path, images: &[Image], max: u64) -> Result<()> {
    check_selected_images(directory, images, max, &Role::ALL)
}
pub(super) fn check_core_images(directory: &Path, images: &[Image], max: u64) -> Result<()> {
    check_selected_images(directory, images, max, &[Role::Core])
}
fn check_selected_images(
    directory: &Path,
    images: &[Image],
    max: u64,
    roles: &[Role],
) -> Result<()> {
    let allowed: std::collections::BTreeSet<_> = roles
        .iter()
        .flat_map(|r| [r.file(), format!("{}-wal", r.file())])
        .collect();
    let mut names = std::collections::BTreeSet::new();
    for i in images {
        crate::nostr_event::validate_hex(&i.sha256, 32)?;
        if !allowed.contains(&i.name) || !names.insert(&i.name) || i.bytes > max {
            bail!("bundle image set differs");
        }
        let mut image = OpenImage::open(directory.join(&i.name), 0, None, max)?;
        if image.digest(None)? != (i.bytes, i.sha256.clone()) {
            bail!("bundle image checksum differs");
        }
    }
    for role in roles {
        if !names.contains(&role.file()) {
            bail!("the complete selected journal set is required");
        }
    }
    for entry in std::fs::read_dir(directory)? {
        let e = entry?;
        let name = e
            .file_name()
            .into_string()
            .map_err(|_| anyhow::anyhow!("invalid bundle filename"))?;
        if names.contains(&name) || matches!(name.as_str(), "MANIFEST.json" | "INCOMPLETE") {
            continue;
        }
        // SQLite may create a private, disposable read-index beside a copied WAL.
        if roles.iter().any(|r| name == format!("{}-shm", r.file())) {
            OpenImage::open(e.path(), 0, None, max)?;
        } else {
            bail!("bundle has an unlisted file");
        }
    }
    Ok(())
}
pub(super) fn copy_private(source: &Path, destination: &Path, expected: &Image) -> Result<()> {
    let mut original = OpenImage::open(source.to_owned(), 0, None, expected.bytes)?;
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(destination)?;
    if original.digest(Some(&mut output))? != (expected.bytes, expected.sha256.clone()) {
        bail!("restore source changed");
    }
    output.sync_all()?;
    Ok(())
}
