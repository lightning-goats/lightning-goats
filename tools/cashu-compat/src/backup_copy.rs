//! Normalize protected, quiesced database COPIES, never installed application state.
//! This supplies no quiescence, coordinated-checkpoint or restore authority.
use anyhow::{Result, bail, ensure};
use rusqlite::{
    Connection, OpenFlags,
    backup::{Backup, StepResult},
};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    fs::{File, OpenOptions},
    io::{Read, Seek, SeekFrom},
    os::unix::fs::{MetadataExt, OpenOptionsExt},
    path::{Component, Path},
    sync::mpsc,
    time::{Duration, Instant},
};

pub const MARKER: &[u8] = b"LIGHTNING_GOATS_CASHU_BACKUP_COPY_V1\n";
const HARD_MAX_BYTES: u64 = 16 * 1024 * 1024 * 1024;

#[derive(Debug, PartialEq, Eq)]
struct Identity {
    device: u64,
    inode: u64,
    owner: u32,
    group: u32,
    mode: u32,
    links: u64,
    bytes: u64,
    modified: i64,
    modified_ns: i64,
    changed: i64,
    changed_ns: i64,
}
fn identity(meta: &std::fs::Metadata) -> Identity {
    Identity {
        device: meta.dev(),
        inode: meta.ino(),
        owner: meta.uid(),
        group: meta.gid(),
        mode: meta.mode(),
        links: meta.nlink(),
        bytes: meta.len(),
        modified: meta.mtime(),
        modified_ns: meta.mtime_nsec(),
        changed: meta.ctime(),
        changed_ns: meta.ctime_nsec(),
    }
}
fn protected_directory(path: &Path) -> Result<()> {
    ensure!(
        path.is_absolute()
            && path
                .components()
                .all(|c| matches!(c, Component::RootDir | Component::Normal(_))),
        "canonical absolute copy directory required"
    );
    ensure!(path.canonicalize()? == path, "copy directory alias refused");
    for ancestor in path.ancestors() {
        let meta = std::fs::symlink_metadata(ancestor)?;
        ensure!(
            meta.is_dir()
                && !meta.file_type().is_symlink()
                && meta.uid() == 0
                && meta.mode() & 0o022 == 0,
            "protected root-owned ancestry required"
        );
    }
    ensure!(
        std::fs::symlink_metadata(path)?.mode() & 0o077 == 0,
        "private copy directory required"
    );
    Ok(())
}
struct Frozen {
    file: File,
    identity: Identity,
    hash: String,
}
impl Frozen {
    fn open(path: &Path, maximum: u64, empty: bool) -> Result<Self> {
        let fd = rustix::fs::open(
            path,
            rustix::fs::OFlags::RDONLY | rustix::fs::OFlags::NOFOLLOW | rustix::fs::OFlags::CLOEXEC,
            rustix::fs::Mode::empty(),
        )?;
        let mut file = File::from(fd);
        let meta = file.metadata()?;
        ensure!(
            meta.is_file()
                && meta.nlink() == 1
                && meta.uid() == 0
                && meta.mode() & 0o077 == 0
                && meta.len() <= maximum
                && (empty || meta.len() > 0),
            "private bounded root-owned copy required"
        );
        let expected = identity(&meta);
        let hash = digest(&mut file, maximum)?;
        ensure!(
            identity(&file.metadata()?) == expected
                && identity(&std::fs::symlink_metadata(path)?) == expected,
            "copy changed during fingerprint"
        );
        Ok(Self {
            file,
            identity: expected,
            hash,
        })
    }
    fn unchanged(&mut self, path: &Path, maximum: u64, wal_owner_refresh: bool) -> Result<()> {
        let matches = |meta: &std::fs::Metadata| {
            let mut observed = identity(meta);
            if wal_owner_refresh {
                // SQLite's Unix VFS unconditionally fchowns an opened WAL
                // when root, even if its owner already matches. This changes
                // only ctime on our protected COPY. Every other identity field
                // and the complete byte hash must still match.
                observed.changed = self.identity.changed;
                observed.changed_ns = self.identity.changed_ns;
            }
            observed == self.identity
        };
        ensure!(
            matches(&self.file.metadata()?)
                && matches(&std::fs::symlink_metadata(path)?)
                && digest(&mut self.file, maximum)? == self.hash
                && matches(&self.file.metadata()?)
                && matches(&std::fs::symlink_metadata(path)?),
            "frozen database copy changed"
        );
        Ok(())
    }
}
fn digest(file: &mut File, maximum: u64) -> Result<String> {
    file.seek(SeekFrom::Start(0))?;
    let mut hash = Sha256::new();
    let mut count = 0u64;
    let mut buffer = [0u8; 65536];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        count = count
            .checked_add(read as u64)
            .ok_or_else(|| anyhow::anyhow!("copy size overflow"))?;
        ensure!(count <= maximum, "copy exceeds byte limit");
        hash.update(&buffer[..read]);
    }
    Ok(hex::encode(hash.finalize()))
}
fn absent(path: &Path) -> Result<()> {
    match std::fs::symlink_metadata(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.into()),
        Ok(_) => bail!("output or uncertain sidecar already exists"),
    }
}
fn basename(name: &str, prefix: &str) -> bool {
    name.starts_with(prefix)
        && name.len() <= 128
        && (name.ends_with(".sqlite") || name.ends_with(".db"))
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
}
fn integrity(connection: &Connection) -> Result<()> {
    ensure!(
        connection.query_row("PRAGMA integrity_check(1)", [], |r| r.get::<_, String>(0))? == "ok",
        "database copy integrity failed"
    );
    let mut statement = connection.prepare("PRAGMA foreign_key_check")?;
    ensure!(
        statement.query([])?.next()?.is_none(),
        "database copy has inconsistent foreign keys"
    );
    Ok(())
}
struct Deadline {
    complete: Option<mpsc::Sender<()>>,
    thread: Option<std::thread::JoinHandle<()>>,
}
impl Deadline {
    fn arm(connection: &Connection, remaining: Duration) -> Result<Self> {
        ensure!(!remaining.is_zero(), "copy normalization deadline exceeded");
        let (complete, receiver) = mpsc::channel();
        let connection = connection.get_interrupt_handle();
        let thread = std::thread::Builder::new()
            .name("cashu-copy-deadline".into())
            .spawn(move || {
                if matches!(
                    receiver.recv_timeout(remaining),
                    Err(mpsc::RecvTimeoutError::Timeout)
                ) {
                    connection.interrupt();
                }
            })?;
        Ok(Self {
            complete: Some(complete),
            thread: Some(thread),
        })
    }
}
impl Drop for Deadline {
    fn drop(&mut self) {
        if let Some(complete) = self.complete.take() {
            let _ = complete.send(());
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

#[derive(Serialize)]
pub struct Receipt {
    pub version: u8,
    pub engine: &'static str,
    pub source_main_sha256: String,
    pub source_wal_sha256: Option<String>,
    pub normalized_sha256: String,
    pub normalized_bytes: u64,
    pub input_scope: &'static str,
    pub application_runtime_started: bool,
    pub quiescence_verified: bool,
    pub coordinated_backup_verified: bool,
    pub restore_authorized: bool,
}

/// Root collector entry point. Inputs must already be private root-owned raw
/// copies under a marked staging directory. Existing outputs are never reused.
pub fn normalize(directory: &Path, input: &str, output: &str, maximum: u64) -> Result<Receipt> {
    ensure!(
        rustix::process::geteuid().as_raw() == 0,
        "root copy collector required"
    );
    ensure!(
        rusqlite::version() == "3.54.1",
        "qualified SQLite engine required"
    );
    let started = Instant::now();
    ensure!(
        (4096..=HARD_MAX_BYTES).contains(&maximum),
        "invalid copy byte bound"
    );
    ensure!(
        basename(input, "raw-") && basename(output, "normalized-"),
        "fixed copy basenames required"
    );
    protected_directory(directory)?;
    let marker_path = directory.join("INCOMPLETE");
    let mut marker = Frozen::open(&marker_path, 128, false)?;
    let mut marker_bytes = Vec::new();
    marker.file.seek(SeekFrom::Start(0))?;
    marker.file.read_to_end(&mut marker_bytes)?;
    ensure!(marker_bytes == MARKER, "backup copy stage marker required");
    let source_path = directory.join(input);
    let target_path = directory.join(output);
    let mut source_file = Frozen::open(&source_path, maximum, false)?;
    let wal_path = directory.join(format!("{input}-wal"));
    let mut wal = match std::fs::symlink_metadata(&wal_path) {
        Ok(_) => Some(Frozen::open(&wal_path, maximum, true)?),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(e.into()),
    };
    // A rollback journal is uncertain input, not permission to discard evidence.
    absent(&directory.join(format!("{input}-journal")))?;
    let shm_path = directory.join(format!("{input}-shm"));
    match std::fs::symlink_metadata(&shm_path) {
        Ok(_) => {
            let _ = Frozen::open(&shm_path, maximum, true)?;
            ensure!(wal.is_some(), "orphan copy shared-memory file");
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e.into()),
    }
    for suffix in ["", "-wal", "-shm", "-journal"] {
        absent(&directory.join(format!("{output}{suffix}")))?;
    }
    let mut source_uri =
        url::Url::from_file_path(&source_path).map_err(|_| anyhow::anyhow!("invalid copy path"))?;
    if wal.is_none() {
        source_uri.set_query(Some("immutable=1"));
    }
    let source = Connection::open_with_flags(
        source_uri.as_str(),
        OpenFlags::SQLITE_OPEN_READ_ONLY
            | OpenFlags::SQLITE_OPEN_URI
            | OpenFlags::SQLITE_OPEN_NOFOLLOW,
    )?;
    source.busy_timeout(Duration::from_secs(1))?;
    source.execute_batch(
        "PRAGMA query_only=ON; PRAGMA trusted_schema=OFF; PRAGMA hard_heap_limit=268435456;",
    )?;
    let source_deadline = Deadline::arm(
        &source,
        Duration::from_secs(60).saturating_sub(started.elapsed()),
    )?;
    // Bound the logical image before allocating destination pages.
    let pages: u64 = source.query_row("PRAGMA page_count", [], |r| r.get(0))?;
    let page_size: u64 = source.query_row("PRAGMA page_size", [], |r| r.get(0))?;
    ensure!(
        pages
            .checked_mul(page_size)
            .is_some_and(|n| n > 0 && n <= maximum),
        "logical database exceeds byte bound"
    );
    integrity(&source)?;
    let created = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&target_path)?;
    created.sync_all()?;
    let mut target = Connection::open_with_flags(
        &target_path,
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NOFOLLOW,
    )?;
    target.busy_timeout(Duration::from_secs(1))?;
    let target_deadline = Deadline::arm(
        &target,
        Duration::from_secs(60).saturating_sub(started.elapsed()),
    )?;
    {
        let backup = Backup::new(&source, &mut target)?;
        loop {
            ensure!(
                started.elapsed() < Duration::from_secs(60),
                "copy normalization deadline exceeded"
            );
            match backup.step(64)? {
                StepResult::Done => break,
                StepResult::More => {}
                _ => bail!("copy normalization could not obtain exclusive copy access"),
            }
        }
    }
    let mode: String = target.query_row("PRAGMA journal_mode=DELETE", [], |r| r.get(0))?;
    ensure!(mode == "delete", "normalized image must not require a WAL");
    integrity(&target)?;
    target.close().map_err(|(_, e)| e)?;
    drop(target_deadline);
    drop(source_deadline);
    source.close().map_err(|(_, e)| e)?;
    source_file.unchanged(&source_path, maximum, false)?;
    if let Some(wal) = &mut wal {
        wal.unchanged(&wal_path, maximum, true)?;
    } else {
        absent(&wal_path)?;
    }
    marker.unchanged(&marker_path, 128, false)?;
    let normalized = Frozen::open(&target_path, maximum, false)?;
    ensure!(
        identity(&created.metadata()?) == normalized.identity,
        "normalized output identity changed"
    );
    normalized.file.sync_all()?;
    for suffix in ["-wal", "-shm", "-journal"] {
        absent(&directory.join(format!("{output}{suffix}")))?;
    }
    File::open(directory)?.sync_all()?;
    protected_directory(directory)?;
    Ok(Receipt {
        version: 1,
        engine: "3.54.1",
        source_main_sha256: source_file.hash,
        source_wal_sha256: wal.map(|v| v.hash),
        normalized_sha256: normalized.hash,
        normalized_bytes: normalized.identity.bytes,
        input_scope: "protected_root_owned_copy_stage",
        application_runtime_started: false,
        quiescence_verified: false,
        coordinated_backup_verified: false,
        restore_authorized: false,
    })
}
