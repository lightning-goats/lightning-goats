"""Refuse changed or aliased SDK code before privileged fixture copying."""
import hashlib
import json
import os
from pathlib import Path
import runpy
import tempfile
import unittest

MODULE = runpy.run_path(str(Path(__file__).resolve().parents[2] /
                           'tools/cashu-client-compat/copy-prepared.py'))


class PreparedSdkTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name).resolve()
        self.source = self.root / 'prepared'
        self.source.mkdir()
        self.lock = json.dumps([{'name': '@cashu/cashu-ts', 'version': 'fixture'}]).encode()
        package = self.source / 'node_modules/@cashu/cashu-ts'
        package.mkdir(parents=True)
        (package / 'package.json').write_text(json.dumps({'name': '@cashu/cashu-ts', 'version': 'fixture'}))
        (package / 'index.js').write_text('// inert synthetic fixture\n')
        (self.source / 'package-lock-evidence.json').write_bytes(self.lock)
        files = {str(p.relative_to(self.source)): hashlib.sha256(p.read_bytes()).hexdigest()
                 for p in (self.source / 'node_modules').rglob('*') if p.is_file()}
        raw = json.dumps({'version': 1, 'lock_sha256': hashlib.sha256(self.lock).hexdigest(),
                          'files': files}).encode()
        (self.source / 'package-files.json').write_bytes(raw)
        self.expected = hashlib.sha256(raw).hexdigest()

    def copy(self, **kwargs):
        return MODULE['copy_prepared'](kwargs.get('source', self.source), self.root / 'copied',
                                       kwargs.get('expected', self.expected),
                                       kwargs.get('lock', self.lock))

    def test_pinned_inventory_copies_only_verified_read_only_files(self):
        result = self.copy()
        self.assertEqual(result['files'], 2)
        for path in (self.root / 'copied').rglob('*'):
            if path.is_file():
                self.assertEqual(path.stat().st_mode & 0o777, 0o444)

    def test_changed_file_is_refused(self):
        (self.source / 'node_modules/@cashu/cashu-ts/index.js').write_text('// changed\n')
        with self.assertRaises(ValueError):
            self.copy()

    def test_wrong_inventory_or_lock_is_refused(self):
        for kwargs in [{'expected': '0' * 64}, {'lock': b'[]'}]:
            with self.subTest(kwargs=kwargs), self.assertRaises(ValueError):
                self.copy(**kwargs)
        self.assertFalse((self.root / 'copied').exists())

    def test_unlisted_file_is_refused(self):
        (self.source / 'unlisted.js').write_text('// unknown\n')
        with self.assertRaises(ValueError):
            self.copy()

    def test_alias_and_special_file_are_refused(self):
        alias = self.source / 'alias'
        alias.symlink_to('node_modules')
        with self.assertRaises(ValueError):
            self.copy()
        alias.unlink()
        os.mkfifo(alias)
        with self.assertRaises(ValueError):
            self.copy()

    def test_root_alias_is_refused(self):
        alias = self.root / 'alias'
        alias.symlink_to(self.source, target_is_directory=True)
        with self.assertRaises(ValueError):
            self.copy(source=alias)


if __name__ == '__main__':
    unittest.main()
