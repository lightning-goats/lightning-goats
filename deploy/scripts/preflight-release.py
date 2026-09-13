#!/usr/bin/env python3
"""Verify pinned release bytes without executing or installing archive payloads.

Obtain the expected archive digest and source from independently reviewed build
evidence. Self-consistent archive metadata alone is not source provenance.
"""
import argparse
import hashlib
import importlib.util
import json
from pathlib import Path
import re
import tempfile

SPEC = importlib.util.spec_from_file_location(
    "release_verifier", Path(__file__).with_name("smoke-release.py"))
RELEASE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(RELEASE)
MAX_ARCHIVE_BYTES = 512 * 1024 * 1024


def preflight(archive, source, expected_digest):
    if not re.fullmatch(r"[0-9a-f]{40}", source):
        raise ValueError("Expected a full source commit")
    if not re.fullmatch(r"[0-9a-f]{64}", expected_digest):
        raise ValueError("Expected an independently supplied SHA256 digest")
    with tempfile.TemporaryDirectory(prefix="lg-preflight-") as directory:
        root = Path(directory)
        snapshot = root / "archive.tar.gz"
        digest = hashlib.sha256()
        size = 0
        # Verify the same private snapshot that is subsequently extracted, even
        # if the caller's original path is replaced during verification.
        with Path(archive).open("rb") as incoming, snapshot.open("xb") as outgoing:
            while chunk := incoming.read(1024 * 1024):
                size += len(chunk)
                if size > MAX_ARCHIVE_BYTES:
                    raise ValueError("Archive exceeds compressed size budget")
                digest.update(chunk)
                outgoing.write(chunk)
        if digest.hexdigest() != expected_digest:
            raise ValueError("Outer archive checksum mismatch")
        payload = root / "payload"
        payload.mkdir()
        RELEASE.verify_archive(snapshot, source, payload)
        hashes = {}
        for path in sorted(payload.rglob("*")):
            if path.is_file():
                with path.open("rb") as stream:
                    digest = hashlib.sha256()
                    while chunk := stream.read(1024 * 1024):
                        digest.update(chunk)
                    hashes[path.relative_to(payload).as_posix()] = digest.hexdigest()
        return {
            "schema_version": 1, "source_commit": source,
            "archive_sha256": expected_digest, "archive_bytes": size,
            "payload_sha256": hashes,
            "payload_executed": False, "installed": False, "activated": False,
            "scope": "Pinned archive integrity only; independently authenticate build provenance and verify installed permissions/sandbox before acceptance",
        }


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("archive", type=Path)
    parser.add_argument("source_commit")
    parser.add_argument("archive_sha256")
    args = parser.parse_args()
    print(json.dumps(preflight(args.archive, args.source_commit, args.archive_sha256), sort_keys=True, indent=2))
