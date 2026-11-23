"""Non-applying checks against explicitly selected standalone binaries."""
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]
PUBLIC = "79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798"


class Checks:
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="lg-outgoing-cli-")
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.config = self.root / "config.toml"
        self.text = (ROOT / "cyberherd" / self.example).read_text()
        self.text = self.text.replace("0" * 64, PUBLIC)
        self.text = self.text.replace("REPLACE_WITH_CLIENT_PUBLIC_KEY", "c6047f9441ed7d6d3045406e95c07cd85c778e4b8cef3ca7abac09b95c709ee5")
        self.text = self.text.replace("00000000-0000-0000-0000-000000000000", "11111111-1111-4111-8111-111111111111")
        for key, value in self.identities.items():
            self.text = self.text.replace(f"{key} = 0", f"{key} = {value}")
        self.text = self.text.replace(self.state, str(self.root / "uncreated-state"))

    def invoke(self, text, *args):
        self.config.write_text(text)
        result = subprocess.run([os.environ[self.binary], "--config", str(self.config), *args],
                                capture_output=True, text=True, check=False, timeout=15)
        self.assertEqual([p.name for p in self.root.iterdir()], ["config.toml"])
        return result

    def test_check_and_disabled_start_never_create_state_or_load_credentials(self):
        result = self.invoke(self.text, "--check")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("no runtime started", result.stdout)
        result = self.invoke(self.text)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("disabled", result.stderr)

    def test_offline_initialization_requires_protected_config_and_excludes_check(self):
        for args in [("--initialize-state",), ("--initialize-state", "--check")]:
            result = self.invoke(self.text, *args)
            self.assertNotEqual(result.returncode, 0)
            self.assertNotIn("CREDENTIALS_DIRECTORY", result.stderr)


@unittest.skipUnless(os.environ.get("LG_CYBERHERD_SIGNER_TEST_BINARY"), "select built signer")
class SignerCLI(Checks, unittest.TestCase):
    binary = "LG_CYBERHERD_SIGNER_TEST_BINARY"
    example = "signer.toml.example"
    identities = {"peer_uid": 10001, "group_gid": 10002}
    state = "/var/lib/cyberherd-signer"

    def test_no_arbitrary_signing_secret_fields_or_publishing_configuration(self):
        for text in [self.text + '\nnsec = "synthetic-private"\n',
                     'strike_api_key = "synthetic-private"\n' + self.text,
                     self.text.replace("relays = []", 'relays = ["wss://relay.damus.io/"]'),
                     self.text.replace("peer_uid = 10001", "peer_uid = 0")]:
            result=self.invoke(text,"--check")
            self.assertNotEqual(result.returncode,0)
            self.assertNotIn("synthetic-private",result.stderr)


@unittest.skipUnless(os.environ.get("LG_CYBERHERD_PAYOUT_TEST_BINARY"), "select built payout worker")
class PayoutCLI(Checks, unittest.TestCase):
    binary = "LG_CYBERHERD_PAYOUT_TEST_BINARY"
    example = "payout.toml.example"
    identities = {"core_uid": 10001, "intake_uid": 10002, "signer_uid": 10003}
    state = "/var/lib/cyberherd-payout"

    def test_identity_spend_defaults_and_unknown_credentials_fail_closed(self):
        for text in [self.text.replace("intake_uid = 10002", "intake_uid = 10001"),
                     self.text.replace("signer_uid = 10003", "signer_uid = 0"),
                     self.text.replace("poll_millis = 1000", "poll_millis = 0"),
                     self.text + '\napi_key = "synthetic-private"\n',
                     'nostr_private_key = "synthetic-private"\n' + self.text]:
            result=self.invoke(text,"--check")
            self.assertNotEqual(result.returncode,0)
            self.assertNotIn("synthetic-private",result.stderr)
        # Explicit zero spending limits are valid and retain their zero meaning.
        self.assertEqual(self.invoke(self.text, "--check").returncode, 0)

    def test_inclusive_fee_migration_is_explicit_and_excludes_other_operations(self):
        modes = [
            ("--migrate-inclusive-fees",),
            ("--writers-stopped",),
            ("--migrate-inclusive-fees", "--writers-stopped", "--check"),
            ("--migrate-inclusive-fees", "--writers-stopped", "--initialize-state"),
            ("--migrate-inclusive-fees", "--writers-stopped", "--sync-failed-cost",
             "--grant", "11111111-1111-4111-8111-111111111111", "--member", PUBLIC),
        ]
        for args in modes:
            with self.subTest(args=args):
                result=self.invoke(self.text,*args)
                self.assertNotEqual(result.returncode,0)
                self.assertTrue("required arguments" in result.stderr or "cannot be used with" in result.stderr,result.stderr)
                self.assertNotIn("no runtime started",result.stdout)
        # Correct flags still cannot use a caller-writable policy or create a DB.
        result=self.invoke(self.text,"--migrate-inclusive-fees","--writers-stopped")
        self.assertNotEqual(result.returncode,0)
        self.assertNotIn("CREDENTIALS_DIRECTORY",result.stderr)

    def test_cost_recovery_is_explicit_cannot_start_runtime_and_reads_no_sender_key(self):
        for args in [("--sync-failed-cost",),
                     ("--grant", "11111111-1111-4111-8111-111111111111"),
                     ("--sync-failed-cost", "--grant", "11111111-1111-4111-8111-111111111111", "--member", PUBLIC),
                     ("--sync-failed-cost", "--check", "--grant", "11111111-1111-4111-8111-111111111111", "--member", PUBLIC)]:
            result = self.invoke(self.text, *args)
            self.assertNotEqual(result.returncode, 0)
            self.assertNotIn("CREDENTIALS_DIRECTORY", result.stderr)
            self.assertNotIn("synthetic-private", result.stderr)


if __name__ == "__main__":
    unittest.main()
