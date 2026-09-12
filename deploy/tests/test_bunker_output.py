"""Protect credential-bearing nak output at the shipped wrapper boundary."""
from pathlib import Path
import subprocess
import tempfile
import unittest

WRAPPER = Path(__file__).resolve().parents[1] / "scripts/run-nak-bunker"


class BunkerOutputTests(unittest.TestCase):
    def test_child_output_is_withheld_without_changing_exit_or_credentials(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            credentials = root / "credentials"
            credentials.mkdir()
            (credentials / "nostr-key").write_text("synthetic-key-never-real\n")
            stub = root / "nak"
            stub.write_text("""#!/usr/bin/python3
import os,sys
assert os.environ['NOSTR_SECRET_KEY'] == 'synthetic-key-never-real'
assert 'synthetic-key-never-real' not in sys.argv
assert '--persist' not in sys.argv
assert sys.argv[3:] == ['bunker','--authorized-keys','synthetic-public-key','ws://127.0.0.1:18547']
# Model both direct and formatted output, including a different pairing secret.
print(os.environ['NOSTR_SECRET_KEY'])
print('bunker://public?secret=synthetic-pairing-capability', file=sys.stderr)
sys.exit(17)
""")
            stub.chmod(0o700)
            result = subprocess.run(["/bin/sh", str(WRAPPER)], capture_output=True,
                env={"CREDENTIALS_DIRECTORY":str(credentials),
                     "RUNTIME_DIRECTORY":str(root), "NAK_BIN":str(stub),
                     "LG_NOSTR_CLIENT_PUBKEY":"synthetic-public-key",
                     "LG_NOSTR_RELAYS":"ws://127.0.0.1:18547"}, timeout=5)
            self.assertEqual(result.returncode, 17)
            self.assertEqual(result.stdout, b"")
            self.assertEqual(result.stderr, b"")

    def test_safe_preexec_configuration_error_remains_visible(self):
        result = subprocess.run(["/bin/sh", str(WRAPPER)], env={},
            capture_output=True, timeout=5)
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(result.stdout, b"")
        self.assertIn(b"systemd credentials directory is not set", result.stderr)


if __name__ == "__main__":
    unittest.main()
