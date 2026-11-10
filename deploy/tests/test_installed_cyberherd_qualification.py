"""Qualifier guards and exact copied-unit policy; no actual service operation."""
import ast
import copy
import importlib.util
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
    spec = importlib.util.spec_from_file_location('installed_qualification', SCRIPTS / 'qualify-cyberherd-installed.py')
    q = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(q)


class InstalledQualificationTests(unittest.TestCase):
    def values(self):
        return {'cyberherd': {'enabled': False},
                'cyberherd-payout': {'enabled': False, 'policy': {'execution_enabled': False,
                    'member_percent_ceiling': 0, 'max_sats_per_feed': 0, 'max_sats_per_24h': 0, 'max_fee_sats': 0}},
                'cyberherd-signer': {'enabled': False, 'policy': {'max_net_sats': 0, 'max_new_per_hour': 0}}}

    def test_active_process_publication_or_nonzero_limits_refuse(self):
        original = self.values()
        q.disabled(original)
        for role in q.ROLES:
            value = copy.deepcopy(original)
            value[role]['enabled'] = True
            with self.assertRaises(ValueError):
                q.disabled(value)
        for name in ('messaging', 'view'):
            value = copy.deepcopy(original)
            value['cyberherd'][name] = {'enabled': True}
            with self.assertRaises(ValueError):
                q.disabled(value)
        for role in ('cyberherd-payout', 'cyberherd-signer'):
            for name in original[role]['policy']:
                value = copy.deepcopy(original)
                value[role]['policy'][name] = True if name == 'execution_enabled' else 1
                with self.assertRaises(ValueError):
                    q.disabled(value)

    def test_host_namespace_rejected_before_identity_file_or_service_access(self):
        with (patch.object(q.C, 'root'), patch.object(q.os, 'readlink', return_value='net:[123]'),
              patch.object(q.C, 'protected', side_effect=AssertionError('filesystem access')),
              patch.object(q.C, 'command', side_effect=AssertionError('service access')),
              patch.object(q, 'baseline', side_effect=AssertionError('identity access'))):
            with self.assertRaisesRegex(ValueError, 'network namespace'):
                q.qualify(Path('/var/lib/lightning-goats-upgrades/install'), Path('/var/lib/lightning-goats-upgrades/probe'), True)

    def test_explicit_flag_is_required_before_namespace_or_files(self):
        with patch.object(q.C, 'root'), patch.object(q, 'namespace', side_effect=AssertionError('namespace access')):
            with self.assertRaisesRegex(ValueError, 'explicit'):
                q.qualify(Path('/unused'), Path('/unused'), False)

    def test_unit_copy_retains_sandbox_and_replaces_real_credentials(self):
        for role in q.ROLES:
            text = (SCRIPTS.parent / 'systemd' / (role + '.service.example')).read_text()
            result = q.render_unit(text, role, Path('/run/test/probe.py'), Path('/run/test/input.json'),
                                   Path('/private/synthetic.cred'), Path('/proc/123/ns/net'))
            self.assertNotIn('/etc/credstore.encrypted/', result)
            self.assertIn('ExecStart=/usr/bin/python3 -I -S -B /run/test/probe.py /run/test/input.json', result)
            self.assertIn('NetworkNamespacePath=/proc/123/ns/net', result)
            self.assertIn('Restart=no', result)
            for directive in ('User=' + role, 'NoNewPrivileges=yes', 'CapabilityBoundingSet=',
                              'ProtectSystem=strict', 'IPAddressDeny=10.0.0.0/8', 'IPAddressDeny=fc00::/7'):
                self.assertIn(directive, result)
            for addition in ('ExecStartPre=/bin/false\n', '  ExecStartPost=/bin/false\n',
                             'LoadCredential=unexpected:/private/unrelated\n', 'Environment=SECRET=unexpected\n',
                             ' EnvironmentFile=/private/unrelated\n', 'ImportCredential=*\n',
                             'SetCredentialEncrypted=unexpected:encrypted\n'):
                with self.assertRaises(ValueError):
                    q.render_unit(text + addition, role, Path('/run/p'), Path('/run/c'), Path('/private/fake'), Path('/proc/123/ns/net'))

    def test_partial_or_wrong_role_positive_report_cannot_qualify(self):
        correct = {name: True for name in q.RESULT_FIELDS}
        correct['role'] = 'cyberherd'
        self.assertEqual(set(q.check_result('cyberherd', correct)), q.RESULT_FIELDS)
        for wrong in [{'role': 'cyberherd', 'ok': True}, dict(correct, role='cyberherd-payout'),
                      dict(correct, private_ipv4_ipv6_denied=False), dict(correct, unexpected=True)]:
            with self.assertRaises(ValueError):
                q.check_result('cyberherd', wrong)

    def test_direct_entry_point_refuses_without_launcher(self):
        result = subprocess.run([sys.executable, '-B', str(SCRIPTS / 'qualify-cyberherd-installed.py'), '--help'],
                                capture_output=True, text=True, timeout=10)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('verified root-owned tool launcher', result.stderr)
        ast.parse(q.PROBE)


if __name__ == '__main__':
    unittest.main()
