#!/usr/bin/env python3
"""Verify a trusted release archive, then run only each binary's --help path.

Checksums detect corruption; obtain the archive/manifest from a trusted release.
Never use this executable smoke test on an untrusted downloaded archive.
"""

import argparse
import hashlib
import os
from pathlib import Path, PurePosixPath
import re
import subprocess
import tarfile
import tempfile


BINARIES = {"lightning-goatsd", "lightning-goatsctl", "lightning-goats-gateway"}
REQUIRED = BINARIES | {
    "BUILD-INFO.txt", "Cargo.lock", "AGENTS.md",
    "deploy/nginx/lightning-goats-http.conf.example",
    "deploy/nginx/lightning-goats-production-site.conf.example",
    "deploy/nginx/lightning-goats-canary-site.conf.example",
    "deploy/systemd/lightning-goats-gateway.service",
    "docs/deployment/deployment-artifacts.md",
}


def verify_archive(archive, source_commit, root):
    """Extract into an empty private directory and verify; execute nothing."""
    if any(root.iterdir()):
        raise ValueError("Archive destination must be empty")
    files = {}
    with tarfile.open(archive, "r:gz") as bundle:
        for member in bundle:
            path = PurePosixPath(member.name)
            if path.is_absolute() or ".." in path.parts:
                raise ValueError("Unsafe archive path")
            if member.isdir():
                continue
            if not member.isfile() or member.size > 256 * 1024 * 1024:
                raise ValueError("Unsupported archive member")
            name = str(path)
            if name in files:
                raise ValueError("Duplicate archive member")
            if len(files) >= 4096 or sum(files.values()) + member.size > 512 * 1024 * 1024:
                raise ValueError("Archive exceeds size/count budget")
            files[name] = member.size
            destination = root / name
            destination.parent.mkdir(parents=True, exist_ok=True)
            with bundle.extractfile(member) as incoming, destination.open("wb") as outgoing:
                while chunk := incoming.read(1024 * 1024):
                    outgoing.write(chunk)
            destination.chmod(0o755 if name in BINARIES else 0o644)
    if not (REQUIRED | {"SHA256SUMS"}) <= files.keys():
        raise ValueError("Release is missing required files")
    expected = {}
    for line in (root / "SHA256SUMS").read_text().splitlines():
        match = re.fullmatch(r"([0-9a-f]{64})  (?:\./)?(.+)", line)
        if not match or match[2] in expected:
            raise ValueError("Invalid checksum manifest")
        expected[match[2]] = match[1]
    if expected.keys() != files.keys() - {"SHA256SUMS"}:
        raise ValueError("Checksum manifest must cover exactly the payload")
    for name, digest in expected.items():
        with (root / name).open("rb") as stream:
            actual = hashlib.sha256()
            while chunk := stream.read(1024 * 1024):
                actual.update(chunk)
            if actual.hexdigest() != digest:
                raise ValueError(f"Checksum mismatch: {name}")
    if not re.fullmatch(r"[0-9a-f]{40}", source_commit):
        raise ValueError("Expected a full source commit")
    info = (root / "BUILD-INFO.txt").read_text().splitlines()
    if info.count(f"source_commit={source_commit}") != 1:
        raise ValueError("Source commit mismatch")


def verify_and_smoke(archive, source_commit):
    with tempfile.TemporaryDirectory(prefix="lg-release-smoke-") as directory:
        root = Path(directory)
        verify_archive(archive, source_commit, root)
        for name in sorted(BINARIES):
            result = subprocess.run(
                [str(root / name), "--help"], cwd=root,
                env={"PATH": os.defpath, "HOME": str(root)},
                capture_output=True, text=True, timeout=10, check=True,
            )
            if not result.stdout.strip():
                raise ValueError(f"Empty --help output: {name}")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("archive", type=Path)
    parser.add_argument("source_commit")
    args = parser.parse_args()
    verify_and_smoke(args.archive, args.source_commit)
    print("PASS: complete archive, payload checksums, source commit and all three --help commands")
