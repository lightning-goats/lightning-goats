"""Guards and protocol fixtures; actual workers run separately in systemd CI."""
import ast
import importlib.util
import json
from pathlib import Path
import subprocess
import sys
import unittest
from unittest.mock import patch


SCRIPTS = Path(__file__).resolve().parents[1] / 'scripts'
spec = importlib.util.spec_from_file_location('inactive_file_transaction', SCRIPTS / 'inactive_file_transaction.py')
tx = importlib.util.module_from_spec(spec)
spec.loader.exec_module(tx)
with patch.dict(sys.modules, {'inactive_file_transaction': tx}):
    spec = importlib.util.spec_from_file_location('worker_qualification', SCRIPTS / 'qualify-cyberherd-workers.py')
    q = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(q)


class InstalledWorkerTests(unittest.TestCase):
    def test_explicit_flag_precedes_namespace_or_host_reads(self):
        with patch.object(q.C, 'root'), patch.object(q.Q, 'namespace', side_effect=AssertionError('host access')):
            with self.assertRaisesRegex(ValueError, 'explicit'):
                q.run(Path('/unused'), Path('/unused'), Path('/unused'), False)

    def test_host_namespace_refused_before_host_baseline(self):
        with (patch.object(q.C, 'root'), patch.object(q.Q.os, 'readlink', return_value='net:[123]'),
              patch.object(q.C, 'protected', side_effect=AssertionError('filesystem access')),
              patch.object(q.C, 'command', side_effect=AssertionError('service access')),
              patch.object(q.Q, 'baseline', side_effect=AssertionError('identity access'))):
            with self.assertRaisesRegex(ValueError, 'network namespace'):
                q.run(Path('/unused'), Path('/unused'), Path('/unused'), True)

    def test_wire_request_matches_native_rust_fixture(self):
        fixture = SCRIPTS.parent / 'tests/fixtures/cyberherd-zero-sign-request.json'
        self.assertEqual(q.ZERO_SIGN_REQUEST, json.loads(fixture.read_text()))
        self.assertNotEqual(q.OWNER, q.CLIENT)
        ast.parse(q.CLIENT_PROBE)

    def test_partial_or_false_boundary_proof_cannot_qualify(self):
        for role, names in q.PROOFS.items():
            correct = {n: True for n in names}
            self.assertEqual(q.check_proof(role, correct), correct)
            for wrong in ({}, dict(correct, unexpected=True), {n: False for n in names}):
                with self.assertRaises(ValueError):
                    q.check_proof(role, wrong)

    def test_fixture_units_keep_sandbox_without_inheriting_authority(self):
        for role in q.Q.ROLES:
            template = (SCRIPTS.parent / 'systemd' / (role + '.service.example')).read_text()
            template += '\nExecStartPre=/bin/false\nEnvironmentFile=/private/real-key\n'
            result = q.unit_text(template, role, role, ['fixture-group'], '/root-owned/fixture',
                                 [Path('/run/fixture/state')], {'test-key': Path('/private/synthetic.cred')}, Path('/proc/123/ns/net'))
            for forbidden in ('/etc/credstore.encrypted/cyberherd-', 'ExecStartPre=', 'EnvironmentFile=',
                              'StateDirectory=', 'RuntimeDirectory=', 'Restart=on-failure'):
                self.assertNotIn(forbidden, result)
            for required in ('NoNewPrivileges=yes', 'CapabilityBoundingSet=', 'ProtectSystem=strict',
                             'MemoryMax=128M', 'MemorySwapMax=0', 'NetworkNamespacePath=/proc/123/ns/net',
                             'IPAddressDeny=10.0.0.0/8', 'IPAddressDeny=fc00::/7',
                             'LoadCredentialEncrypted=test-key:/private/synthetic.cred',
                             'InaccessiblePaths=-/etc/credstore.encrypted'):
                self.assertIn(required, result)
            with self.assertRaisesRegex(ValueError, 'isolation'):
                q.properties(template.replace('ProtectSystem=strict', 'ProtectSystem=no'))

    def test_fixture_toml_roundtrip_preserves_tables_arrays_and_escaped_values(self):
        value = {'enabled': False, 'count': 0, 'nested': {'key': 'a"b\nc', 'list': ['x', 'y']},
                 'users': [{'name': 'herd', 'active': True}, {'name': 'nova', 'active': False}]}
        self.assertEqual(q.tomllib.loads(q.toml_text(value)), value)
        for wrong in ({'unsafe key': 1}, {'value': None}, {'value': float('nan')}):
            with self.assertRaises(ValueError):
                q.toml_text(wrong)

    def test_direct_entry_point_requires_authenticated_launcher(self):
        result = subprocess.run([sys.executable, '-B', str(SCRIPTS / 'qualify-cyberherd-workers.py'), '--help'],
                                capture_output=True, text=True, timeout=10)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('verified root-owned tool launcher', result.stderr)


if __name__ == '__main__':
    unittest.main()
