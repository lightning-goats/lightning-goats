"""Offline restore failure boundaries; actual process recovery is a Rust test."""
import hashlib
import json
import os
from pathlib import Path
import sqlite3
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
SCRIPT = ROOT / "deploy/scripts/restore-stores.py"


class RestoreTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name)
        self.source = self.root / "snapshot"
        self.source.mkdir()
        with sqlite3.connect(self.source / "daemon.db") as db:
            # The daemon creates these snapshots with bundled SQLite >= 3.38.
            # Ubuntu 22.04 Python uses SQLite 3.37; provide only the fixture
            # clock while constructing its schema. Restore executes no clock SQL.
            if sqlite3.sqlite_version_info < (3, 38, 0):
                db.create_function("unixepoch", 0, lambda: 1_700_000_000)
            for migration in sorted((ROOT / "migrations").glob("*.sql")):
                db.executescript(migration.read_text())
            db.execute("INSERT INTO overlay_identity VALUES(1, '11111111-1111-4111-8111-111111111111')")
        with sqlite3.connect(self.source / "gateway.db") as db:
            for name in ["feeder_requests", "feeder_request_events", "feeder_refusals", "weather_high_water"]:
                db.execute(f"CREATE TABLE {name}(id INTEGER)")
        self.destination = self.root / "restored"

    def invoke(self, attest=True):
        command = ["python3", str(SCRIPT), "--source-directory", str(self.source),
                   "--destination-directory", str(self.destination)]
        if attest:
            command.append("--writers-stopped")
        return subprocess.run(command, capture_output=True, text=True, check=False)

    def test_requires_explicit_quiescence_and_never_overwrites(self):
        self.assertNotEqual(self.invoke(False).returncode, 0)
        self.assertFalse(self.destination.exists())
        self.destination.mkdir()
        sentinel = self.destination / "keep"
        sentinel.write_text("unrelated work")
        self.assertNotEqual(self.invoke().returncode, 0)
        self.assertEqual(sentinel.read_text(), "unrelated work")
        self.assertEqual(list(self.destination.iterdir()), [sentinel])

    def test_rotates_only_destination_identity_and_records_private_hashes(self):
        before = {p.name: hashlib.sha256(p.read_bytes()).hexdigest() for p in self.source.glob("*.db")}
        result = self.invoke()
        self.assertEqual(result.returncode, 0, result.stderr)
        manifest = json.loads(result.stdout)
        self.assertNotEqual(manifest["overlay_stream_id"], "11111111-1111-4111-8111-111111111111")
        self.assertFalse((self.destination / "INCOMPLETE").exists())
        self.assertEqual(os.stat(self.destination).st_mode & 0o777, 0o700)
        for name in ["daemon.db", "gateway.db", "RESTORE.json"]:
            self.assertEqual(os.stat(self.destination / name).st_mode & 0o777, 0o600)
        for name, digest in manifest["files"].items():
            self.assertEqual(hashlib.sha256((self.destination / name).read_bytes()).hexdigest(), digest)
        self.assertEqual(before, {p.name: hashlib.sha256(p.read_bytes()).hexdigest() for p in self.source.glob("*.db")})

    def test_invalid_second_store_leaves_incomplete_marker(self):
        (self.source / "gateway.db").write_bytes(b"not sqlite")
        self.assertNotEqual(self.invoke().returncode, 0)
        self.assertTrue((self.destination / "INCOMPLETE").is_file())
        self.assertFalse((self.destination / "RESTORE.json").exists())
