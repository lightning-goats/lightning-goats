"""Candidate reproducibility and fail-closed inspection with disposable files."""
import json
from pathlib import Path
import runpy
import tempfile
import unittest


TOOLS = runpy.run_path(str(Path(__file__).resolve().parents[1] / 'scripts/package-cashu.py'))
SOURCE = '1' * 40


class CashuPackageTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.directory = Path(temporary.name)
        self.payload = self.directory / 'payload'
        self.payload.mkdir()

    def write(self, name, content):
        path = self.payload / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(content)
        return TOOLS['file_sha256'](path)

    def stage_review(self):
        for name in TOOLS['PAYLOAD']:
            self.write(name, b'fixture')
        rules = {lock: TOOLS['file_sha256'](self.payload / lock) for lock in TOOLS['LOCKS']}
        archive = {'name': 'fixture', 'version': '1.0.0'}
        inventory = self.write(TOOLS['INVENTORY'], json.dumps({'lock_sha256': rules, 'packages': [archive]}).encode())
        notice = b'locked archive notice'
        checksum = self.write('notice-hash-probe', notice)
        (self.payload / 'notice-hash-probe').unlink()
        text = 'texts/' + checksum + '.txt'
        self.write('notices/' + text, notice)
        self.write('notices/index.json', json.dumps({'inventory_sha256': inventory, 'notice_files_missing': [],
                   'packages': [dict(archive, notice_files={'LICENSE': text})]}).encode())
        self.write('rust-toolchain/COPYRIGHT-library.html', b'fixture')
        self.write('rust-toolchain/licenses/MIT.txt', b'fixture')
        binary = b'\x7fELF\x02\x01\x01' + bytes(9) + b'\x03\x00\x3e\x00' + b'fixture bytes'
        info = {'source_commit': SOURCE, 'source_tree': '2'*40, 'profile': 'dev',
                'target': TOOLS['TARGET'], 'activation': 'none', 'binaries': {}}
        for name in TOOLS['BINS']:
            info['binaries'][name] = {
                'sha256': self.write(name, binary), 'cargo_archive_members': {'fixture@1.0.0': ['libfixture.rlib(f.o)']},
                'rust_toolchain_archive_members': {'libstd.rlib': ['std.o']}, 'elf_needed': ['libc.so.6'],
            }
        self.write('BUILD-INFO.json', json.dumps(info).encode())

    def test_identical_payload_repackages_byte_for_byte_and_verifies(self):
        self.stage_review()
        first, second = (self.directory / name for name in ('one.tar.gz', 'two.tar.gz'))
        TOOLS['pack'](self.payload, first, 1234)
        TOOLS['pack'](self.payload, second, 1234)
        self.assertEqual(first.read_bytes(), second.read_bytes())
        self.assertEqual(TOOLS['verify'](first, SOURCE)['binaries'], 6)
        with self.assertRaisesRegex(ValueError, 'build identity'):
            TOOLS['verify'](first, '3'*40)

    def test_a_rehashed_wrong_link_inventory_and_extra_file_refuse(self):
        self.stage_review()
        info = self.payload / 'BUILD-INFO.json'
        original = json.loads(info.read_bytes())
        damaged = json.loads(info.read_bytes())
        damaged['binaries'][TOOLS['BINS'][0]]['cargo_archive_members'] = {'unlisted@1.0.0': ['libunlisted.rlib(f.o)']}
        info.write_text(json.dumps(damaged))
        bad = self.directory / 'bad.tar.gz'
        TOOLS['pack'](self.payload, bad, 0)
        with self.assertRaisesRegex(ValueError, 'link inventory'):
            TOOLS['verify'](bad, SOURCE)
        info.write_text(json.dumps(original))
        self.write('unexpected.txt', b'unapproved payload')
        unknown = self.directory / 'unknown.tar.gz'
        TOOLS['pack'](self.payload, unknown, 0)
        with self.assertRaisesRegex(ValueError, 'unexpected files'):
            TOOLS['verify'](unknown, SOURCE)

    def test_link_map_uses_selected_members_and_rejects_unknown_owners(self):
        archive = self.directory / 'libfixture.rlib'
        different = self.directory / 'libunlisted.rlib'
        raw_map = (
            'Archive member included to satisfy reference by file (symbol)\n\n'
            f'{archive}(owner.o)\n'
            f'                              {different}(reference.o) (function)\n'
            f'\nDiscarded input sections\n  {different}(discarded.o)\n'
        )
        self.assertEqual(dict(TOOLS['selected_archives'](raw_map)), {archive: {'owner.o'}})
        with self.assertRaisesRegex(ValueError, 'no inventoried owner'):
            TOOLS['identify_links'](raw_map, [], {}, self.directory / 'sysroot', self.directory / 'elf')


if __name__ == '__main__':
    unittest.main()
