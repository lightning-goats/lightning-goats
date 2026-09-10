"""Safety boundaries for the privileged, namespace-only installation rehearsal."""
import importlib.util
import json
from pathlib import Path
import subprocess
import unittest
from unittest.mock import patch

SPEC = importlib.util.spec_from_file_location(
    "install_rehearsal", Path(__file__).resolve().parents[1] / "scripts/rehearse-install.py"
)
INSTALL = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(INSTALL)


class InstallRehearsalTests(unittest.TestCase):
    def test_host_namespace_is_rejected_before_network_change(self):
        with patch.object(INSTALL.os, "geteuid", return_value=0), \
             patch.object(INSTALL.os, "readlink", return_value="net:[1]"), \
             patch.object(INSTALL.subprocess, "run") as command:
            with self.assertRaisesRegex(ValueError, "host network"):
                INSTALL.require_isolation()
            command.assert_not_called()

    def test_non_loopback_interface_is_rejected(self):
        with patch.object(INSTALL.os, "geteuid", return_value=0), \
             patch.object(INSTALL.os, "readlink", side_effect=["net:[2]", "net:[1]"]), \
             patch.object(INSTALL.subprocess, "check_output", return_value=json.dumps([{"ifname":"lo"},{"ifname":"eth0"}]).encode()), \
             patch.object(INSTALL.subprocess, "run") as command:
            with self.assertRaisesRegex(ValueError, "only loopback"):
                INSTALL.require_isolation()
            command.assert_not_called()

    def test_fixture_drift_is_not_silently_accepted(self):
        for fixture in ["different text", "old old"]:
            with self.assertRaises(ValueError):
                INSTALL.replace_once(fixture,"old","new")
        self.assertEqual(INSTALL.replace_once("before old after","old","new"),"before new after")
