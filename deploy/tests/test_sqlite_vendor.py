"""The native backport may change one dependency declaration, never driver code."""
import importlib.util
from pathlib import Path
import shutil
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location("sqlite_vendor", ROOT / "deploy/scripts/verify-sqlite-vendor.py")
VENDOR = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(VENDOR)


class SQLiteVendorTests(unittest.TestCase):
    def setUp(self):
        self.files = VENDOR.payload(ROOT)

    def test_exact_published_driver_manifest_backport_and_both_licenses(self):
        result = VENDOR.verify_payload(self.files)
        self.assertEqual(result["upstream_files"], 56)
        self.assertTrue(result["manifest_only_patch"])
        self.assertIn("vendor/sqlx-sqlite/LICENSE-MIT", self.files)
        self.assertIn("vendor/sqlx-sqlite/LICENSE-APACHE", self.files)

    def test_changed_source_license_manifest_or_origin_archive_is_refused(self):
        for path in ["vendor/sqlx-sqlite/src/lib.rs", "vendor/sqlx-sqlite/LICENSE-MIT",
                     "vendor/sqlx-sqlite/Cargo.toml", VENDOR.UPSTREAM]:
            with self.subTest(path=path):
                changed = dict(self.files)
                changed[path] += b"\n"
                with self.assertRaises(ValueError):
                    VENDOR.verify_payload(changed)

    def test_missing_extra_source_and_unbound_application_or_lock_are_refused(self):
        changed = dict(self.files)
        del changed["vendor/sqlx-sqlite/src/lib.rs"]
        with self.assertRaises(ValueError):
            VENDOR.verify_payload(changed)
        changed = dict(self.files)
        changed["vendor/sqlx-sqlite/src/unreviewed.rs"] = b"unreviewed"
        with self.assertRaises(ValueError):
            VENDOR.verify_payload(changed)
        for path, before, after in [
            ("Cargo.toml", b'rust-version = "1.88"', b'rust-version = "1.94"'),
            ("Cargo.toml", b'vendor/sqlx-sqlite', b'vendor/elsewhere'),
            ("Cargo.lock", b'name = "libsqlite3-sys"\nversion = "0.39.1"', b'name = "libsqlite3-sys"\nversion = "0.30.1"'),
            ("Cargo.lock", VENDOR.BINDING_SHA256.encode(), b"0" * 64),
        ]:
            with self.subTest(path=path, before=before):
                self.assertIn(before, self.files[path])
                changed = dict(self.files)
                changed[path] = changed[path].replace(before, after)
                with self.assertRaises(ValueError):
                    VENDOR.verify_payload(changed)

    def test_filesystem_symlink_cannot_substitute_reviewed_source(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            shutil.copytree(ROOT / "vendor", root / "vendor")
            for name in ["Cargo.toml", "Cargo.lock"]:
                shutil.copyfile(ROOT / name, root / name)
            original = root / "vendor/sqlx-sqlite/src/lib.rs"
            original.unlink()
            original.symlink_to(ROOT / "vendor/sqlx-sqlite/src/lib.rs")
            with self.assertRaises(ValueError):
                VENDOR.payload(root)


if __name__ == "__main__":
    unittest.main()
