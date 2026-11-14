"""Self-read-only protected transactions; native/systemd results mocked here.

The installed disposable rehearsal separately executes real native validation,
backup, service resumption and signed requests. These tests never prove those
native operations with a mock and never touch an actual installation.
"""
import copy
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

import test_upgrade_contact_maintenance as maintenance

SCRIPTS = Path(__file__).resolve().parents[1] / 'scripts'
with patch.dict(sys.modules, {'inactive_file_transaction': maintenance.U.TX}):
    spec = importlib.util.spec_from_file_location('self_read_activation_fixture', SCRIPTS / 'activate-cyberherd-self-read.py')
    A = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(A)

ORIGIN = 'https://example.com'


class SelfReadBoundaries(unittest.TestCase):
    def test_direct_execution_refuses_before_substituted_helper_import(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            shutil.copyfile(SCRIPTS / 'activate-cyberherd-self-read.py', root / 'activate.py')
            (root / 'maintain-contact-upgrade.py').write_text('raise RuntimeError("INJECTED")')
            result = subprocess.run(['/usr/bin/python3', '-I', '-S', '-B', str(root / 'activate.py'), '--help'],
                capture_output=True, text=True, timeout=10)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn('externally verified root-owned tool launcher', result.stderr)
            self.assertNotIn('INJECTED', result.stderr)

    def test_scope_preserves_every_existing_field_and_never_adds_an_operator(self):
        original = {'enabled': True, 'view': {'enabled': True, 'listen': '127.0.0.1:3340'},
                    'messaging': {'lifecycle': False}, 'core': {'epoch': 'unchanged'}}
        candidate = copy.deepcopy(original)
        candidate['view']['auth'] = {'public_origin': ORIGIN, 'operators': []}
        A.configuration_scope(original, candidate, ORIGIN)
        for section, name, value in [('view', 'listen', '0.0.0.0:3340'), ('messaging', 'lifecycle', True),
                                     ('core', 'epoch', 'changed')]:
            changed = copy.deepcopy(candidate)
            changed[section][name] = value
            with self.subTest(name=name), self.assertRaises(ValueError):
                A.configuration_scope(original, changed, ORIGIN)
        for auth in ({'public_origin': ORIGIN, 'operators': ['ab' * 32]},
                     {'public_origin': 'https://wrong.example', 'operators': []},
                     {'public_origin': ORIGIN, 'operators': [], 'extra': True}):
            changed = copy.deepcopy(candidate)
            changed['view']['auth'] = auth
            with self.subTest(auth=auth), self.assertRaises(ValueError):
                A.configuration_scope(original, changed, ORIGIN)
        for origin in ('http://example.com', 'https://example.com/', 'https://user@example.com',
                       'https://example.com?query=1', 'https://example.com#fragment'):
            with self.subTest(origin=origin), self.assertRaises(ValueError):
                A.configuration_scope(original, candidate, origin)
        with self.assertRaises(ValueError):
            A.configuration_scope(candidate, candidate, ORIGIN)

    def test_flags_are_required_before_host_access(self):
        with patch.object(A, 'load', side_effect=AssertionError('host accessed')):
            with self.assertRaisesRegex(ValueError, 'exclusive maintenance'):
                A.apply(Path('/unused'), '0' * 64, False)
            with self.assertRaisesRegex(ValueError, 'resumption approval'):
                A.resume(Path('/unused'), False)


@unittest.skipUnless(os.geteuid() == 0, 'actual protected file transactions require root')
class SelfReadFiles(unittest.TestCase):
    write = staticmethod(maintenance.ContactMaintenanceFiles.write)
    status = maintenance.ContactMaintenanceFiles.status
    migrate = maintenance.ContactMaintenanceFiles.migrate
    cleanup_carriers = maintenance.ContactMaintenanceFiles.cleanup_carriers

    def setUp(self):
        maintenance.ContactMaintenanceFiles.setUp(self)
        U = maintenance.U
        for name, value in [('U', U), ('C', U.C), ('Q', U.Q), ('S', U.S), ('M', U.M), ('TX', U.TX),
                            ('verified_tool_identity', U.verified_tool_identity)]:
            context = patch.object(A, name, value)
            context.start()
            self.addCleanup(context.stop)
        self.member = self.etc / 'cyberherd/config.toml'
        config = U.tomllib.loads(self.member.read_text())
        config['view'] = {'enabled': True, 'listen': '127.0.0.1:3340'}
        self.write(self.member, U.S.W.toml_text(config).encode(), 0o600)
        self.original_files[str(self.member)] = U.TX.fingerprint(self.member)
        core = U.tomllib.loads(self.config.read_text())
        core['contact'] = {'recipient': self.policy['owner'], 'inbox_relays': ['wss://relay.damus.io/']}
        self.write(self.config, U.S.W.toml_text(core).encode(), 0o600)
        self.original_files[str(self.config)] = U.TX.fingerprint(self.config)
        self.checker_refuses = False
        self.activation = self.base / 'self-read'
        A.prepare(self.activation, ORIGIN)
        self.activation_sha = U.digest(self.activation / 'PREPARED.json')
        self.commands.clear()

    def command(self, *arguments, **kwargs):
        if arguments[0] == '/usr/bin/unshare':
            self.commands.append(arguments)
            self.assertIn('--reuid=50002', arguments)
            self.assertIn('--regid=50002', arguments)
            self.assertIn('--bounding-set=-all', arguments)
            self.assertEqual(arguments[-1], '--check')
            self.assertIn(str(self.binary / 'cyberherdd'), arguments)
            path = Path(arguments[arguments.index('--config') + 1])
            self.carriers.add(path.parent)
            self.assertEqual(path.stat().st_gid, 50002)
            self.assertEqual(path.stat().st_mode & 0o777, 0o440)
            self.assertEqual(maintenance.U.tomllib.loads(path.read_text())['view']['auth'],
                             {'public_origin': ORIGIN, 'operators': []})
            return 'checker refuses' if self.checker_refuses else 'CyberHerd configuration is valid; no runtime started\n'
        return maintenance.ContactMaintenanceFiles.command(self, *arguments, **kwargs)

    def unchanged_money_and_programs(self):
        self.assertEqual(self.migrations, [])
        for role, component in self.components.items():
            self.assertEqual(Path(component['database']).read_bytes(), self.old_bytes[role])
        for path, fp in self.original_files.items():
            if Path(path) not in (self.member, self.unit):
                self.assertEqual(A.TX.fingerprint(path), fp)

    def test_exact_member_only_apply_and_resume_preserve_paid_images_contact_and_masks(self):
        original = A.U.tomllib.loads(self.member.read_text())
        result = A.apply(self.activation, self.activation_sha, True)
        self.assertTrue(result['current_four_journal_backup_verified'])
        self.assertFalse(result['operator_access_enabled'])
        self.assertFalse(result['services_started'])
        configured = A.U.tomllib.loads(self.member.read_text())
        self.assertEqual(configured['view'].pop('auth'), {'public_origin': ORIGIN, 'operators': []})
        self.assertEqual(configured, original)
        self.assertEqual(self.starts, [])
        self.assertTrue(A.M.masked(self.unit))
        self.unchanged_money_and_programs()
        resumed = A.resume(self.activation, True)
        self.assertTrue(resumed['self_reads_configured'])
        self.assertFalse(resumed['operator_access_enabled'])
        self.assertFalse(resumed['payouts_enabled'])
        self.assertEqual(self.starts, ['lightning-goats-receive.service', 'cyberherd.service'])
        self.assertEqual(self.unit.read_bytes(), (self.activation / 'original-unit').read_bytes())
        for role in ('cyberherd-payout', 'cyberherd-signer'):
            self.assertTrue(A.M.masked(self.etc / (role + '.service')))
        self.unchanged_money_and_programs()
        with self.assertRaisesRegex(ValueError, 'already began'):
            A.resume(self.activation, True)
        self.assertFalse(hasattr(A, 'rollback'))

    def test_preparation_digest_or_native_checker_drift_stops_before_maintenance(self):
        with self.assertRaisesRegex(ValueError, 'reviewed preparation digest'):
            A.apply(self.activation, '0' * 64, True)
        (self.binary / 'cyberherdd').write_bytes(b'changed native checker')
        with self.assertRaisesRegex(ValueError, 'native member checker changed'):
            A.apply(self.activation, self.activation_sha, True)
        self.assertFalse((self.activation / 'MAINTENANCE-STARTED.json').exists())
        self.assertFalse(self.unit.is_symlink())

    def test_native_configuration_refusal_keeps_original_services_running(self):
        self.checker_refuses = True
        with self.assertRaisesRegex(ValueError, 'native self-read configuration'):
            A.apply(self.activation, self.activation_sha, True)
        self.assertFalse(self.unit.is_symlink())
        self.assertFalse((self.activation / 'MAINTENANCE-STARTED.json').exists())
        self.unchanged_money_and_programs()

    def test_failed_native_backup_preserves_original_configuration_and_stops_writers(self):
        self.native_refusal = 'verify'
        with self.assertRaisesRegex(ValueError, 'not positively accepted'):
            A.apply(self.activation, self.activation_sha, True)
        self.assertEqual(self.member.read_bytes(), (self.activation / 'original-member.toml').read_bytes())
        self.assertTrue(A.M.masked(self.unit))
        self.assertEqual(self.starts, [])
        self.unchanged_money_and_programs()
        with self.assertRaisesRegex(ValueError, 'already began'):
            A.apply(self.activation, self.activation_sha, True)

    def test_financial_image_drift_cannot_be_replaced_or_resumed(self):
        A.apply(self.activation, self.activation_sha, True)
        self.core_db.write_bytes(b'new financial bytes retained')
        with self.assertRaisesRegex(ValueError, 'financial images changed'):
            A.resume(self.activation, True)
        self.assertEqual(self.starts, [])
        self.assertEqual(self.core_db.read_bytes(), b'new financial bytes retained')
        self.assertTrue(A.M.masked(self.unit))

    def test_changed_backup_manifest_prevents_resumption(self):
        A.apply(self.activation, self.activation_sha, True)
        path = self.activation / 'backup/MANIFEST.json'
        path.write_text(path.read_text() + ' ')
        with self.assertRaisesRegex(ValueError, 'self-read backup changed'):
            A.resume(self.activation, True)
        self.assertEqual(self.starts, [])

    def test_transaction_cannot_redirect_replacement_to_core_or_program(self):
        manifest = self.activation / 'configuration/TRANSACTION.json'
        value = json.loads(manifest.read_text())
        value['files'][0]['destination'] = str(self.config)
        manifest.write_text(json.dumps(value))
        prepared = self.activation / 'PREPARED.json'
        a = json.loads(prepared.read_text())
        a['configuration_manifest_sha256'] = A.U.digest(manifest)
        a['configurations'] = value['files']
        prepared.write_text(json.dumps(a))
        with self.assertRaisesRegex(ValueError, 'exact member configuration'):
            A.apply(self.activation, A.U.digest(prepared), True)
        self.assertFalse(self.unit.is_symlink())
        self.unchanged_money_and_programs()

    def test_numeric_mode_and_operator_flags_are_not_booleans(self):
        prepared = self.activation / 'PREPARED.json'
        original = prepared.read_text()
        for key, value in [('self_read_auth_only', 1), ('operator_access_enabled', 0)]:
            a = json.loads(original)
            a[key] = value
            prepared.write_text(json.dumps(a))
            with self.subTest(key=key), self.assertRaisesRegex(ValueError, 'preparation identity'):
                A.apply(self.activation, A.U.digest(prepared), True)
            self.assertFalse(self.unit.is_symlink())
