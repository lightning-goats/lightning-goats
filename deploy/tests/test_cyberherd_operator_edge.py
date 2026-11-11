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
