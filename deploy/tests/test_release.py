"""Archive regressions use harmless executable fixtures; CI also packages real binaries."""

import hashlib
import importlib.util
import io
from pathlib import Path
import tarfile
import tempfile
import unittest


SPEC = importlib.util.spec_from_file_location(
    "smoke_release", Path(__file__).resolve().parents[1] / "scripts/smoke-release.py"
)
SMOKE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(SMOKE)
SOURCE = "a" * 40


class ReleaseArchiveTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.archive = Path(self.directory.name) / "release.tar.gz"
        self.files = {name: b"fixture\n" for name in SMOKE.REQUIRED}
        for name in SMOKE.BINARIES:
            self.files[name] = b'#!/bin/sh\n[ "$1" = "--help" ] || exit 1\nprintf "harmless help\\n"\n'
        self.files["BUILD-INFO.txt"] = f"source_commit={SOURCE}\n".encode()

    def write_archive(self, mutate=None, extra_member=None):
        manifest = "".join(
            f"{hashlib.sha256(data).hexdigest()}  ./{name}\n"
            for name, data in sorted(self.files.items())
        ).encode()
        if mutate:
            mutate()
        with tarfile.open(self.archive, "w:gz") as bundle:
            for name, data in {**self.files, "SHA256SUMS": manifest}.items():
                member = tarfile.TarInfo("./" + name)
                member.size = len(data)
                bundle.addfile(member, io.BytesIO(data))
            if extra_member:
                bundle.addfile(extra_member)

    def test_complete_archive_and_all_three_help_paths(self):
        self.write_archive()
        SMOKE.verify_and_smoke(self.archive, SOURCE)

    def test_missing_gateway_fails_even_with_matching_manifest(self):
        del self.files["lightning-goats-gateway"]
        self.write_archive()
        with self.assertRaisesRegex(ValueError, "missing required"):
            SMOKE.verify_and_smoke(self.archive, SOURCE)

    def test_corrupt_binary_fails_before_execution(self):
        self.write_archive(lambda: self.files.update({"lightning-goatsd": b"corrupt"}))
        with self.assertRaisesRegex(ValueError, "Checksum mismatch"):
            SMOKE.verify_and_smoke(self.archive, SOURCE)

    def test_missing_website_fails_even_with_matching_manifest(self):
        del self.files["web/site-config.js"]
        self.write_archive()
        with self.assertRaisesRegex(ValueError, "missing required"):
            SMOKE.verify_and_smoke(self.archive, SOURCE)

    def test_unlisted_payload_fails(self):
        self.write_archive(lambda: self.files.update({"extra": b"unlisted"}))
        with self.assertRaisesRegex(ValueError, "exactly the payload"):
            SMOKE.verify_and_smoke(self.archive, SOURCE)

    def test_wrong_source_commit_fails(self):
        self.write_archive()
        with self.assertRaisesRegex(ValueError, "Source commit mismatch"):
            SMOKE.verify_and_smoke(self.archive, "b" * 40)

    def test_traversal_and_symlink_fail(self):
        for name, kind in [("../escape", tarfile.REGTYPE), ("link", tarfile.SYMTYPE)]:
            with self.subTest(name=name):
                member = tarfile.TarInfo(name)
                member.type = kind
                member.linkname = "/etc/passwd" if kind == tarfile.SYMTYPE else ""
                self.write_archive(extra_member=member)
                with self.assertRaises(ValueError):
                    SMOKE.verify_and_smoke(self.archive, SOURCE)
