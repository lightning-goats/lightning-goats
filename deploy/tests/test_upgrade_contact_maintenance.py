"""File/failure orchestration tests; native schema and systemd are mocked here.

Root cases use actual protected disposable files, distinct numeric journal owners,
mask replacement and program transactions. They do not qualify native migration,
actual UID execution, installed systemd or a live host.
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
import types
import unittest
from unittest.mock import patch
import uuid

import test_upgrade_contact_prepare as preparation

SCRIPTS = Path(__file__).resolve().parents[1] / 'scripts'
with patch.dict(sys.modules, {'inactive_file_transaction': preparation.TX}):
    spec = importlib.util.spec_from_file_location('contact_maintenance', SCRIPTS / 'maintain-contact-upgrade.py')
    U = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(U)


class ContactMaintenanceBoundaries(unittest.TestCase):
    def test_direct_execution_refuses_before_substituted_helpers(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            shutil.copyfile(SCRIPTS / 'maintain-contact-upgrade.py', root / 'maintain.py')
            (root / 'prepare-contact-upgrade.py').write_text('raise RuntimeError("INJECTED")')
            result = subprocess.run(['/usr/bin/python3', '-I', '-S', '-B', str(root / 'maintain.py'), '--help'],
                                    capture_output=True, text=True, timeout=10)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn('externally verified root-owned tool launcher', result.stderr)
            self.assertNotIn('INJECTED', result.stderr)

    def test_maintenance_and_resumption_need_explicit_flags_before_host_access(self):
        with patch.object(U, 'load', side_effect=AssertionError('host accessed')):
            with self.assertRaisesRegex(ValueError, 'exclusive maintenance'):
                U.apply(Path('/missing'), False)
            with self.assertRaisesRegex(ValueError, 'resumption approval'):
                U.resume(Path('/missing'), False)

    def test_runtime_migration_uses_status_original_uid_no_caps_and_an_isolated_network(self):
        with patch.object(U.C, 'command') as command:
            U.migrate_as_runtime(Path('/protected/ctl'), Path('/protected/config'), {'uid': 10001, 'gid': 10002})
        arguments = command.call_args.args
        self.assertEqual(arguments[:5], ('/usr/bin/unshare', '--net', '--', '/usr/bin/setpriv', '--reuid=10001'))
        for argument in ('--regid=10002', '--clear-groups', '--inh-caps=-all', '--ambient-caps=-all',
                         '--bounding-set=-all', '--no-new-privs'):
            self.assertIn(argument, arguments)
        self.assertEqual(arguments[-4:], ('/protected/ctl', '--config', '/protected/config', 'status'))
        self.assertEqual(command.call_args.kwargs['timeout'], 300)

    def test_native_receipts_require_exact_operation_identity_and_no_resumption(self):
        value = {'operation': 'contact_upgrade_source_verify', 'id': str(uuid.uuid4()),
                 'accepted': True, 'resume_authorized': False}
        with patch.object(U.C, 'protected'), patch.object(U.C, 'command', return_value=json.dumps(value)):
            self.assertEqual(U.native({'review': '/protected'}, 'verify-contact-upgrade-source'), value)
        for changes in ({'operation': 'snapshot'}, {'accepted': False}, {'resume_authorized': True}, {'unexpected': 'x'}):
            changed = dict(value, **changes)
            with patch.object(U.C, 'protected'), patch.object(U.C, 'command', return_value=json.dumps(changed)):
                with self.assertRaises(ValueError):
                    U.native({'review': '/protected'}, 'verify-contact-upgrade-source')

    def test_feeder_cap_native_selector_uses_only_its_explicit_source_boundary(self):
        value = {'operation': 'feeder_cap_upgrade_source_verify', 'id': str(uuid.uuid4()),
                 'accepted': True, 'resume_authorized': False}
        with patch.object(U, 'UPGRADE_KIND', 'feeder-cap'), patch.object(U.C, 'protected'), patch.object(U.C, 'command', return_value=json.dumps(value)) as command:
            self.assertEqual(U.native({'review': '/protected'}, 'verify-contact-upgrade-source'), value)
            self.assertIn('verify-feeder-cap-upgrade-source', command.call_args.args)
            with self.assertRaises(ValueError):
                U.patch_mode({'core_upgrade':'contact'}, False)
        with patch.object(U, 'load', side_effect=AssertionError('host accessed')):
            with self.assertRaisesRegex(ValueError, 'explicit feeder-cap'):
                U.repair_backlog(Path('/missing'), 'stable-repair', 1, 24101)
        with patch.object(U, 'UPGRADE_KIND', 'feeder-cap'), patch.object(U, 'load', side_effect=AssertionError('host accessed')):
            for values in [('', 1, 24101), ('id', 0, 24101), ('id', 1, 0), ('id', True, 24101)]:
                with self.assertRaisesRegex(ValueError, 'stable repair ID'):
                    U.repair_backlog(Path('/missing'), *values)


@unittest.skipUnless(os.geteuid() == 0, 'actual protected file transactions require root')
class ContactMaintenanceFiles(unittest.TestCase):
    def setUp(self):
        previous = os.umask(0o077)
        self.addCleanup(os.umask, previous)
        self.temporary = tempfile.TemporaryDirectory(prefix='lg-contact-maintenance-', dir='/run')
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.base, self.etc, self.binary, self.vendor = (self.root / name for name in ('upgrades', 'etc', 'bin', 'vendor'))
        for path in (self.base, self.etc, self.binary, self.vendor):
            path.mkdir(mode=0o700)
        self.config = self.etc / 'core.toml'
        self.unit = self.etc / U.C.UNIT_NAME
        self.policy_path = self.etc / 'maintenance.toml'
        self.commands, self.starts, self.migrations, self.carriers = [], [], [], set()
        self.native_refusal = None
        self.unit_live = {name + '.service': role in ('core', 'membership') for role, name in U.P.ROLE_NAMES.items()}
        self.identity = {role: {'uid': 50001 + i, 'gid': 50001 + i} for i, role in enumerate(U.P.ROLE_NAMES)}
        self.components = {}
        runtime = self.root / 'runtime'
        runtime.mkdir(mode=0o700)
        for role, name in U.P.ROLE_NAMES.items():
            parent = runtime / role
            parent.mkdir(mode=0o700)
            path = parent / (role + '.db')
            path.write_bytes(('original financial/signed history: ' + role).encode())
            path.chmod(0o600)
            os.chown(path, self.identity[role]['uid'], self.identity[role]['gid'])
            os.chown(parent, self.identity[role]['uid'], self.identity[role]['gid'])
            self.components[role] = dict(self.identity[role], database=str(path), unit=name + '.service')
        self.core_db = Path(self.components['core']['database'])
        self.old_bytes = {role: Path(value['database']).read_bytes() for role, value in self.components.items()}

        patches = [(U.C, 'BASE', self.base), (U.C, 'CONFIG', self.config), (U.C, 'UNIT', self.unit),
                   (U.C, 'DATABASE', self.core_db), (U.S, 'MAINTENANCE', self.policy_path),
                   (U.Q, 'BIN', self.binary), (U.Q, 'ETC', self.etc), (U.Q, 'UNITS', self.vendor),
                   (U, 'MASKS', self.etc), (U, 'OVERRIDE_ROOTS', (self.etc, self.vendor)),
                   (U.M, 'C', U.C), (U.M, 'MASKS', self.etc),
                   (U, 'verified_tool_identity', lambda: {'source': 'contact-maintenance-fixture'}),
                   (U.C, 'root', lambda: None), (U.C, 'status', self.status), (U.C, 'command', self.command),
                   (U.Q, 'no_role_processes', lambda identities: None),
                   (U.Q.I, 'require_no_sudo', lambda name: None), (U.S, 'role_dropins', lambda: {}),
                   (U.Q.I, 'check_identities', lambda: {U.P.ROLE_NAMES[role]: value for role, value in self.identity.items() if role != 'core'}),
                   (U.C.pwd, 'getpwnam', lambda name: types.SimpleNamespace(pw_name='lightning-goats', pw_uid=50001, pw_gid=50001, pw_shell='/usr/sbin/nologin')),
                   (U.C.grp, 'getgrgid', lambda gid: types.SimpleNamespace(gr_name='lightning-goats' if gid == 50001 else 'fixture')),
                   (U.os, 'getgrouplist', lambda *args: [50001]),
                   (U, 'migrate_as_runtime', self.migrate), (U.M, 'running', lambda *args: None), (U.M, 'health', lambda url: None)]
        for obj, name, value in patches:
            context = patch.object(obj, name, value)
            context.start()
            self.addCleanup(context.stop)

        values = preparation.fixture()
        values[0]['database']['url'] = 'sqlite://' + str(self.core_db)
        values[1]['core']['core_uid'] = 50001
        values[4]['components'] = self.components
        self.policy = values[4]
        self.policy.update(version=1, max_component_bytes=1024 * 1024)
        self.write(self.config, U.S.W.toml_text(values[0]).encode(), 0o600)
        self.write(self.policy_path, U.S.W.toml_text(self.policy).encode(), 0o600)
        self.write(self.unit, b'[Service]\nUser=lightning-goats\nGroup=lightning-goats\n', 0o644)
        paths = [self.config, self.policy_path, self.unit]
        for role, value in zip(('membership', 'payout', 'signer'), values[1:4]):
            name = U.P.ROLE_NAMES[role]
            directory = self.etc / name
            directory.mkdir(mode=0o700)
            config = directory / 'config.toml'
            self.write(config, U.S.W.toml_text(value).encode(), 0o600)
            paths.append(config)
            unit = self.vendor / (name + '.service')
            self.write(unit, ('[Service]\nUser=' + name + '\n').encode(), 0o644)
            paths.append(unit)
            if role in ('payout', 'signer'):
                (self.etc / (name + '.service')).symlink_to('/dev/null')
        for name in (*U.P.TARGET_NAMES, 'cyberherdd', 'cyberherd-payoutd', 'cyberherd-signerd'):
            path = self.binary / name
            self.write(path, ('old program: ' + name).encode(), 0o755)
            paths.append(path)
        self.original_files = {str(path): U.TX.fingerprint(path) for path in paths}
        self.baseline = {'files': self.original_files, 'identities': self.identity, 'policy': self.policy,
                         'states': {role: self.status(name + '.service') for role, name in U.P.ROLE_NAMES.items()}}
        self.review = self.base / 'review'
        self.review.mkdir(mode=0o700)
        payload = self.review / 'payload'
        payload.mkdir(mode=0o700)
        hashes = {}
        for name in U.P.TARGET_NAMES:
            self.write(payload / name, ('new program: ' + name).encode(), 0o755)
            hashes[name] = U.digest(payload / name)
        for index, path in enumerate(self.original_files):
            U.TX.copy_durable(path, self.review / f'original-{index}', self.original_files[path])
        self.write(self.review / 'release.tar.gz', b'qualified core fixture archive', 0o600)
        self.write(self.review / 'review.tar.gz', b'qualified review fixture archive', 0o600)
        review = {'version': 1, 'tool': U.verified_tool_identity(), 'baseline': self.baseline,
                  'native_runtime_help_verified': True,
                  'original_order': list(self.original_files), 'selected_program_sha256': hashes,
                  'release': {'archive_sha256': U.digest(self.review / 'release.tar.gz')},
                  'review_archive_sha256': U.digest(self.review / 'review.tar.gz'),
                  **{key: False for key in ('activation_supported', 'services_changed', 'journals_opened', 'credentials_loaded', 'contact_enabled')}}
        U.C.write_json(self.review / 'PREPARED.json', review)
        self.review_sha = U.digest(self.review / 'PREPARED.json')
        self.execution = self.base / 'execution'
        context = patch.object(U.P, 'baseline', return_value=self.baseline)
        context.start()
        self.addCleanup(context.stop)
        U.plan(self.review, self.review_sha, self.execution)
        self.addCleanup(self.cleanup_carriers)

    def cleanup_carriers(self):
        for path in self.carriers:
            shutil.rmtree(path)

    @staticmethod
    def write(path, raw, mode):
        path.write_bytes(raw)
        path.chmod(mode)

    def status(self, unit):
        role = next(role for role, name in U.P.ROLE_NAMES.items() if unit == name + '.service')
        mask = self.etc / unit
        masked = mask.is_symlink() and os.readlink(mask) == '/dev/null'
        live = self.unit_live[unit]
        user = 'lightning-goats' if role == 'core' else U.P.ROLE_NAMES[role]
        return {'Id': unit, 'LoadState': 'masked' if masked else 'loaded',
                'ActiveState': 'active' if live else 'inactive', 'SubState': 'running' if live else 'dead',
                'MainPID': '12345' if live else '0', 'UnitFileState': 'masked' if masked else 'disabled',
                'NeedDaemonReload': 'no', 'ControlGroup': '', 'User': user, 'Group': user,
                'FragmentPath': str(self.unit if role == 'core' else self.vendor / unit), 'DropInPaths': ''}

    def command(self, *arguments, **kwargs):
        self.commands.append(arguments)
        if arguments[0] == '/usr/bin/systemctl':
            if arguments[1] in ('stop', 'start'):
                for unit in arguments[3:]:
                    self.unit_live[unit] = arguments[1] == 'start'
                    if arguments[1] == 'start':
                        self.starts.append(unit)
            return ''
        operation = arguments[3]
        if operation in ('snapshot-before-contact', 'snapshot'):
            destination = Path(arguments[arguments.index('--destination') + 1])
            destination.mkdir(mode=0o700)
            images = []
            for role, component in self.components.items():
                path = destination / (role + '.db')
                raw = Path(component['database']).read_bytes()
                self.write(path, raw, 0o600)
                images.append({'name': path.name, 'sha256': hashlib.sha256(raw).hexdigest(), 'bytes': len(raw)})
            U.C.write_json(destination / 'MANIFEST.json', {'id': str(uuid.uuid4()), 'images': images})
            bundle = destination
        else:
            bundle = Path(arguments[arguments.index('--bundle') + 1])
        manifest = json.loads((bundle / 'MANIFEST.json').read_text())
        if operation == 'compare-contact-upgrade':
            candidate = Path(arguments[arguments.index('--candidate-copy') + 1])
            self.assertEqual((candidate / 'core.db').read_bytes(), b'migrated synthetic core')
            self.assertEqual((bundle / 'core.db').read_bytes(), self.old_bytes['core'])
            for role in ('membership', 'payout', 'signer'):
                self.assertEqual((candidate / (role + '.db')).read_bytes(), self.old_bytes[role])
        names = {'snapshot-before-contact': 'contact_upgrade_snapshot', 'snapshot': 'snapshot',
                 'verify-contact-upgrade-source': 'contact_upgrade_source_verify', 'verify': 'verify',
                 'compare-contact-upgrade': 'contact_upgrade_compare'}
        return json.dumps({'operation': names[operation], 'id': manifest['id'],
                           'accepted': operation != self.native_refusal, 'resume_authorized': False})

    def migrate(self, program, config_path, component):
        config = U.tomllib.loads(config_path.read_text())
        path = Path(config['database']['url'].removeprefix('sqlite://'))
        self.migrations.append(path)
        self.assertTrue(U.M.masked(self.unit))
        self.assertTrue(U.M.masked(self.etc / 'cyberherd.service'))
        if path != self.core_db:
            self.carriers.add(config_path.parent)
            self.assertEqual(self.core_db.read_bytes(), self.old_bytes['core'])
            for name in U.P.TARGET_NAMES:
                self.assertEqual((self.binary / name).read_bytes(), ('old program: ' + name).encode())
        else:
            self.assertTrue((self.execution / 'CLONE-ACCEPTED.json').exists())
            self.assertTrue((self.execution / 'MIGRATION-STARTED.json').exists())
            for name in U.P.TARGET_NAMES:
                self.assertEqual((self.binary / name).read_bytes(), ('new program: ' + name).encode())
        path.write_bytes(b'migrated synthetic core')

    def test_native_backup_and_clone_precede_replacement_and_resumption_preserves_upgraded_state(self):
        self.assertEqual(self.commands, [])
        self.assertEqual(U.load(self.execution)[1]['baseline'], self.baseline)
        result = U.apply(self.execution, True)
        self.assertTrue(result['clone_compared'])
        self.assertFalse(result['services_started'])
        self.assertEqual(self.starts, [])
        self.assertEqual(len(self.migrations), 2)
        self.assertEqual(self.core_db.read_bytes(), b'migrated synthetic core')
        original_unit = (self.review / ('original-' + str(list(self.original_files).index(str(self.unit))))).read_bytes()
        result = U.resume(self.execution, True)
        self.assertTrue(result['core_running'])
        self.assertEqual(self.starts, ['lightning-goats-receive.service', 'cyberherd.service'])
        self.assertEqual(self.unit.read_bytes(), original_unit)
        self.assertFalse((self.etc / 'cyberherd.service').exists())
        self.assertEqual(self.core_db.read_bytes(), b'migrated synthetic core')
        self.assertEqual((self.execution / 'backup/core.db').read_bytes(), self.old_bytes['core'])
        for role in ('payout', 'signer'):
            self.assertTrue(U.M.masked(self.etc / (U.P.ROLE_NAMES[role] + '.service')))
        with self.assertRaisesRegex(ValueError, 'already began'):
            U.resume(self.execution, True)

    def test_native_source_rejection_retains_masks_original_programs_and_financial_bytes(self):
        self.native_refusal = 'verify-contact-upgrade-source'
        with self.assertRaisesRegex(ValueError, 'not positively accepted'):
            U.apply(self.execution, True)
        self.assertEqual(self.migrations, [])
        self.assertTrue(U.M.masked(self.unit))
        self.assertEqual(self.starts, [])
        self.assertEqual(self.core_db.read_bytes(), self.old_bytes['core'])
        for name in U.P.TARGET_NAMES:
            self.assertEqual((self.binary / name).read_bytes(), ('old program: ' + name).encode())
        with self.assertRaisesRegex(ValueError, 'already began'):
            U.apply(self.execution, True)

    def test_unaccepted_clone_never_changes_live_programs_or_schema(self):
        self.native_refusal = 'compare-contact-upgrade'
        with self.assertRaisesRegex(ValueError, 'not positively accepted'):
            U.apply(self.execution, True)
        self.assertEqual(len(self.migrations), 1)
        self.assertEqual(self.core_db.read_bytes(), self.old_bytes['core'])
        self.assertFalse((self.execution / 'MIGRATION-STARTED.json').exists())
        self.assertFalse((self.execution / 'COMPARISON.json').exists())
        self.assertEqual(self.starts, [])
        for name in U.P.TARGET_NAMES:
            self.assertEqual((self.binary / name).read_bytes(), ('old program: ' + name).encode())

    def test_changed_paid_state_after_migration_blocks_unit_restoration_and_restart(self):
        U.apply(self.execution, True)
        self.core_db.write_bytes(b'unreviewed new financial state')
        with self.assertRaisesRegex(ValueError, 'financial images changed'):
            U.resume(self.execution, True)
        self.assertTrue(U.M.masked(self.unit))
        self.assertEqual(self.starts, [])
        self.assertEqual(self.core_db.read_bytes(), b'unreviewed new financial state')

    def test_unknown_program_bytes_or_changed_review_cannot_apply(self):
        target = self.binary / 'lightning-goatsctl'
        target.write_bytes(b'unknown executable')
        with self.assertRaisesRegex(ValueError, 'installation changed'):
            U.apply(self.execution, True)
        self.assertEqual(self.migrations, [])
        self.assertFalse(self.unit.is_symlink())
        self.assertEqual(self.starts, [])

    def test_changed_selected_program_is_refused_before_any_service_stop(self):
        (self.review / 'payload/lightning-goatsctl').write_bytes(b'unqualified program')
        with self.assertRaisesRegex(ValueError, 'selected native program changed'):
            U.apply(self.execution, True)
        self.assertEqual(self.commands, [])
        self.assertEqual(self.migrations, [])
        self.assertFalse((self.execution / 'MAINTENANCE-STARTED.json').exists())
        self.assertFalse(self.unit.is_symlink())

    def test_ambiguous_live_migration_retains_new_programs_masks_and_financial_state(self):
        migrate = self.migrate
        def interrupted(program, config, component):
            migrate(program, config, component)
            if Path(U.tomllib.loads(config.read_text())['database']['url'].removeprefix('sqlite://')) == self.core_db:
                self.core_db.write_bytes(b'ambiguous migration state must be retained')
                raise ValueError('synthetic live migration interruption')
        with patch.object(U, 'migrate_as_runtime', interrupted):
            with self.assertRaisesRegex(ValueError, 'live migration interruption'):
                U.apply(self.execution, True)
        self.assertTrue((self.execution / 'MIGRATION-STARTED.json').exists())
        self.assertFalse((self.execution / 'COMPARISON.json').exists())
        self.assertTrue(U.M.masked(self.unit))
        self.assertEqual(self.starts, [])
        self.assertEqual(self.core_db.read_bytes(), b'ambiguous migration state must be retained')
        self.assertEqual((self.execution / 'backup/core.db').read_bytes(), self.old_bytes['core'])
        for name in U.P.TARGET_NAMES:
            self.assertEqual((self.binary / name).read_bytes(), ('new program: ' + name).encode())
        with self.assertRaisesRegex(ValueError, 'already began'):
            U.apply(self.execution, True)
        self.assertFalse(hasattr(U, 'rollback'))
