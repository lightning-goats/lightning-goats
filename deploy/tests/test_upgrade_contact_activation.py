"""Contact-only configuration transactions; native state/systemd mocked here.

The required root cases exercise real protected files and replacement/mask
handling. Native configuration semantics have separate Rust tests. These Python
fixtures do not establish an installed running contact service or fee delivery.
"""
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
    spec = importlib.util.spec_from_file_location('private_contact_activation', SCRIPTS / 'activate-private-contact.py')
    A = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(A)

ACCEPTED = {'accepted': True, 'configuration_only': True, 'only_contact_configuration_added': True,
            'public_payment_policy_unchanged': True, 'ledger_opened': False, 'credentials_loaded': False,
            'provider_operations': 0, 'services_changed': False}


class ContactActivationBoundaries(unittest.TestCase):
    def test_direct_execution_refuses_before_substituted_helpers(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            shutil.copyfile(SCRIPTS / 'activate-private-contact.py', root / 'activate.py')
            (root / 'maintain-contact-upgrade.py').write_text('raise RuntimeError("INJECTED")')
            result = subprocess.run(['/usr/bin/python3', '-I', '-S', '-B', str(root / 'activate.py'), '--help'],
                                    capture_output=True, text=True, timeout=10)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn('externally verified root-owned tool launcher', result.stderr)
            self.assertNotIn('INJECTED', result.stderr)

    def test_apply_and_resume_require_flags_before_host_access(self):
        with patch.object(A, 'load', side_effect=AssertionError('host accessed')):
            with self.assertRaisesRegex(ValueError, 'exclusive maintenance'):
                A.apply(Path('/missing'), 'a' * 64, False)
            with self.assertRaisesRegex(ValueError, 'resumption approval'):
                A.resume(Path('/missing'), False)

    def test_native_checker_requires_exact_positive_configuration_only_report(self):
        values = [ACCEPTED, dict(ACCEPTED, provider_operations=1), dict(ACCEPTED, ledger_opened=True),
                  dict(ACCEPTED, credentials_loaded=1), dict(ACCEPTED, accepted=1),
                  dict(ACCEPTED, extra=True), {key: value for key, value in ACCEPTED.items() if key != 'services_changed'}]
        for index, value in enumerate(values):
            with self.subTest(value=value), patch.object(A.C, 'command', return_value=json.dumps(value)):
                if index == 0:
                    self.assertEqual(A.validate(Path('/checker'), Path('/old'), Path('/new')), ACCEPTED)
                else:
                    with self.assertRaises(ValueError):
                        A.validate(Path('/checker'), Path('/old'), Path('/new'))


@unittest.skipUnless(os.geteuid() == 0, 'actual protected file transactions require root')
class ContactActivationFiles(unittest.TestCase):
    # Reuse the existing disposable four-owner fixture without inheriting and
    # rerunning its unrelated schema/program tests. Its native/systemd mocks
    # remain explicit, while the file transaction below is the actual code.
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
        self.validator = self.binary / 'check-private-contact'
        self.write(self.validator, b'qualified native configuration checker fixture', 0o755)
        self.activation = self.base / 'contact-activation'
        self.native_validation = dict(ACCEPTED)
        A.prepare(self.activation, self.validator, U.digest(self.validator), ['wss://relay.damus.io/'])
        self.activation_sha = U.digest(self.activation / 'PREPARED.json')
        self.commands.clear()

    def command(self, *arguments, **kwargs):
        if getattr(self, 'validator', None) is not None and arguments[0] == str(self.validator):
            self.commands.append(arguments)
            return json.dumps(self.native_validation)
        return maintenance.ContactMaintenanceFiles.command(self, *arguments, **kwargs)

    def unchanged_money_and_programs(self):
        self.assertEqual(self.migrations, [])
        for role, component in self.components.items():
            self.assertEqual(Path(component['database']).read_bytes(), self.old_bytes[role])
        for path, fingerprint in self.original_files.items():
            if Path(path) not in (self.config, self.unit):
                self.assertEqual(A.TX.fingerprint(path), fingerprint)

    def test_apply_only_changes_config_after_backup_then_resumes_two_existing_roles(self):
        original = A.U.tomllib.loads(self.config.read_text())
        self.assertEqual(A.load(self.activation)[0]['baseline'], self.baseline)
        self.assertEqual(self.config.read_bytes(), (self.activation / 'original-core.toml').read_bytes())
        result = A.apply(self.activation, self.activation_sha, True)
        self.assertTrue(result['current_four_journal_backup_verified'])
        self.assertFalse(result['services_started'])
        self.assertEqual(self.starts, [])
        self.assertTrue(A.M.masked(self.unit))
        configured = A.U.tomllib.loads(self.config.read_text())
        self.assertEqual(configured.pop('contact'), {'recipient': self.policy['owner'],
                                                   'inbox_relays': ['wss://relay.damus.io/']})
        self.assertEqual(configured, original)
        self.unchanged_money_and_programs()
        self.assertEqual((self.activation / 'backup/core.db').read_bytes(), self.old_bytes['core'])
        self.assertTrue(A.resume(self.activation, True)['contact_enabled'])
        self.assertEqual(self.starts, ['lightning-goats-receive.service', 'cyberherd.service'])
        self.assertEqual(self.unit.read_bytes(), (self.activation / 'original-unit').read_bytes())
        self.unchanged_money_and_programs()
        for name in ('cyberherd-payout', 'cyberherd-signer'):
            self.assertTrue(A.M.masked(self.etc / (name + '.service')))
        with self.assertRaisesRegex(ValueError, 'already began'):
            A.resume(self.activation, True)
        self.assertFalse(hasattr(A, 'rollback'))

    def test_digest_or_checker_drift_refuses_before_service_stop(self):
        with self.assertRaisesRegex(ValueError, 'reviewed preparation digest'):
            A.apply(self.activation, '0' * 64, True)
        self.validator.write_bytes(b'changed checker')
        with self.assertRaisesRegex(ValueError, 'native checker changed'):
            A.apply(self.activation, self.activation_sha, True)
        self.assertEqual(self.commands, [])
        self.assertFalse(self.unit.is_symlink())
        self.unchanged_money_and_programs()

    def test_native_validation_refusal_keeps_service_running(self):
        self.native_validation['provider_operations'] = 1
        with self.assertRaisesRegex(ValueError, 'native contact-only'):
            A.apply(self.activation, self.activation_sha, True)
        self.assertFalse(self.unit.is_symlink())
        self.assertEqual(self.starts, [])
        self.assertFalse((self.activation / 'MAINTENANCE-STARTED.json').exists())

    def test_failed_backup_keeps_original_config_and_masks_without_restart(self):
        self.native_refusal = 'verify'
        with self.assertRaisesRegex(ValueError, 'not positively accepted'):
            A.apply(self.activation, self.activation_sha, True)
        self.assertEqual(self.config.read_bytes(), (self.activation / 'original-core.toml').read_bytes())
        self.assertTrue(A.M.masked(self.unit))
        self.assertEqual(self.starts, [])
        self.unchanged_money_and_programs()
        with self.assertRaisesRegex(ValueError, 'already began'):
            A.apply(self.activation, self.activation_sha, True)

    def test_changed_journal_after_apply_prevents_restart_and_retains_new_bytes(self):
        A.apply(self.activation, self.activation_sha, True)
        self.core_db.write_bytes(b'new financial bytes must never be discarded')
        with self.assertRaisesRegex(ValueError, 'financial images changed'):
            A.resume(self.activation, True)
        self.assertTrue(A.M.masked(self.unit))
        self.assertEqual(self.starts, [])
        self.assertEqual(self.core_db.read_bytes(), b'new financial bytes must never be discarded')

    def test_changed_backup_manifest_blocks_restart(self):
        A.apply(self.activation, self.activation_sha, True)
        manifest = self.activation / 'backup/MANIFEST.json'
        manifest.write_text(manifest.read_text() + ' ')
        with self.assertRaisesRegex(ValueError, 'activation backup changed'):
            A.resume(self.activation, True)
        self.assertEqual(self.starts, [])
        self.assertTrue(A.M.masked(self.unit))

    def test_transaction_cannot_replace_a_program_or_unrelated_file(self):
        path = self.activation / 'configuration/TRANSACTION.json'
        value = json.loads(path.read_text())
        value['files'][0]['destination'] = str(self.binary / 'lightning-goatsd')
        path.write_text(json.dumps(value))
        prepared = self.activation / 'PREPARED.json'
        a = json.loads(prepared.read_text())
        a['configuration_manifest_sha256'] = A.U.digest(path)
        a['configurations'] = value['files']
        prepared.write_text(json.dumps(a))
        with self.assertRaisesRegex(ValueError, 'exact core configuration'):
            A.apply(self.activation, A.U.digest(prepared), True)
        self.assertFalse(self.unit.is_symlink())
        self.unchanged_money_and_programs()
