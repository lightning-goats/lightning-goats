"""Negative checks for the financial storage dependency's provenance gate."""
from pathlib import Path
import shutil
import tempfile
import unittest

from verify_vendor import ROOT, verify


class VendorTests(unittest.TestCase):
    def test_pristine(self):
        verify()

    def test_source_and_inventory_tampering(self):
        for change in ("source", "extra", "missing", "symlink", "binding", "cdk"):
            with self.subTest(change=change), tempfile.TemporaryDirectory() as temporary:
                root = Path(temporary) / "probe"
                shutil.copytree(ROOT / "vendor", root / "vendor")
                for name in ("Cargo.toml", "Cargo.lock"):
                    shutil.copyfile(ROOT / name, root / name)
                source = root / "vendor/rusqlite-0.31.0"
                if change == "source":
                    with (source / "src/lib.rs").open("ab") as output:
                        output.write(b"\n// modified\n")
                elif change == "extra":
                    (source / "extra.rs").write_text("unreviewed")
                elif change == "missing":
                    (source / "LICENSE").unlink()
                elif change == "symlink":
                    target = source / "LICENSE"
                    target.unlink()
                    target.symlink_to(source / "README.md")
                elif change == "cdk":
                    with (root / "vendor/cdk-0.18.3/src/wallet/mod.rs").open("ab") as output:
                        output.write(b"\n// unreviewed runtime change\n")
                else:
                    lock = root / "Cargo.lock"
                    lock.write_text(lock.read_text().replace('version = "0.39.1"', 'version = "0.28.0"'))
                with self.assertRaises(ValueError):
                    verify(root)


if __name__ == "__main__":
    unittest.main()
