#!/usr/bin/env python3
"""Verify minimal SQLite and CDK API backports against published archives."""
import hashlib
from pathlib import Path
import tarfile
import tomllib

ROOT = Path(__file__).resolve().parent
ARCHIVE_SHA256 = "b838eba278d213a8beaf485bd313fd580ca4505a00d5871caeb1457c55322cae"
BINDING_SHA256 = "14a5f01b87b92705d07b433c06f96cbc764637d81a0bffb5734ce17cf0f9a2b3"


def require(condition, message):
    if not condition:
        raise ValueError(message)


def verify_source(vendor, name, digest, changed_file, original, replacement):
    archive = vendor / f"{name}.crate"
    require(hashlib.sha256(archive.read_bytes()).hexdigest() == digest, "archive digest")
    source = vendor / name
    expected = set()
    with tarfile.open(archive) as bundle:
        for entry in bundle:
            if entry.isdir():
                continue
            require(entry.isfile(), "archive contains a non-file")
            relative = Path(entry.name).relative_to(name)
            require(".." not in relative.parts, "unsafe archive member")
            expected.add(relative)
            content = bundle.extractfile(entry).read()
            if relative == Path(changed_file):
                require(content.count(original) == 1, "upstream patch anchor changed")
                content = content.replace(original, replacement)
            actual = source / relative
            require(not actual.is_symlink() and actual.is_file(), f"missing/non-file: {relative}")
            require(actual.read_bytes() == content, f"unapproved change: {relative}")
    actual = set()
    for path in source.rglob("*"):
        require(not path.is_symlink(), "source symlink")
        if path.is_file():
            actual.add(path.relative_to(source))
    require(actual == expected, "source file inventory")


def verify(root=ROOT):
    vendor = root / "vendor"
    original = b'[dependencies.libsqlite3-sys]\nversion = "0.28.0"'
    verify_source(vendor, "rusqlite-0.31.0", ARCHIVE_SHA256, "Cargo.toml", original,
                  b'[dependencies.libsqlite3-sys]\nversion = "=0.39.1"')
    original = b'pub use mint_connector::transport::Transport as HttpTransport;'
    verify_source(vendor, "cdk-0.18.3",
                  "6969a2ea499035f5f49f5cdaf355f34251ce85a8ddd47c557177ecee5ed7da2a",
                  "src/wallet/mod.rs", original, original +
                  b'\n// Application transport adapter: expose the existing generic client, unchanged.\npub use mint_connector::http_client::HttpClient as CustomHttpClient;')
    manifest = tomllib.loads((root / "Cargo.toml").read_text())
    require(manifest["package"]["rust-version"] == "1.88", "MSRV changed")
    require(manifest["patch"]["crates-io"]["rusqlite"]["path"] == "vendor/rusqlite-0.31.0", "manifest backport path changed")
    require(manifest["patch"]["crates-io"]["cdk"]["path"] == "vendor/cdk-0.18.3", "CDK patch path changed")
    lock = tomllib.loads((root / "Cargo.lock").read_text())
    packages = lock["package"]
    binding = [p for p in packages if p["name"] == "libsqlite3-sys"]
    require(len(binding) == 1 and binding[0]["version"] == "0.39.1", "binding version")
    require(binding[0]["checksum"] == BINDING_SHA256, "binding digest")
    cdk = [p for p in packages if p["name"] == "cdk"]
    require(len(cdk) == 1 and cdk[0]["version"] == "0.18.3" and "source" not in cdk[0], "CDK backport binding")
    wrappers = [p for p in packages if p["name"] == "rusqlite"]
    require(len(wrappers) == 1 and wrappers[0]["version"] == "0.31.0", "wrapper version")
    require("source" not in wrappers[0], "wrapper must be the local backport")


if __name__ == "__main__":
    verify()
    print("Cashu SQLite/CDK sources, exact patches, inventories and locked bindings verified")
