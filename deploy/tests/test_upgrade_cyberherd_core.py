"""Paid-core transaction fixtures. No real unit, database parser or network.

The root cases use real protected /run files and actual file replacement;
systemd/native acceptance is simulated. Native schema/UID migration has its own
required Rust fixture and must not be claimed as installed systemd acceptance.
"""
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

SCRIPTS = Path(__file__).resolve().parents[1] / 'scripts'
SPEC_TX = importlib.util.spec_from_file_location('inactive_file_transaction', SCRIPTS / 'inactive_file_transaction.py')
TX = importlib.util.module_from_spec(SPEC_TX)
SPEC_TX.loader.exec_module(TX)
with patch.dict(sys.modules, {'inactive_file_transaction': TX}):
    SPEC = importlib.util.spec_from_file_location('paid_core_upgrade', SCRIPTS / 'upgrade-cyberherd-core.py')
    CORE = importlib.util.module_from_spec(SPEC)
    SPEC.loader.exec_module(CORE)


class PaidCoreBoundaries(unittest.TestCase):
    def test_direct_execution_refuses_before_local_imports(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            shutil.copyfile(SCRIPTS / 'upgrade-cyberherd-core.py', root / 'upgrade-cyberherd-core.py')
            (root / 'inactive_file_transaction.py').write_text('raise RuntimeError("INJECTED")')
            result = subprocess.run(['/usr/bin/python3', '-I', '-S', '-B', str(root / 'upgrade-cyberherd-core.py'), '--help'],
                                    capture_output=True, text=True, timeout=10)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn('externally verified root-owned tool launcher', result.stderr)
            self.assertNotIn('INJECTED', result.stderr)

    def test_required_attestations_refuse_before_host_inspection(self):
        with patch.object(CORE, 'load', side_effect=AssertionError('host reached')):
            with self.assertRaisesRegex(ValueError, 'all other'):
                CORE.apply(Path('/missing'), False)
            with self.assertRaisesRegex(ValueError, 'unit-restoration'):
                CORE.finish(Path('/missing'), False)

    def test_subprocess_environment_excludes_credentials_and_errors_do_not_echo_output(self):
        fake = subprocess.CompletedProcess([], 1, 'PRIVATE_STATUS', 'PRIVATE_KEY')
        with patch.dict(os.environ, {'CREDENTIALS_DIRECTORY': '/secret', 'LD_PRELOAD': '/secret', 'HTTPS_PROXY': 'secret'}), patch.object(CORE.subprocess, 'run', return_value=fake) as call:
            with self.assertRaises(ValueError) as failure:
                CORE.command('/bin/false')
            self.assertNotIn('PRIVATE', str(failure.exception))
            environment = call.call_args.kwargs['env']
            self.assertEqual(set(environment), {'PATH', 'LC_ALL'})


@unittest.skipUnless(os.geteuid() == 0, 'protected filesystem fixture requires root')
class PaidCoreFiles(unittest.TestCase):
    def setUp(self):
        old_umask = os.umask(0o077)
        self.addCleanup(os.umask, old_umask)
        self.temporary = tempfile.TemporaryDirectory(prefix='lg-paid-core-fixture-', dir='/run')
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.root.chmod(0o700)
        self.base = self.root / 'upgrades'
        self.base.mkdir(mode=0o700)
        self.unit = self.root / 'core.service'
        self.unit.write_text('[Service]\nUser=fixture\nRestart=on-failure\n')
        self.unit.chmod(0o644)
        self.config = self.root / 'config.toml'
        self.config.write_text('original configuration\n')
        self.config.chmod(0o600)
        self.bin = self.root / 'bin'
        self.bin.mkdir()
        self.targets = {name: self.bin / name for name in ('lightning-goatsd', 'lightning-goatsctl')}
        for name, path in self.targets.items():
            path.write_text('original-' + name)
            path.chmod(0o755)
        self.state = self.root / 'runtime'
        self.state.mkdir(mode=0o700)
        self.database = self.state / 'core.db'
        self.database.write_bytes(b'original-main')
        self.database.chmod(0o600)
        os.chown(self.state, 50020, 50020)
        os.chown(self.database, 50020, 50020)
        self.commands = []
        self.active = True
        self.original = {'unit': {'DropInPaths': ''}, 'files': {
            str(self.unit): TX.fingerprint(self.unit), str(self.config): TX.fingerprint(self.config),
            **{str(p): TX.fingerprint(p) for p in self.targets.values()}}, 'uid': 50020, 'gid': 50020}
        self.directory = self.base / 'upgrade'
        self.directory.mkdir(mode=0o700)
        (self.directory / 'payload').mkdir()
        hashes = {}
        for name in self.targets:
            path = self.directory / 'payload' / name
            path.write_bytes(('new-' + name).encode())
            path.chmod(0o755)
            hashes[name] = TX.fingerprint(path)['sha256']
        policy = self.directory / 'policy.toml'
        policy.write_text('synthetic policy')
        policy.chmod(0o600)
        for i, path in enumerate(self.original['files']):
            TX.copy_durable(path, self.directory / f'original-{i}', self.original['files'][path])
        TX.prepare(self.directory / 'files', [(path, self.directory / 'payload' / name, self.original['files'][str(path)])
                                            for name, path in self.targets.items()], lambda: None)
        self.metadata = {'version': 1, 'tool': {'source': 'fixture'}, 'baseline': self.original,
                         'original_order': list(self.original['files']),
                         'release': {'payload_sha256': hashes},
                         'policy_sha256': hashlib.sha256(policy.read_bytes()).hexdigest(),
                         'policy': {'max_component_bytes': 1024 ** 2}}
        CORE.write_json(self.directory / 'UPGRADE.json', self.metadata)
        for name, value in [('BASE', self.base), ('UNIT', self.unit), ('CONFIG', self.config), ('DATABASE', self.database), ('TARGETS', self.targets)]:
            mock = patch.object(CORE, name, value)
            mock.start()
            self.addCleanup(mock.stop)
        for name, value in [('root', lambda: None), ('verified_tool_identity', lambda: {'source': 'fixture'}),
                            ('new_roles_absent', lambda: None), ('require_baseline', lambda expected: None),
                            ('status', self.status), ('command', self.command)]:
            mock = patch.object(CORE, name, value)
            mock.start()
            self.addCleanup(mock.stop)
        self.assertEqual(CORE.load(self.directory), self.metadata)

    def status(self, name):
        masked = self.unit.is_symlink()
        return {'Id': name, 'LoadState': 'masked' if masked else 'loaded',
                'ActiveState': 'active' if self.active else 'inactive',
                'SubState': 'running' if self.active else 'dead', 'MainPID': '1' if self.active else '0',
                'UnitFileState': 'masked' if masked else 'disabled', 'FragmentPath': str(self.unit),
                'DropInPaths': '', 'NeedDaemonReload': 'no', 'ControlGroup': ''}

    def command(self, *args, **kwargs):
        self.commands.append(args)
        if args[:2] == ('/usr/bin/systemctl', 'stop'):
            self.active = False
        if 'backup-legacy-core' in args:
            (self.directory / 'legacy').mkdir(mode=0o700)
        if args[0] == '/usr/bin/setpriv':
            self.assertTrue(self.unit.is_symlink())
            self.assertEqual(self.targets['lightning-goatsctl'].read_text(), 'new-lightning-goatsctl')
            self.assertIn('--clear-groups', args)
            self.assertIn('--no-new-privs', args)
            # Simulate migration; native Rust fixtures separately qualify the
            # actual old-to-current schema transformation and preserved rows.
            self.database.write_bytes(b'migrated-main')
            os.chown(self.database, 50020, 50020)
        if 'verify-core-upgrade' in args:
            return json.dumps({'accepted': True, 'candidate_compared': '--candidate-copy' in args, 'resume_authorized': False, 'id': 'fixture'})
        return ''

    def test_custom_unit_mask_and_restore_keep_original_bytes_and_never_start(self):
        before_unit, before_config = self.unit.read_bytes(), self.config.read_bytes()
        result = CORE.apply(self.directory, True)
        self.assertTrue(result['core_masked'])
        self.assertFalse(result['core_started'])
        self.assertEqual(os.readlink(self.unit), '/dev/null')
        self.assertEqual((self.directory / 'migrated/core.db').read_bytes(), b'migrated-main')
        self.assertEqual((self.directory / 'migrated/core.db').stat().st_uid, 0)
        self.assertFalse((self.directory / 'migrated/INCOMPLETE').exists())
        self.assertFalse((self.database.parent / 'core.db-shm').exists())
        self.assertEqual(CORE.load(self.directory), self.metadata)  # sorted JSON must not reorder indexed backups
        result = CORE.finish(self.directory, True)
        self.assertTrue(result['explicit_core_restart_required'])
        self.assertEqual(self.unit.read_bytes(), before_unit)
        self.assertEqual(TX.fingerprint(self.unit), self.original['files'][str(self.unit)])
        self.assertEqual(self.config.read_bytes(), before_config)
        self.assertFalse(self.active)
        self.assertFalse(any(args[:2] in [('/usr/bin/systemctl', 'start'), ('/usr/bin/systemctl', 'restart')] for args in self.commands))
        self.assertEqual(self.database.read_bytes(), b'migrated-main')

    def test_preparation_pins_private_archive_and_original_files_without_stopping(self):
        archive = self.root / 'archive.tar.gz'
        archive.write_bytes(b'fixture archive')
        destination = self.base / 'fresh-preparation'
        release = self.metadata['release']
        def extract(archive, source, payload):
            for name in self.targets:
                shutil.copyfile(self.directory / 'payload' / name, payload / name)
                (payload / name).chmod(0o755)
        with patch.object(CORE, 'baseline', return_value=self.original), patch.object(CORE.PREFLIGHT, 'preflight', return_value=release), patch.object(CORE.PREFLIGHT.RELEASE, 'verify_archive', side_effect=extract):
            result = CORE.prepare(archive, 'a' * 40, hashlib.sha256(archive.read_bytes()).hexdigest(), destination)
        self.assertFalse(result['service_changed'])
        self.assertFalse(result['state_opened'])
        self.assertEqual((destination / 'release.tar.gz').read_bytes(), archive.read_bytes())
        self.assertEqual(CORE.load(destination)['baseline'], self.original)
        self.assertTrue(self.active)
        self.assertFalse(self.unit.is_symlink())
        self.assertFalse(any(args[0] == '/usr/bin/systemctl' for args in self.commands))
        self.assertEqual(self.database.read_bytes(), b'original-main')

    def test_bad_archive_digest_leaves_installation_running_and_retains_partial_preparation(self):
        archive = self.root / 'bad.tar.gz'
        archive.write_bytes(b'fixture untrusted archive')
        destination = self.base / 'failed-preparation'
        with patch.object(CORE, 'baseline', return_value=self.original):
            with self.assertRaisesRegex(ValueError, 'Outer archive checksum'):
                CORE.prepare(archive, 'a' * 40, '0' * 64, destination)
        self.assertTrue(self.active)
        self.assertFalse(self.unit.is_symlink())
        self.assertTrue((destination / 'release.tar.gz').exists())
        self.assertFalse((destination / 'UPGRADE.json').exists())
        self.assertEqual(self.database.read_bytes(), b'original-main')

    def test_native_rejection_keeps_mask_and_state_and_disallows_implicit_repeat(self):
        original_command = self.command
        def reject(*args, **kwargs):
            if args[0] == '/usr/bin/setpriv':
                raise ValueError('synthetic migration refusal')
            return original_command(*args, **kwargs)
        with patch.object(CORE, 'command', reject):
            with self.assertRaisesRegex(ValueError, 'migration refusal'):
                CORE.apply(self.directory, True)
        self.assertTrue(self.unit.is_symlink())
        self.assertEqual(self.database.read_bytes(), b'original-main')
        self.assertTrue((self.directory / 'MIGRATION-STARTED.json').exists())
        self.assertFalse((self.directory / 'COMPARISON.json').exists())
        with self.assertRaises(FileExistsError):
            CORE.apply(self.directory, True)
        with self.assertRaises(FileNotFoundError):
            CORE.finish(self.directory, True)
        self.assertFalse(any(args[:2] == ('/usr/bin/systemctl', 'start') for args in self.commands))

    def test_drift_after_comparison_refuses_unit_restoration(self):
        CORE.apply(self.directory, True)
        self.database.write_bytes(b'newer-paid-state')
        with self.assertRaisesRegex(ValueError, 'paid state changed'):
            CORE.finish(self.directory, True)
        self.assertTrue(self.unit.is_symlink())
        self.assertEqual(self.database.read_bytes(), b'newer-paid-state')

    def test_symlinks_aliases_and_policy_drift_are_rejected(self):
        policy = self.directory / 'policy.toml'
        policy.write_text('changed policy')
        with self.assertRaisesRegex(ValueError, 'policy changed'):
            CORE.load(self.directory)
        policy.unlink()
        policy.symlink_to('/dev/null')
        with self.assertRaises(ValueError):
            CORE.load(self.directory)
        alias = self.root / 'alias'
        alias.symlink_to(self.directory)
        with self.assertRaises(ValueError):
            CORE.protected(alias, directory=True)

    def test_hot_journal_and_wrong_runtime_ownership_never_produce_accepted_copy(self):
        self.active = False
        self.unit.unlink()
        self.unit.symlink_to('/dev/null')
        journal = Path(str(self.database) + '-journal')
        journal.write_bytes(b'hot')
        with self.assertRaisesRegex(ValueError, 'rollback journal'):
            CORE.capture(self.directory, self.metadata, self.directory / 'hot-copy')
        journal.unlink()
        os.chown(self.database, 0, 0)
        with self.assertRaisesRegex(ValueError, 'journal identity'):
            CORE.capture(self.directory, self.metadata, self.directory / 'wrong-owner-copy')
        self.assertTrue((self.directory / 'wrong-owner-copy/INCOMPLETE').exists())
