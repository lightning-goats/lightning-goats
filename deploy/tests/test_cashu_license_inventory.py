"""Archive regressions for permission notices stored in AUTHORS."""
import hashlib
import importlib.util
import io
from pathlib import Path
import tarfile
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location(
    'cashu_license_inventory', ROOT / 'deploy/scripts/inventory-cashu-licenses.py')
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)


class InventoryTests(unittest.TestCase):
    def record(self, authors):
        with tempfile.TemporaryDirectory() as temporary:
            cache = Path(temporary)
            (cache / 'registry').mkdir()
            archive = cache / 'registry/fixture-1.0.0.crate'
            with tarfile.open(archive, 'w:gz') as bundle:
                for name, data in {
                    'Cargo.toml': b'[package]\nname="fixture"\nversion="1.0.0"\nlicense="MIT"\n',
                    'AUTHORS': authors,
                }.items():
                    entry = tarfile.TarInfo('fixture-1.0.0/' + name)
                    entry.size = len(data)
                    bundle.addfile(entry, io.BytesIO(data))
            checksum = MODULE.digest(archive.read_bytes())
            return MODULE.registry_record(
                {'name': 'fixture', 'version': '1.0.0', 'checksum': checksum}, cache)

    def test_actual_upstream_authors_permission_notice_is_inventoried(self):
        # Exact r-efi 5.3.0 source commit's AUTHORS, not a current-branch license:
        # github.com/r-efi/r-efi/blob/97b55bed1c2c91dcbf787674849f05337ff80b33/AUTHORS
        data = (ROOT / 'deploy/tests/fixtures/r-efi-5.3.0-AUTHORS').read_bytes()
        self.assertEqual(hashlib.sha1(b'blob ' + str(len(data)).encode() + b'\0' + data).hexdigest(),
                         '7397b2bd6a8580d51fe7b23783f8f1a05fa70291')
        self.assertEqual(self.record(data)['license_notice_files_sha256'],
                         {'AUTHORS': MODULE.digest(data)})

    def test_contributor_list_alone_is_not_a_permission_notice(self):
        self.assertEqual(self.record(b'AUTHORS:\nExample Contributor\n')['license_notice_files_sha256'], {})

    def test_permission_reference_without_copyright_or_disclaimer_does_not_qualify(self):
        self.assertEqual(self.record(b'Permission is hereby granted, free of charge\n')['license_notice_files_sha256'], {})


if __name__ == '__main__':
    unittest.main()
