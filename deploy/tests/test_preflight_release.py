"""Pin external archive bytes before checking its self-described payload."""
import hashlib
import importlib.util
import os
from pathlib import Path
import subprocess
import sys
import unittest
from unittest.mock import patch

import test_release

SOURCE = test_release.SOURCE

SPEC = importlib.util.spec_from_file_location(
    "preflight", Path(__file__).resolve().parents[1] / "scripts/preflight-release.py")
PREFLIGHT = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(PREFLIGHT)


class PreflightTests(unittest.TestCase):
    setUp = test_release.ReleaseArchiveTests.setUp
    write_archive = test_release.ReleaseArchiveTests.write_archive

    def test_non_regular_inputs_fail_promptly_without_following_links(self):
        self.write_archive()
        digest = hashlib.sha256(self.archive.read_bytes()).hexdigest()
        link = self.archive.with_name("link")
        link.symlink_to(self.archive)
        fifo = self.archive.with_name("fifo")
        os.mkfifo(fifo)
        for path in [fifo, link, Path("/dev/null"), self.archive.parent]:
            with self.subTest(path=path.name):
                result = subprocess.run(
                    [sys.executable, str(Path(PREFLIGHT.__file__)), str(path), SOURCE, digest],
                    capture_output=True, text=True, timeout=2)
                self.assertNotEqual(result.returncode, 0)
                self.assertIn("regular file", result.stderr)
                self.assertEqual(result.stdout, "")

    def test_path_replacement_cannot_change_checked_inode(self):
        self.write_archive()
        digest = hashlib.sha256(self.archive.read_bytes()).hexdigest()
        original_fstat = os.fstat

        def replace_after_inspection(descriptor):
            result = original_fstat(descriptor)
            self.archive.rename(self.archive.with_name("original"))
            self.archive.symlink_to("/dev/zero")
            return result

        with patch.object(PREFLIGHT.os, "fstat", side_effect=replace_after_inspection):
            report = PREFLIGHT.preflight(self.archive, SOURCE, digest)
        self.assertEqual(report["archive_sha256"], digest)

    def test_pinned_payload_is_reported_without_execution(self):
        self.files["lightning-goatsd"] = b"not executable code"
        self.write_archive()
        digest = hashlib.sha256(self.archive.read_bytes()).hexdigest()
        with patch("subprocess.run", side_effect=AssertionError("must not execute")):
            report = PREFLIGHT.preflight(self.archive, SOURCE, digest)
        self.assertFalse(report["payload_executed"])
        self.assertFalse(report["installed"])
        self.assertFalse(report["activated"])
        self.assertEqual(report["payload_sha256"]["lightning-goatsd"],
                         hashlib.sha256(self.files["lightning-goatsd"]).hexdigest())

    def test_outer_digest_rejects_self_consistent_replacement_before_extract(self):
        self.write_archive()
        digest = hashlib.sha256(self.archive.read_bytes()).hexdigest()
        self.files["lightning-goatsd"] = b"replacement with matching internal checksum"
        self.write_archive()
        with patch.object(PREFLIGHT.RELEASE, "verify_archive", side_effect=AssertionError("too early")):
            with self.assertRaisesRegex(ValueError, "Outer archive checksum mismatch"):
                PREFLIGHT.preflight(self.archive, SOURCE, digest)

    def test_matching_outer_digest_does_not_override_source_mismatch(self):
        self.write_archive()
        digest = hashlib.sha256(self.archive.read_bytes()).hexdigest()
        with self.assertRaisesRegex(ValueError, "Source commit mismatch"):
            PREFLIGHT.preflight(self.archive, "b" * 40, digest)

    def test_compressed_size_limit(self):
        self.write_archive()
        with patch.object(PREFLIGHT, "MAX_ARCHIVE_BYTES", 1):
            with self.assertRaisesRegex(ValueError, "size budget"):
                PREFLIGHT.preflight(self.archive, SOURCE, "0" * 64)
