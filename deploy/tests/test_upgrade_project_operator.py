"""Capacity/operator boundaries; protected root fixtures use disposable files only."""
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

import test_upgrade_contact_maintenance as support

SCRIPTS = Path(__file__).resolve().parents[1] / 'scripts'
with patch.dict(sys.modules, {'inactive_file_transaction': support.U.TX}):
    spec = importlib.util.spec_from_file_location('project_operator_fixture', SCRIPTS / 'enable-cyberherd-project-operator.py')
    O = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(O)
O.U = support.U
O.C, O.Q, O.S, O.M, O.TX = O.U.C, O.U.Q, O.U.S, O.U.M, O.U.TX
spec = importlib.util.spec_from_file_location('capacity_client_fixture', SCRIPTS / 'set-cyberherd-capacity.py')
K = importlib.util.module_from_spec(spec)
spec.loader.exec_module(K)


def member():
    return {'enabled': True, 'project_pubkey': K.AUTHOR,
        'settings': {'max_members': 10, 'minimum_sats': 10, 'member_percent': 10},
        'view': {'enabled': True, 'auth': {'public_origin': K.ORIGIN, 'operators': ['b' * 64]}},
        'messaging': {'enabled': True, 'lifecycle': False}}


class OperatorBoundaries(unittest.TestCase):
    def test_only_operator_and_seed_change(self):
        original = member()
        before = copy.deepcopy(original)
        actual = O.render(original, K.AUTHOR)
        expected = copy.deepcopy(original)
        expected['settings']['max_members'] = 3
        expected['view']['auth']['operators'].append(K.AUTHOR)
        self.assertEqual(actual, expected)
        self.assertEqual(original, before)
        for changed in (dict(original, project_pubkey='c' * 64), dict(original, enabled=False), actual):
            with self.assertRaises(ValueError):
                O.render(changed, K.AUTHOR)

    def test_approval_precedes_host_access(self):
        with patch.object(O, 'load') as load:
            with self.assertRaisesRegex(ValueError, 'explicit'):
                O.apply(Path('/missing'), '0' * 64, False)
            load.assert_not_called()

    def test_direct_execution_cannot_import_checkout_helpers(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            shutil.copyfile(SCRIPTS / 'enable-cyberherd-project-operator.py', root / 'operator.py')
            (root / 'maintain-contact-upgrade.py').write_text('raise RuntimeError("INJECTED")')
            result = subprocess.run(['/usr/bin/python3', '-I', '-S', '-B', str(root / 'operator.py'), '--help'],
                                    capture_output=True, text=True, timeout=10)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn('verified root-owned tool launcher', result.stderr)
            self.assertNotIn('INJECTED', result.stderr)


class CapacityCommand(unittest.TestCase):
    def state(self):
        return {'owner': K.AUTHOR, 'revision': 4, 'cycle': 2, 'total_members': 0, 'members': [],
                'settings': {'max_members': 10, 'minimum_sats': '10', 'member_percent': 10}}

    def test_exact_signed_command_preserves_other_settings_and_reuses_unknown_request(self):
        before = self.state()
        calls, bodies = [], []
        def call(method, uri, body):
            calls.append((method, uri))
            if method == 'GET':
                if not bodies:
                    return copy.deepcopy(before)
                return dict(before, revision=5, settings=dict(before['settings'], max_members=3))
            bodies.append(body)
            if len(bodies) == 1:
                raise K.Retryable()
            command = json.loads(body)
            self.assertEqual(command['expected_revision'], 4)
            self.assertEqual(command['action'], {'type': 'settings', 'minimum_sats': '10',
                                                'max_members': 3, 'member_percent': 10})
            return {'command_id': command['id'], 'changed': True, 'revision': 5}
        result = K.update(call, pause=lambda _: None)
        self.assertEqual(len(bodies), 2)
        self.assertEqual(bodies[0], bodies[1])
        self.assertTrue(result['members_and_cycle_preserved'])
        self.assertFalse(result['payment_or_feeding_requested'])
        self.assertEqual(calls, [('GET', K.STATE), ('POST', K.COMMAND), ('POST', K.COMMAND), ('GET', K.STATE)])

    def test_already_three_never_posts(self):
        before = self.state()
        before['settings']['max_members'] = 3
        def call(method, uri, body):
            self.assertEqual((method, uri, body), ('GET', K.STATE, b''))
            return before
        self.assertFalse(K.update(call)['changed'])

    def test_wrong_identity_and_boolean_settings_refuse_before_command(self):
        for original in (dict(self.state(), owner='b' * 64), dict(self.state(), revision=True),
                         dict(self.state(), settings=dict(self.state()['settings'], max_members=True))):
            calls = []
            def call(method, uri, body):
                calls.append(method)
                return original
            with self.assertRaises(ValueError):
                K.update(call)
            self.assertEqual(calls, ['GET'])

    def test_unexpected_readback_refuses(self):
        original = self.state()
        def call(method, uri, body):
            if method == 'POST':
                return {'command_id': json.loads(body)['id'], 'changed': True, 'revision': 5}
            value = copy.deepcopy(original)
            original['settings']['member_percent'] = 25
            return value
        with self.assertRaisesRegex(ValueError, 'changed other'):
            K.update(call)


@unittest.skipUnless(os.geteuid() == 0, 'protected operator files run separately in root CI')
class OperatorFiles(unittest.TestCase):
    status = support.ContactMaintenanceFiles.status
    write = staticmethod(support.ContactMaintenanceFiles.write)
    cleanup_carriers = support.ContactMaintenanceFiles.cleanup_carriers
    migrate = support.ContactMaintenanceFiles.migrate

    def setUp(self):
        support.ContactMaintenanceFiles.setUp(self)
        context = patch.object(O, 'verified_tool_identity', lambda: O.U.verified_tool_identity())
        context.start(); self.addCleanup(context.stop)
        self.worker_refusal = False
        current = O.Q.ETC / 'cyberherd/config.toml'
        value = O.U.tomllib.loads(current.read_text())
        value.update(view=member()['view'], settings=member()['settings'])
        value['project_pubkey'] = self.policy['owner']
        self.write(current, O.S.W.toml_text(value).encode(), 0o600)
        self.baseline['files'][str(current)] = O.TX.fingerprint(current)
        self.destination = self.base / 'project-operator'
        O.prepare(self.destination, self.policy['owner'])
        self.sha = O.U.digest(self.destination / 'PREPARED.json')

    def command(self, *arguments, **kwargs):
        if arguments[0] == '/usr/bin/unshare':
            self.commands.append(arguments)
            self.carriers.add(Path(arguments[-2]).parent)
            return 'refused' if self.worker_refusal else 'CyberHerd configuration is valid; no runtime started'
        return support.ContactMaintenanceFiles.command(self, *arguments, **kwargs)

    def test_only_member_config_replaced_and_only_member_restarted(self):
        result = O.apply(self.destination, self.sha, True)
        self.assertTrue(result['stored_capacity_update_pending'])
        restarts = [c for c in self.commands if c[:2] == ('/usr/bin/systemctl', 'restart')]
        self.assertEqual(restarts, [('/usr/bin/systemctl', 'restart', '--', 'cyberherd.service')])
        for name, fingerprint in self.original_files.items():
            if name != str(self.etc / 'cyberherd/config.toml'):
                self.assertEqual(O.TX.fingerprint(name), fingerprint)
        for role, value in self.components.items():
            self.assertEqual(Path(value['database']).read_bytes(), self.old_bytes[role])
        with self.assertRaisesRegex(ValueError, 'already began'):
            O.apply(self.destination, self.sha, True)

    def test_native_refusal_precedes_config_change_and_restart(self):
        self.worker_refusal = True
        with self.assertRaisesRegex(ValueError, 'native project operator'):
            O.apply(self.destination, self.sha, True)
        self.assertEqual(O.TX.fingerprint(self.etc / 'cyberherd/config.toml'),
                         self.baseline['files'][str(self.etc / 'cyberherd/config.toml')])
        self.assertFalse((self.destination / 'APPLY-REQUESTED.json').exists())

    def test_changed_candidate_or_independent_digest_refused(self):
        with self.assertRaisesRegex(ValueError, 'digest'):
            O.apply(self.destination, '0' * 64, True)
        (self.destination / 'candidate-member.toml').write_text('enabled = false\n')
        with self.assertRaisesRegex(ValueError, 'only the selected'):
            O.apply(self.destination, self.sha, True)
