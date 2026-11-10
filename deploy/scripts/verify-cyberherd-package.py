#!/usr/bin/env python3
"""Verify a review archive without extracting, installing or executing it."""
import hashlib
import json
from pathlib import Path, PurePosixPath
import re
import runpy
import sys
import tarfile
import tomllib


def verify(path, source):
    if not re.fullmatch(r"[0-9a-f]{40}", source):
        raise ValueError("expected exact source SHA")
    files = {}
    total = 0
    with tarfile.open(path, "r:gz") as archive:
        for member in archive:
            name = PurePosixPath(member.name)
            if name.is_absolute() or ".." in name.parts or not (member.isdir() or member.isfile()):
                raise ValueError("archive has unsafe path/type")
            if member.isdir():
                continue
            key = str(name)
            total += member.size
            if key in files or member.size > 128 * 1024 * 1024 or total > 512 * 1024 * 1024:
                raise ValueError("duplicate/oversized review archive")
            files[key] = archive.extractfile(member).read()
    expected = {}
    for line in files["SHA256SUMS"].decode().splitlines():
        digest, name = line.split("  ", 1)
        name = str(PurePosixPath(name))
        if not re.fullmatch(r"[0-9a-f]{64}", digest) or name in expected:
            raise ValueError("invalid review checksum manifest")
        expected[name] = digest
    if set(expected) != set(files) - {"SHA256SUMS"}:
        raise ValueError("review manifest does not cover every file")
    for name, digest in expected.items():
        if hashlib.sha256(files[name]).hexdigest() != digest:
            raise ValueError("review file checksum mismatch")
    required = {"cyberherdd", "cyberherd-payoutd", "cyberherd-signerd", "cyberherd-statectl", "BUILD-INFO.txt", "Cargo.lock", "Cargo.toml",
                "deploy/cyberherd/state-maintenance.toml.example", "STATE-MAINTENANCE.md", "OFFLINE-INITIALIZATION.md",
                "deploy/cyberherd/config.toml.example", "deploy/cyberherd/payout.toml.example", "deploy/cyberherd/signer.toml.example"}
    if not required.issubset(files):
        raise ValueError("review archive is missing required artifacts")
    runpy.run_path(str(Path(__file__).with_name("verify-sqlite-vendor.py")))["verify_payload"](files)
    info = dict(line.split("=", 1) for line in files["BUILD-INFO.txt"].decode().splitlines())
    if info["source_commit"] != source or info["activation"] != "none":
        raise ValueError("review source/activation mismatch")
    for role in ["cyberherd", "cyberherd-payout", "cyberherd-signer"]:
        unit = files[f"deploy/systemd/{role}.service.example"].decode()
        if f"User={role}\n" not in unit or "NoNewPrivileges=yes\n" not in unit:
            raise ValueError("review unit lacks separate non-admin identity")
    for kind in ["config", "payout", "signer"]:
        config = tomllib.loads(files[f"deploy/cyberherd/{kind}.toml.example"].decode())
        if config["enabled"] is not False:
            raise ValueError("review configuration must be disabled")
        if kind == "payout" and (config["policy"]["execution_enabled"] or config["policy"]["max_sats_per_24h"]):
            raise ValueError("review payout must have no execution/budget")
    return {"source_commit": source, "activation": "none", "binaries": 4, "files": len(files)}


if __name__ == "__main__":
    print(json.dumps(verify(sys.argv[1], sys.argv[2]), sort_keys=True))
