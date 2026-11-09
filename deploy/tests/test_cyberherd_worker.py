"""Non-applying CLI checks against an explicitly selected built worker."""
import os
from pathlib import Path
import subprocess
import tempfile
import unittest


@unittest.skipUnless(os.environ.get("LG_CYBERHERDD_TEST_BINARY"), "select a built cyberherdd binary")
class CyberHerdWorkerCLI(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="lg-cyberherd-cli-")
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.config = self.root / "config.toml"
        source = Path(__file__).resolve().parents[1] / "cyberherd/config.toml.example"
        self.text = source.read_text().replace("core_uid = 0", "core_uid = 10001")
        self.text = self.text.replace("00000000-0000-0000-0000-000000000000", "11111111-1111-4111-8111-111111111111")
        public = "79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798"
        self.text = self.text.replace("0" * 64, public)
        self.text = self.text.replace("/var/lib/cyberherd", str(self.root / "uncreated-state"))
        self.text = self.text.replace("/run/lightning-goats-bridge/core.sock", str(self.root / "uncreated.sock"))

    def invoke(self, text, *args):
        self.config.write_text(text)
        result = subprocess.run(
            [os.environ["LG_CYBERHERDD_TEST_BINARY"], "--config", str(self.config), *args],
            text=True, capture_output=True, timeout=15, check=False,
        )
        self.assertEqual([p.name for p in self.root.iterdir()], ["config.toml"])
        return result

    def test_check_validates_without_creating_state_or_starting_disabled_worker(self):
        result = self.invoke(self.text, "--check")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("no runtime started", result.stdout)
        result = self.invoke(self.text)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("disabled", result.stderr)

    def test_zero_core_identity_and_secret_fields_fail_before_runtime(self):
        for text in [self.text.replace("core_uid = 10001", "core_uid = 0"),
                     'strike_api_key = "synthetic-placeholder"\n' + self.text,
                     self.text.replace("required_relays = 1", "required_relays = 8")]:
            self.assertNotEqual(self.invoke(text, "--check").returncode, 0)

    def test_optional_planning_is_bounded_and_accepts_no_spending_credential(self):
        policy = '''
[rewards]
fee_basis_points = 0
minimum_fee_sats = 1
minimum_net_sats = 1
profile_max_age_seconds = 3600
operation_addresses = ["herd@lightning-goats.com"]
'''
        result = self.invoke(self.text + policy, "--check")
        self.assertEqual(result.returncode, 0, result.stderr)
        for invalid in [policy.replace("fee_basis_points = 0", "fee_basis_points = 10001"),
                        policy.replace("minimum_net_sats = 1", "minimum_net_sats = 0"),
                        policy.replace("profile_max_age_seconds = 3600", "profile_max_age_seconds = 0"),
                        policy + 'strike_api_key = "synthetic-placeholder"\n']:
            self.assertNotEqual(self.invoke(self.text + invalid, "--check").returncode, 0)


if __name__ == "__main__":
    unittest.main()
