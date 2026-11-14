"""Offline maintenance policy checks never grant file or service authority."""
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]


@unittest.skipUnless(os.environ.get("LG_CYBERHERD_STATE_TEST_BINARY"), "select built state tool")
class StateCLI(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="lg-state-cli-")
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.config = self.root / "state.toml"
        self.text = (ROOT / "deploy/cyberherd/state-maintenance.toml.example").read_text()
        self.text = self.text.replace("REPLACE_WITH_PROJECT_PUBLIC_KEY", "79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798")
        self.text = self.text.replace("00000000-0000-0000-0000-000000000000", "11111111-1111-4111-8111-111111111111")
        for i in range(4):
            self.text = self.text.replace("uid = 0", f"uid = {10001+i}", 1)
            self.text = self.text.replace("gid = 0", f"gid = {10001+i}", 1)

    def invoke(self, text, *args):
        self.config.write_text(text)
        result = subprocess.run([os.environ["LG_CYBERHERD_STATE_TEST_BINARY"], "--config", str(self.config), *args],
                                capture_output=True, text=True, check=False, timeout=15)
        self.assertEqual([p.name for p in self.root.iterdir()], ["state.toml"])
        self.assertNotIn("CREDENTIALS_DIRECTORY", result.stderr)
        self.assertNotIn("synthetic-forbidden", result.stderr)
        return result

    def test_policy_only_check_and_secret_identity_scope_rejections_create_no_state(self):
        result = self.invoke(self.text, "check")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("no state, credentials or services accessed", result.stdout)
        for text in ["api_key = 'synthetic-forbidden'\n" + self.text,
                     self.text.replace("uid = 10002", "uid = 10001"),
                     self.text.replace("strike-production", "unqualified"),
                     self.text.replace("11111111-1111-4111-8111-111111111111", "00000000-0000-0000-0000-000000000000")]:
            self.assertNotEqual(self.invoke(text, "check").returncode, 0)

    def test_writes_need_explicit_attestation_and_protected_root_policy(self):
        destination = str(self.root / "new-state")
        for args in [("snapshot", "--destination", destination),
                     ("snapshot", "--destination", destination, "--writers-stopped"),
                     ("verify", "--bundle", destination),
                     ("review-rewards", "--bundle", destination, "--destination", destination),
                     ("snapshot-before-contact", "--destination", destination),
                     ("snapshot-before-contact", "--destination", destination, "--writers-stopped"),
                     ("verify-contact-upgrade-source", "--bundle", destination),
                     ("compare-contact-upgrade", "--bundle", destination, "--candidate-copy", destination),
                     ("stage-restore", "--bundle", destination, "--destination", destination),
                     ("stage-restore", "--bundle", destination, "--destination", destination, "--writers-stopped")]:
            self.assertNotEqual(self.invoke(self.text, *args).returncode, 0)


if __name__ == "__main__":
    unittest.main()
