"""Test source selection in the core release packager."""
import os
from pathlib import Path, PurePosixPath
import shutil
import subprocess
import tarfile
import tempfile
import unittest


PACKAGER = Path(__file__).resolve().parents[1] / "scripts/package-release.sh"


class PackageReleaseTests(unittest.TestCase):
    def test_ignored_local_files_cannot_change_release_archive(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            repo = root / "repo"
            repo.mkdir()
            for directory in ("deploy/scripts", "docs", "web", "migrations", "vendor"):
                (repo / directory).mkdir(parents=True)
                (repo / directory / "tracked.txt").write_text("committed input\n")
            (repo / "deploy/scripts/verify-sqlite-vendor.py").write_text("# fixture verifier\n")
            for name in ("AGENTS.md", "Cargo.lock", "Cargo.toml"):
                (repo / name).write_text("committed input\n")
            (repo / ".gitignore").write_text("__pycache__/\n/deploy/local-draft.json\n")
            shutil.copyfile(PACKAGER, repo / "deploy/scripts/package-release.sh")
            subprocess.run(["git", "init", "-q"], cwd=repo, check=True)
            subprocess.run(["git", "add", "."], cwd=repo, check=True)
            subprocess.run(["git", "-c", "user.name=Fixture", "-c", "user.email=fixture@example.invalid",
                            "commit", "-qm", "Fixture"], cwd=repo, check=True)

            binaries = root / "bin"
            binaries.mkdir()
            for name in ("lightning-goatsd", "lightning-goatsctl", "lightning-goats-gateway", "rustc", "cargo"):
                program = binaries / name
                program.write_text("#!/bin/sh\nprintf 'fixture version\\n'\n")
                program.chmod(0o755)
            env = {**os.environ, "PATH": str(binaries) + os.pathsep + os.environ["PATH"]}

            def package(output):
                result = subprocess.run(["bash", str(repo / "deploy/scripts/package-release.sh"),
                                         str(binaries), str(root / output), "fixture"],
                                        env=env, check=True, capture_output=True, text=True, timeout=15)
                return Path(result.stdout.strip())

            baseline = package("first")
            (repo / "deploy/scripts/__pycache__").mkdir()
            ignored = ("deploy/scripts/__pycache__/fixture.pyc", "deploy/local-draft.json")
            for name in ignored:
                (repo / name).write_text("untracked fixture artifact\n")
            repeated = package("second")
            with tarfile.open(repeated, "r:gz") as archive:
                members = {PurePosixPath(name).as_posix() for name in archive.getnames()}
            self.assertIn("deploy/scripts/tracked.txt", members)
            self.assertTrue(members.isdisjoint(ignored))
            self.assertEqual(baseline.read_bytes(), repeated.read_bytes())
