"""Safety guards for fresh inactive installation; no host account/file mutation."""
import hashlib
import importlib.util
import io
import json
import subprocess
from pathlib import Path
import tarfile
import tempfile
import unittest
from unittest.mock import patch

SPEC = importlib.util.spec_from_file_location(
    "prepare_canary", Path(__file__).resolve().parents[1] / "scripts/prepare-vps-canary.py"
)
PREPARE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(PREPARE)
SOURCE = "a" * 40


class PrepareCanaryTests(unittest.TestCase):
    def test_sudo_denial_accepts_observed_listing_statuses_only(self):
        denial = "User lightning-goats is not allowed to run sudo on staging.example.\n"
        for status in [0, 1]:
            with self.subTest(status=status), patch.object(PREPARE, "command", return_value=subprocess.CompletedProcess([], status, denial, "")):
                PREPARE.require_no_sudo()

    def test_sudo_grants_errors_and_misleading_denial_are_rejected(self):
        denial = "User lightning-goats is not allowed to run sudo on staging.example.\n"
        for status, stdout, stderr in [
            (0, "User lightning-goats may run the following commands:\n    (ALL) NOPASSWD: ALL\n", ""),
            (1, "", ""), (0, "", ""), (2, denial, ""),
            (1, "", denial), (0, denial, "sudo: policy plugin failed\n"),
            (0, denial + "    (ALL) NOPASSWD: ALL\n", ""),
            (0, denial.replace("lightning-goats", "another-user"), ""),
        ]:
            with self.subTest(status=status, stdout=stdout, stderr=stderr), patch.object(PREPARE, "command", return_value=subprocess.CompletedProcess([], status, stdout, stderr)):
                with self.assertRaisesRegex(ValueError, "no sudo authority"):
                    PREPARE.require_no_sudo()

    def archive(self, path):
        files = {name: b"inert fixture\n" for name in PREPARE.RELEASE.REQUIRED | PREPARE.FILES.keys()}
        files["BUILD-INFO.txt"] = f"source_commit={SOURCE}\n".encode()
        manifest = "".join(f"{hashlib.sha256(value).hexdigest()}  ./{name}\n"
                           for name, value in sorted(files.items())).encode()
        with tarfile.open(path, "w:gz") as bundle:
            for name, value in {**files, "SHA256SUMS": manifest}.items():
                item = tarfile.TarInfo("./" + name)
                item.size = len(value)
                bundle.addfile(item, io.BytesIO(value))
        return hashlib.sha256(path.read_bytes()).hexdigest()

    def test_bad_digest_or_source_fails_before_host_checks_or_mutation(self):
        with tempfile.TemporaryDirectory() as directory:
            archive = Path(directory) / "release.tar.gz"
            digest = self.archive(archive)
            for source, checksum in [(SOURCE, "0" * 64), ("b" * 40, digest)]:
                with self.subTest(source=source), patch.object(PREPARE, "preflight") as preflight, patch.object(PREPARE, "apply_install") as apply:
                    with self.assertRaises(ValueError):
                        PREPARE.prepare(archive, source, checksum)
                    preflight.assert_not_called()
                    apply.assert_not_called()

    def test_default_plan_never_executes_payload_or_installs(self):
        with tempfile.TemporaryDirectory() as directory:
            archive = Path(directory) / "release.tar.gz"
            digest = self.archive(archive)
            with patch.object(PREPARE, "preflight"), patch.object(PREPARE, "apply_install") as apply, patch.object(PREPARE, "command") as command:
                plan = PREPARE.prepare(archive, SOURCE, digest)
                self.assertFalse(plan["applied"])
                self.assertEqual(plan["source_commit"], SOURCE)
                self.assertEqual(len(plan["files"]), 4)
                self.assertNotIn("/usr/local/bin/lightning-goats-gateway", plan["files"])
                json.dumps(plan)
                apply.assert_not_called()
                command.assert_not_called()

    def test_existing_and_dangling_paths_are_never_overwritten(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source, destination = root / "source", root / "destination"
            source.write_text("new bytes")
            destination.write_text("preserve existing bytes")
            with self.assertRaises(ValueError):
                PREPARE.require_fresh([destination])
            with self.assertRaises(FileExistsError):
                PREPARE.install_new_file(source, destination)
            self.assertEqual(destination.read_text(), "preserve existing bytes")
            link = root / "link"
            link.symlink_to(root / "absent")
            with self.assertRaises(ValueError):
                PREPARE.require_fresh([link])
            self.assertTrue(link.is_symlink())
            self.assertFalse((root / "absent").exists())

    def test_symlink_parent_is_rejected(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "actual").mkdir()
            (root / "link").symlink_to(root / "actual")
            with self.assertRaises(ValueError):
                PREPARE.require_trusted_parents(root / "link" / "file")
            self.assertEqual(list((root / "actual").iterdir()), [])

    def test_apply_without_root_fails_before_reading_archive(self):
        with patch.object(PREPARE.os, "geteuid", return_value=1000), patch.object(PREPARE, "copy_verified_archive") as read:
            with self.assertRaisesRegex(ValueError, "root staging privileges"):
                PREPARE.prepare(Path("missing"), SOURCE, "0" * 64, apply=True)
            read.assert_not_called()


if __name__ == "__main__":
    unittest.main()
