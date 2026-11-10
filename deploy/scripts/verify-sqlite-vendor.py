#!/usr/bin/env python3
"""Verify the exact published SQLx driver and its single binding-pin backport."""
import hashlib
import io
import json
from pathlib import Path, PurePosixPath
import stat
import sys
import tarfile
import tomllib

UPSTREAM = "vendor/upstream/sqlx-sqlite-0.8.6.crate"
UPSTREAM_SHA256 = "c2d12fe70b2c1b4401038055f90f151b78208de1f9f89a7dbfd41587a10c3eea"
BINDING_SHA256 = "14a5f01b87b92705d07b433c06f96cbc764637d81a0bffb5734ce17cf0f9a2b3"
PREFIX = "vendor/sqlx-sqlite/"
OLD = b'[dependencies.libsqlite3-sys]\nversion = "0.30.1"'
NEW = b'[dependencies.libsqlite3-sys]\nversion = "=0.39.1"'


def verify_payload(files):
    original = files.get(UPSTREAM, b"")
    if hashlib.sha256(original).hexdigest() != UPSTREAM_SHA256:
        raise ValueError("published SQLx archive checksum differs")
    expected = {}
    with tarfile.open(fileobj=io.BytesIO(original), mode="r:gz") as archive:
        for member in archive:
            name = PurePosixPath(member.name)
            if (name.is_absolute() or ".." in name.parts or not member.isfile()
                    or not name.parts or name.parts[0] != "sqlx-sqlite-0.8.6"
                    or len(name.parts) < 2 or member.size > 2 * 1024 * 1024):
                raise ValueError("invalid published SQLx member")
            path = PREFIX + str(PurePosixPath(*name.parts[1:]))
            if path in expected or len(expected) >= 128:
                raise ValueError("duplicate/oversized published SQLx source")
            data = archive.extractfile(member).read()
            if path == PREFIX + "Cargo.toml":
                if data.count(OLD) != 1:
                    raise ValueError("upstream binding declaration changed")
                data = data.replace(OLD, NEW)
            expected[path] = data
    required = set(expected) | {UPSTREAM, "vendor/README.md"}
    if {name for name in files if name.startswith("vendor/")} != required:
        raise ValueError("vendored SQLx file set differs")
    for path, data in expected.items():
        if files[path] != data:
            raise ValueError("vendored SQLx changed beyond the binding pin: " + path)
    manifest = tomllib.loads(files["Cargo.toml"].decode())
    if (manifest["package"]["rust-version"] != "1.88"
            or manifest["dependencies"]["sqlx"] != {"version": "=0.8.6", "default-features": False,
                "features": ["runtime-tokio", "sqlite", "migrate", "macros", "uuid"]}
            or manifest["patch"]["crates-io"] != {"sqlx-sqlite": {"path": "vendor/sqlx-sqlite"}}):
        raise ValueError("application SQLx/MSRV patch binding differs")
    packages = tomllib.loads(files["Cargo.lock"].decode())["package"]
    drivers = [p for p in packages if p["name"] == "sqlx-sqlite"]
    bindings = [p for p in packages if p["name"] == "libsqlite3-sys"]
    cohort = [p for p in packages if p["name"].startswith("sqlx") and p["name"] != "sqlx-sqlite"]
    if (len(drivers) != 1 or drivers[0]["version"] != "0.8.6"
            or "source" in drivers[0] or "checksum" in drivers[0]
            or len(bindings) != 1 or bindings[0]["version"] != "0.39.1"
            or bindings[0].get("source") != "registry+https://github.com/rust-lang/crates.io-index"
            or bindings[0].get("checksum") != BINDING_SHA256):
        raise ValueError("locked SQLite driver/binding differs")
    if ({p["name"] for p in cohort} != {"sqlx", "sqlx-core", "sqlx-macros", "sqlx-macros-core", "sqlx-mysql", "sqlx-postgres"}
            or len(cohort) != 6 or any(p["version"] != "0.8.6" or p.get("source") != "registry+https://github.com/rust-lang/crates.io-index" for p in cohort)):
        raise ValueError("the rest of the SQLx dependency graph changed")
    return {"driver": "sqlx-sqlite 0.8.6", "binding": "libsqlite3-sys 0.39.1",
            "upstream_files": len(expected), "manifest_only_patch": True}


def payload(root):
    root = Path(root)
    if (root / "vendor").is_symlink():
        raise ValueError("vendor directory cannot be a symlink")
    files = {}
    for path in [root / "Cargo.toml", root / "Cargo.lock", *(root / "vendor").rglob("*")]:
        if path.is_symlink():
            raise ValueError("vendor payload cannot contain symlinks")
        metadata = path.stat()
        if stat.S_ISDIR(metadata.st_mode):
            continue
        if (not stat.S_ISREG(metadata.st_mode) or metadata.st_size > 2 * 1024 * 1024
                or len(files) >= 132 or sum(map(len, files.values())) + metadata.st_size > 8 * 1024 * 1024):
            raise ValueError("invalid/oversized vendor payload")
        files[path.relative_to(root).as_posix()] = path.read_bytes()
    return files


if __name__ == "__main__":
    root = Path(sys.argv[1]) if len(sys.argv) == 2 else Path(__file__).resolve().parents[2]
    print(json.dumps(verify_payload(payload(root)), sort_keys=True))
