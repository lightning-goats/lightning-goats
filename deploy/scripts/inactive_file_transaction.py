"""Recoverable file replacement primitive for an inactive upgrade coordinator.

No CLI or service control. The caller must freeze other administrative writers,
validate fixed destinations/metadata and recheck service inactivity in `guard`.
This primitive never authorizes activation or changes a database.
"""
from contextlib import contextmanager
import fcntl
import hashlib
import json
import os
from pathlib import Path
import stat
import tempfile


def sync_directory(path):
    descriptor = os.open(path, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW)
    try:
        os.fsync(descriptor)
    finally:
        os.close(descriptor)


def fingerprint(path):
    descriptor = os.open(path, os.O_PATH | os.O_NOFOLLOW | os.O_CLOEXEC)
    try:
        info = os.fstat(descriptor)
        if not stat.S_ISREG(info.st_mode):
            raise ValueError("transaction input must be a regular file")
        digest = hashlib.sha256()
        with open(f"/proc/self/fd/{descriptor}", "rb") as incoming:
            while chunk := incoming.read(1024 * 1024):
                digest.update(chunk)
            attributes = {name: os.getxattr(incoming.fileno(), name).hex()
                          for name in os.listxattr(incoming.fileno())}
        return {"sha256": digest.hexdigest(), "uid": info.st_uid,
                "gid": info.st_gid, "mode": stat.S_IMODE(info.st_mode),
                "xattrs": attributes}
    finally:
        os.close(descriptor)


def copy_durable(source, destination, metadata):
    # All paths live under caller-validated, non-writable parents. Recheck the
    # resulting bytes and metadata before any destination rename.
    with Path(source).open("rb") as incoming, Path(destination).open("xb") as outgoing:
        while chunk := incoming.read(1024 * 1024):
            outgoing.write(chunk)
        outgoing.flush()
        os.fchown(outgoing.fileno(), metadata["uid"], metadata["gid"])
        os.fchmod(outgoing.fileno(), metadata["mode"])
        for name in os.listxattr(outgoing.fileno()):
            if name not in metadata["xattrs"]:
                os.removexattr(outgoing.fileno(), name)
        for name, value in metadata["xattrs"].items():
            os.setxattr(outgoing.fileno(), name, bytes.fromhex(value))
        os.fsync(outgoing.fileno())
    if fingerprint(destination) != metadata:
        raise ValueError("staged file fingerprint mismatch")


def prepare(directory, replacements, guard):
    """replacements: [(absolute destination, source, expected fingerprint)]."""
    guard()
    destinations = [str(Path(row[0])) for row in replacements]
    if not destinations or len(set(destinations)) != len(destinations):
        raise ValueError("destinations must be nonempty and unique")
    if any(not Path(p).is_absolute() for p in destinations):
        raise ValueError("destinations must be absolute")
    for destination, _, expected in replacements:
        if fingerprint(destination) != expected:
            raise ValueError("destination drift before preparation")
    directory = Path(directory)
    directory.mkdir(mode=0o700)  # refuses existing/partial transactions
    sync_directory(directory.parent)
    records = []
    for index, (destination, source, old) in enumerate(replacements):
        new = dict(old, sha256=fingerprint(source)["sha256"])
        copy_durable(destination, directory / f"{index}.old", old)
        copy_durable(source, directory / f"{index}.new", new)
        records.append({"destination": str(destination), "old": old, "new": new})
    # The immutable manifest is the recovery authority, written last. Never
    # infer a completed operation from an in-memory counter after a crash.
    with (directory / "TRANSACTION.json").open("x") as manifest:
        json.dump({"version": 1, "files": records}, manifest)
        manifest.flush()
        os.fsync(manifest.fileno())
    (directory / "LOCK").touch(exist_ok=False, mode=0o600)
    sync_directory(directory)
    return records


@contextmanager
def locked(directory):
    descriptor = os.open(directory / "LOCK", os.O_RDWR | os.O_NOFOLLOW | os.O_CLOEXEC)
    try:
        fcntl.flock(descriptor, fcntl.LOCK_EX | fcntl.LOCK_NB)
        yield
    finally:
        os.close(descriptor)


def execute(directory, guard, rollback=False, after_replace=lambda _: None):
    """Resume or roll back using actual fingerprints; never overwrite drift."""
    directory = Path(directory)
    with locked(directory):
        manifest = json.loads((directory / "TRANSACTION.json").read_text())
        if manifest["version"] != 1:
            raise ValueError("unknown transaction version")
        rows = manifest["files"]
        # Validate ALL original/staged copies and destinations before changes,
        # including records already applied before an interrupted invocation.
        guard()
        for index, row in enumerate(rows):
            for version in ["old", "new"]:
                if fingerprint(directory / f"{index}.{version}") != row[version]:
                    raise ValueError("transaction backup/staging corruption")
            if fingerprint(row["destination"]) not in [row["old"], row["new"]]:
                raise ValueError("destination drift; manual reconciliation required")
        version = "old" if rollback else "new"
        for index, row in enumerate(rows):
            guard()
            destination = Path(row["destination"])
            actual = fingerprint(destination)
            if actual == row[version]:
                continue
            if actual not in [row["old"], row["new"]]:
                raise ValueError("destination drift during transaction")
            # Temporary directory is on the destination filesystem; cleanup is
            # task-owned and never touches destination or retained backups.
            with tempfile.TemporaryDirectory(prefix=".lg-upgrade-", dir=destination.parent) as temp:
                staged = Path(temp) / "payload"
                copy_durable(directory / f"{index}.{version}", staged, row[version])
                guard()
                if fingerprint(destination) != actual:
                    raise ValueError("destination changed before replacement")
                os.replace(staged, destination)
                sync_directory(destination.parent)
            after_replace(index)
        guard()
        if any(fingerprint(row["destination"]) != row[version] for row in rows):
            raise ValueError("final transaction readback mismatch")
        return {"version": 1, "direction": "rollback" if rollback else "apply",
                "files_verified": len(rows), "services_activated": False}
