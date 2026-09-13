"""Exercise interrupted replacement and restore with real disposable files."""
import importlib.util
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

SCRIPT = Path(__file__).resolve().parents[1] / "scripts/inactive_file_transaction.py"
SPEC = importlib.util.spec_from_file_location("transaction", SCRIPT)
TX = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(TX)


class TransactionTests(unittest.TestCase):
    def setUp(self):
        temp = tempfile.TemporaryDirectory()
        self.addCleanup(temp.cleanup)
        self.root = Path(temp.name)
        self.directory = self.root / "transaction"
        self.targets = [self.root / str(i) for i in range(3)]
        self.sources = [self.root / f"new{i}" for i in range(3)]
        for i, (target, source) in enumerate(zip(self.targets, self.sources)):
            target.write_bytes(f"old{i}".encode())
            target.chmod(0o640)
            os.setxattr(target, "user.lg-test", b"preserved")
            source.write_bytes(f"new{i}".encode())
        self.rows = [(p, s, TX.fingerprint(p)) for p, s in zip(self.targets, self.sources)]
        self.guard = lambda: None

    def prepare(self):
        TX.prepare(self.directory, self.rows, self.guard)

    def test_apply_and_rollback_preserve_metadata_and_unrelated_state(self):
        sentinel = self.root / "ledger.db"
        sentinel.write_bytes(b"never modify this state")
        self.prepare()
        result = TX.execute(self.directory, self.guard)
        self.assertFalse(result["services_activated"])
        self.assertEqual([p.read_bytes() for p in self.targets], [p.read_bytes() for p in self.sources])
        TX.execute(self.directory, self.guard, rollback=True)
        for target, _, expected in self.rows:
            self.assertEqual(TX.fingerprint(target), expected)
        self.assertEqual(sentinel.read_bytes(), b"never modify this state")

    def test_process_death_after_first_rename_can_resume_then_rollback(self):
        self.prepare()
        program = "import importlib.util,os,sys; s=importlib.util.spec_from_file_location('t',sys.argv[1]); m=importlib.util.module_from_spec(s); s.loader.exec_module(m); m.execute(sys.argv[2],lambda:None,after_replace=lambda i:os._exit(23))"
        result = subprocess.run([sys.executable, "-c", program, str(SCRIPT), str(self.directory)], timeout=5)
        self.assertEqual(result.returncode, 23)
        self.assertEqual(self.targets[0].read_bytes(), b"new0")
        self.assertEqual(self.targets[1].read_bytes(), b"old1")
        TX.execute(self.directory, self.guard)
        TX.execute(self.directory, self.guard, rollback=True)
        self.assertEqual([p.read_bytes() for p in self.targets], [b"old0", b"old1", b"old2"])

    def test_drift_or_corrupt_backup_prevents_any_replacement(self):
        self.prepare()
        self.targets[2].write_bytes(b"operator change")
        with self.assertRaisesRegex(ValueError, "drift"):
            TX.execute(self.directory, self.guard)
        self.assertEqual(self.targets[0].read_bytes(), b"old0")
        self.targets[2].write_bytes(b"old2")
        (self.directory / "2.old").write_bytes(b"corrupted backup")
        with self.assertRaisesRegex(ValueError, "corruption"):
            TX.execute(self.directory, self.guard)
        self.assertEqual(self.targets[0].read_bytes(), b"old0")

    def test_guard_change_stops_before_next_replacement_and_allows_rollback(self):
        self.prepare()
        inactive = [True]
        def guard():
            if not inactive[0]:
                raise ValueError("service no longer inactive")
        with self.assertRaisesRegex(ValueError, "no longer inactive"):
            TX.execute(self.directory, guard, after_replace=lambda _: inactive.__setitem__(0, False))
        self.assertEqual(self.targets[1].read_bytes(), b"old1")
        TX.execute(self.directory, self.guard, rollback=True)
        self.assertEqual(self.targets[0].read_bytes(), b"old0")

    def test_existing_transaction_and_concurrent_executor_are_rejected(self):
        self.prepare()
        with self.assertRaises(FileExistsError):
            self.prepare()
        with TX.locked(self.directory):
            with self.assertRaises(BlockingIOError):
                TX.execute(self.directory, self.guard)
        self.assertEqual(self.targets[0].read_bytes(), b"old0")
