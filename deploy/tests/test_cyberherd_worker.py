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

    def test_initialization_is_separate_requires_protected_configuration_and_creates_no_runtime(self):
        for args in [("--initialize-state",), ("--initialize-state", "--check")]:
            result = self.invoke(self.text, *args)
            self.assertNotEqual(result.returncode, 0)
            self.assertNotIn("CREDENTIALS_DIRECTORY", result.stderr)

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

    def test_optional_messaging_check_reads_no_credential_and_rejects_raw_keys(self):
        messaging = '''
[messaging]
enabled = false
start_after = 0
poll_millis = 1000
relays = ["wss://relay.damus.io/"]
'''
        self.assertEqual(self.invoke(self.text + messaging, "--check").returncode, 0)
        public = "79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798"
        client = "c6047f9441ed7d6d3045406e95c07cd85c778e4b8cef3ca7abac09b95c709ee5"
        signer = f'''
[messaging.signer]
nak_path = "/uninstalled/nak"
bunker_pubkey = "{public}"
client_pubkey = "{client}"
bunker_relays = ["wss://nos.lol/"]
'''
        self.assertEqual(self.invoke(self.text + messaging + signer, "--check").returncode, 0)
        self.assertNotEqual(self.invoke(self.text + messaging.replace("enabled = false", "enabled = true") + "lifecycle = true\n", "--check").returncode, 0)
        public_notices = messaging.replace("enabled = false", "enabled = true") + "lifecycle = true\n" + signer
        self.assertEqual(self.invoke(self.text + public_notices, "--check").returncode, 0)
        for invalid in [messaging.replace("start_after = 0", "start_after = -1"),
                        messaging.replace("poll_millis = 1000", "poll_millis = 0"),
                        messaging + signer.replace(client, public),
                        messaging + signer + 'nsec = "synthetic-placeholder"\n']:
            self.assertNotEqual(self.invoke(self.text + invalid, "--check").returncode, 0)

    def test_public_view_check_is_loopback_only_and_creates_no_state(self):
        view = '\n[view]\nenabled = false\nlisten = "127.0.0.1:3340"\noverlay = true\n'
        self.assertEqual(self.invoke(self.text + view, "--check").returncode, 0)
        for invalid in [view.replace("127.0.0.1:3340", "0.0.0.0:3340"),
                        view.replace("127.0.0.1:3340", "10.8.0.6:3340"),
                        view.replace("127.0.0.1:3340", "127.0.0.1:0"),
                        view + 'api_key = "synthetic-placeholder"\n']:
            self.assertNotEqual(self.invoke(self.text + invalid, "--check").returncode, 0)

    def test_http_auth_has_pinned_public_origin_allowlist_and_no_secret_fields(self):
        public = "79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798"
        view = '\n[view]\nenabled = false\nlisten = "127.0.0.1:3340"\n[view.auth]\npublic_origin = "https://example.com"\noperators = ["' + public + '"]\n'
        self.assertEqual(self.invoke(self.text + view, "--check").returncode, 0)
        for invalid in [view.replace("https://example.com", "http://example.com"),
                        view.replace("https://example.com", "https://10.8.0.6"),
                        view + 'nsec = "synthetic-placeholder"\n',
                        view.replace('operators = ["' + public + '"]', 'operators = ["' + public + '","' + public + '"]')]:
            self.assertNotEqual(self.invoke(self.text + invalid, "--check").returncode, 0)


if __name__ == "__main__":
    unittest.main()
