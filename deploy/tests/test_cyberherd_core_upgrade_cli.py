"""Core backup commands must branch before app configuration or migration."""
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]


@unittest.skipUnless(os.environ.get("LG_CORE_UPGRADE_TEST_BINARY"), "select native core CLI")
class LegacyCoreCLI(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="lg-legacy-cli-")
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.policy = self.root / "policy.toml"
        self.text = (ROOT / "deploy/cyberherd/core-upgrade.toml.example").read_text()
        self.text = self.text.replace("uid = 0", "uid = 50010").replace("gid = 0", "gid = 50010")

    def invoke(self, text, *args):
        self.policy.write_text(text)
        result = subprocess.run([os.environ["LG_CORE_UPGRADE_TEST_BINARY"], "--config", "/must-not-load-app-config.toml", *args],
                                capture_output=True, text=True, timeout=15, check=False)
        self.assertNotIn("CREDENTIALS_DIRECTORY", result.stderr)
        self.assertNotIn("synthetic-forbidden", result.stderr)
        self.assertEqual([p.name for p in self.root.iterdir()], ["policy.toml"])
        return result

    def test_policy_check_never_loads_app_configuration_or_opens_state(self):
        args = ("check-core-upgrade", "--policy", str(self.policy))
        result = self.invoke(self.text, *args)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("no state, credentials or services accessed", result.stdout)
        for text in ["api_key='synthetic-forbidden'\n" + self.text,
                     self.text.replace("uid = 50010", "uid = 0"),
                     self.text.replace("lightning-goats-receive.service", "--wrong.service")]:
            self.assertNotEqual(self.invoke(text, *args).returncode, 0)

    def test_backup_requires_attestation_root_policy_and_new_private_output(self):
        output = str(self.root / "must-not-exist")
        for args in [("backup-legacy-core", "--policy", str(self.policy), "--destination", output),
                     ("backup-legacy-core", "--policy", str(self.policy), "--destination", output, "--writers-stopped"),
                     ("verify-core-upgrade", "--policy", str(self.policy), "--bundle", output),
                     ("verify-core-upgrade", "--policy", str(self.policy), "--bundle", output, "--candidate-copy", output)]:
            self.assertNotEqual(self.invoke(self.text, *args).returncode, 0)
