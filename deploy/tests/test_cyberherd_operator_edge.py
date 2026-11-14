"""The native/TLS rehearsal cannot start in an ordinary or host namespace."""
import importlib.util
import os
from pathlib import Path
import unittest
from unittest.mock import patch


class InstalledOperatorEdgeGuards(unittest.TestCase):
    def setUp(self):
        script = Path(__file__).resolve().parents[1] / 'scripts/rehearse-cyberherd-inactive.py'
        spec = importlib.util.spec_from_file_location('operator_edge_guard_fixture', script)
        self.module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(self.module)

    def test_fixture_hostname_preserves_hosts_and_rejects_injected_records(self):
        original = '127.0.0.1 localhost\n::1 localhost'
        result = self.module.hosts_with_local_hostname(original, 'ip-10-0-130-177')
        self.assertEqual(result, original + '\n127.0.1.1 ip-10-0-130-177\n')
        for hostname in ('', 'bad\n1.2.3.4 another', 'host alias', '-invalid', 'x' * 254):
            with self.subTest(hostname=hostname), self.assertRaises(ValueError):
                self.module.hosts_with_local_hostname(original, hostname)

    def test_shared_mount_namespace_refuses_before_any_mount_or_file_operation(self):
        with patch.object(self.module.os, 'geteuid', return_value=0), \
                patch.object(self.module.os, 'readlink', return_value='mnt:[initial]'), \
                patch.object(self.module, 'run') as run:
            with self.assertRaisesRegex(ValueError, 'private root mount namespace'):
                self.module.isolate_fixture_hostname(Path('/unused'))
            run.assert_not_called()

    def test_nonroot_refused_before_any_process_or_file_operation(self):
        with patch.object(self.module.os, 'geteuid', return_value=1000), \
                patch.object(self.module, 'run') as run, \
                patch.object(self.module.subprocess, 'Popen') as process:
            with self.assertRaisesRegex(ValueError, 'attested isolated disposable'):
                self.module.installed_operator_edge(Path('/unused'), Path('/unused'), [])
            run.assert_not_called()
            process.assert_not_called()

    def test_ci_labels_cannot_authorize_the_initial_network_namespace(self):
        with patch.object(self.module.os, 'geteuid', return_value=0), \
                patch.dict(os.environ, {'GITHUB_ACTIONS': 'true', 'GITHUB_RUN_ID': '123'}), \
                patch.object(self.module.os, 'readlink', return_value='net:[same-initial-namespace]'), \
                patch.object(self.module, 'run') as run, \
                patch.object(self.module.subprocess, 'Popen') as process:
            with self.assertRaisesRegex(ValueError, 'attested isolated disposable'):
                self.module.installed_operator_edge(Path('/unused'), Path('/unused'), [])
            run.assert_not_called()
            process.assert_not_called()

    def test_private_namespace_with_other_interfaces_is_not_isolated(self):
        with patch.object(self.module.os, 'geteuid', return_value=0), \
                patch.dict(os.environ, {'GITHUB_ACTIONS': 'true', 'GITHUB_RUN_ID': '123'}), \
                patch.object(self.module.os, 'readlink', side_effect=['net:[private]', 'net:[initial]']), \
                patch.object(self.module, 'run', return_value='[{"ifname":"lo"},{"ifname":"eth0"}]'), \
                patch.object(self.module.subprocess, 'Popen') as process:
            with self.assertRaisesRegex(ValueError, 'attested isolated disposable'):
                self.module.installed_operator_edge(Path('/unused'), Path('/unused'), [])
            process.assert_not_called()
