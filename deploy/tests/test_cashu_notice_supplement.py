"""Exact-source binding checks for license files omitted from crate archives."""
import copy
import hashlib
import importlib.util
import io
import json
from pathlib import Path
import tempfile
import tarfile
import unittest

SCRIPT = Path(__file__).resolve().parents[1] / 'scripts/export-cashu-notices.py'
SPEC = importlib.util.spec_from_file_location('cashu_notices', SCRIPT)
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)


class SupplementTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name).resolve()
        self.relative = 'docs/licenses/upstream/LICENSE'
        target = self.root / self.relative
        target.parent.mkdir(parents=True)
        self.data = b'fixture license\n'
        target.write_bytes(self.data)
        self.commit = 'a' * 40
        self.inventory = {'packages': [{
            'name': 'fixture', 'version': '1.0.0', 'status': 'verified_archive',
            'checksum': 'b' * 64, 'archive_repository': 'https://github.com/owner/repo.git',
            'archive_vcs_info': {'git': {'sha1': self.commit}},
        }]}
        self.raw = json.dumps(self.inventory).encode()
        self.document = {'schema_version': 1,
                         'inventory_sha256': MODULE.digest(self.raw), 'packages': [{
            'package': 'fixture@1.0.0', 'checksum': 'b' * 64, 'commit': self.commit,
            'files': [{'file': self.relative, 'sha256': MODULE.digest(self.data),
                       'git_blob_sha1': hashlib.sha1(b'blob 16\0' + self.data).hexdigest(),
                       'url': 'https://github.com/owner/repo/blob/' + self.commit + '/LICENSE'}],
        }]}
        self.path = self.root / 'supplement.json'

    def load(self, document=None, cache=None):
        self.path.write_text(json.dumps(document or self.document))
        return MODULE.load_supplement(self.root, self.raw, self.inventory, self.path, cache)

    def test_exact_commit_and_both_hashes_accept(self):
        self.assertEqual(self.load()['fixture@1.0.0'][self.relative][0], self.data)

    def test_workspace_repository_path_keeps_same_owner_and_repo(self):
        self.inventory['packages'][0]['archive_repository'] = 'https://github.com/owner/repo/tree/master/crates/fixture'
        self.raw = json.dumps(self.inventory).encode()
        self.document['inventory_sha256'] = MODULE.digest(self.raw)
        self.assertEqual(self.load()['fixture@1.0.0'][self.relative][0], self.data)

    def test_repository_credentials_query_and_other_paths_refused(self):
        for value in ['https://user@github.com/owner/repo',
                      'https://github.com/owner/repo?ref=main',
                      'https://github.com/owner/repo/blob/main/LICENSE',
                      'https://github.com/owner/../repo']:
            with self.subTest(value=value), self.assertRaises(ValueError):
                MODULE.github_repository(value)

    def test_changed_bytes_refused(self):
        (self.root / self.relative).write_bytes(b'changed')
        with self.assertRaises(ValueError):
            self.load()

    def test_rebound_inventory_archive_or_commit_refused(self):
        for key, value in [('inventory_sha256', '0' * 64),
                           ('checksum', '0' * 64), ('commit', '0' * 40)]:
            with self.subTest(key=key):
                document = copy.deepcopy(self.document)
                if key == 'inventory_sha256':
                    document[key] = value
                else:
                    document['packages'][0][key] = value
                with self.assertRaises(ValueError):
                    self.load(document)

    def test_other_repository_or_current_branch_refused(self):
        for url in ['https://github.com/other/repo/blob/' + self.commit + '/LICENSE',
                    'https://github.com/owner/repo/blob/main/LICENSE']:
            document = copy.deepcopy(self.document)
            document['packages'][0]['files'][0]['url'] = url
            with self.assertRaises(ValueError):
                self.load(document)

    def test_duplicate_package_refused(self):
        self.document['packages'] *= 2
        with self.assertRaises(ValueError):
            self.load()

    def test_local_cdk_requires_verified_archive_patch_and_manifest(self):
        root = SCRIPT.parents[2]
        inventory = json.loads((root / 'docs/testing/cashu-license-inventory-20261116.json').read_text())
        package = next(p for p in inventory['packages'] if p['name'] == 'cdk')
        commit, repository, checksum = MODULE.source_origin(root, package)
        self.assertEqual(commit, 'b2035a3525c98b22226926bdc20fa128a1351ae5')
        self.assertEqual(repository, 'https://github.com/cashubtc/cdk')
        self.assertEqual(checksum, '6969a2ea499035f5f49f5cdaf355f34251ce85a8ddd47c557177ecee5ed7da2a')
        for key, value in [('manifest_sha256', '0' * 64), ('manifest', 'Cargo.toml')]:
            changed = dict(package, **{key: value})
            with self.subTest(key=key), self.assertRaises(ValueError):
                MODULE.source_origin(root, changed)

    def test_arbitrary_local_package_has_no_implicit_upstream_authority(self):
        self.inventory['packages'][0]['status'] = 'local_manifest'
        self.raw = json.dumps(self.inventory).encode()
        self.document['inventory_sha256'] = MODULE.digest(self.raw)
        with self.assertRaises(ValueError):
            self.load()

    def test_path_escape_and_symlink_refused(self):
        document = copy.deepcopy(self.document)
        document['packages'][0]['files'][0]['file'] = 'docs/licenses/upstream/../../../supplement.json'
        with self.assertRaises(ValueError):
            self.load(document)
        target = self.root / self.relative
        target.unlink()
        target.symlink_to(self.root / 'elsewhere')
        (self.root / 'elsewhere').write_bytes(self.data)
        with self.assertRaises(ValueError):
            self.load()

    def test_signed_release_record_rechecks_complete_archive_payload(self):
        cache = self.root / 'cache'
        (cache / 'registry').mkdir(parents=True)
        archive = cache / 'registry/fixture-1.0.0.crate'
        payload = {'Cargo.toml.orig': b'tracked manifest', 'src/lib.rs': b'archive source'}
        with tarfile.open(archive, 'w:gz') as bundle:
            for name, data in {**payload, 'Cargo.toml': b'Cargo generated this file'}.items():
                member = tarfile.TarInfo('fixture-1.0.0/' + name)
                member.size = len(data)
                bundle.addfile(member, io.BytesIO(data))
        package = dict(self.inventory['packages'][0], archive_vcs_info=None,
                       checksum=MODULE.digest(archive.read_bytes()))
        row = {'commit': self.commit, 'release_source': {
            'tag': 'fixture-1.0.0', 'git_commit': self.commit, 'tag_object': 'c' * 40,
            'git_tree': 'd' * 40,
            'member_git_blobs': {name: MODULE.git_blob(data) for name, data in payload.items()},
        }}
        self.assertEqual(MODULE.release_origin(package, row, cache)[0], self.commit)
        incomplete = copy.deepcopy(row)
        del incomplete['release_source']['member_git_blobs']['src/lib.rs']
        with self.assertRaisesRegex(ValueError, 'does not match'):
            MODULE.release_origin(package, incomplete, cache)
        different = dict(row, commit='e' * 40)
        with self.assertRaises(ValueError):
            MODULE.release_origin(package, different, cache)
        with self.assertRaises(ValueError):
            MODULE.release_origin(dict(package, archive_vcs_info={'git': {}}), row, cache)
        workspace = copy.deepcopy(row)
        workspace['release_source']['tag'] = 'v0.2.8'
        with self.assertRaises(ValueError):
            MODULE.release_origin(package, workspace, cache)
        ldk = dict(package, archive_repository='https://github.com/lightningdevkit/rust-lightning/')
        self.assertEqual(MODULE.release_origin(ldk, workspace, cache)[0], self.commit)
        workspace['release_source']['member_git_blobs']['src/lib.rs'] = 'f' * 40
        with self.assertRaisesRegex(ValueError, 'does not match'):
            MODULE.release_origin(ldk, workspace, cache)

    def test_catalog_text_requires_the_verified_archive_declaration_and_exact_blob(self):
        cache = self.root / 'cache'
        (cache / 'registry').mkdir(parents=True)
        archive = cache / 'registry/fixture-1.0.0.crate'
        data = b'[package]\nname="fixture"\nversion="1.0.0"\nlicense="MITNFA"\n'
        with tarfile.open(archive, 'w:gz') as bundle:
            member = tarfile.TarInfo('fixture-1.0.0/Cargo.toml')
            member.size = len(data)
            bundle.addfile(member, io.BytesIO(data))
        package = self.inventory['packages'][0]
        package.update(checksum=MODULE.digest(archive.read_bytes()), declared_license='MITNFA',
                       license_notice_files_sha256={})
        self.raw = json.dumps(self.inventory).encode()
        self.document['inventory_sha256'] = MODULE.digest(self.raw)
        row = self.document['packages'][0]
        row['checksum'] = package['checksum']
        digest = 'c' * 40
        row['catalog_source'] = {'license_id': 'MITNFA', 'catalog_commit': digest}
        row['files'][0]['url'] = 'https://github.com/spdx/license-list-data/blob/' + digest + '/text/MITNFA.txt'
        self.assertEqual(self.load(cache=cache)['fixture@1.0.0'][self.relative][1]['catalog_license_id'],
                         'MITNFA')
        for field, value in [('url', row['files'][0]['url'].replace('MITNFA', 'MIT')),
                             ('git_blob_sha1', 'e' * 40), ('catalog_license_id', 'MIT')]:
            incorrect = copy.deepcopy(self.document)
            incorrect['packages'][0]['files'][0][field] = value
            with self.subTest(field=field), self.assertRaises(ValueError):
                self.load(incorrect, cache)
        with self.assertRaises(ValueError):
            self.load()
        wrong = dict(package, declared_license='MIT OR Apache-2.0')
        claim = dict(row, catalog_source={'license_id': 'Apache-2.0', 'catalog_commit': digest})
        with self.assertRaisesRegex(ValueError, 'verified archive declaration'):
            MODULE.catalog_notice_url(wrong, claim, cache)


if __name__ == '__main__':
    unittest.main()
