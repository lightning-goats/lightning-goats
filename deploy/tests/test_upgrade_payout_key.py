"""Payout import never rotates credentials, calls an API or echoes a key."""
import hashlib
import importlib.util
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

SCRIPTS = Path(__file__).resolve().parents[1] / 'scripts'
TX_SPEC = importlib.util.spec_from_file_location('inactive_file_transaction', SCRIPTS / 'inactive_file_transaction.py')
TX = importlib.util.module_from_spec(TX_SPEC)
TX_SPEC.loader.exec_module(TX)
with patch.dict(sys.modules, {'inactive_file_transaction': TX}):
    SPEC = importlib.util.spec_from_file_location('payout_key_prepare', SCRIPTS / 'prepare-cyberherd-payout-key.py')
    KEY = importlib.util.module_from_spec(SPEC)
    SPEC.loader.exec_module(KEY)


class TokenBoundaries(unittest.TestCase):
    def test_exact_token_normalizes_only_surrounding_whitespace(self):
        self.assertEqual(KEY.token(b'  SYNTHETIC-NOT-A-REAL-KEY\r\n'), b'SYNTHETIC-NOT-A-REAL-KEY')
        for raw in [b'', b'a b', b'a\nb', b'a\x1bb', b'\xff', b'a' * 8193]:
            with self.assertRaises(ValueError):
                KEY.token(raw)

    def test_subprocess_errors_and_environment_cannot_expose_secret(self):
        fake = subprocess.CompletedProcess([], 1, b'SYNTHETIC-PRIVATE', b'SYNTHETIC-PRIVATE')
        with patch.object(KEY.subprocess, 'run', return_value=fake) as call:
            with self.assertRaises(ValueError) as failure:
                KEY.crypt('encrypt', b'SYNTHETIC-PRIVATE')
            self.assertNotIn('SYNTHETIC-PRIVATE', str(failure.exception))
            self.assertNotIn('SYNTHETIC-PRIVATE', repr(call.call_args.args[0]))
            self.assertEqual(set(call.call_args.kwargs['env']), {'PATH', 'LC_ALL'})


@unittest.skipUnless(os.geteuid() == 0, 'private credential filesystem fixture requires root')
class PayoutKeyFiles(unittest.TestCase):
    def test_exclusive_encrypted_creation_exact_reuse_and_changed_key_refusal(self):
        with tempfile.TemporaryDirectory(prefix='lg-payout-key-fixture-', dir='/run') as directory:
            root = Path(directory)
            root.chmod(0o700)
            store = root / 'store'
            store.mkdir(mode=0o700)
            source, host = root / 'input', root / 'host'
            source.write_bytes(b'SYNTHETIC-ONLY\n')
            host.write_bytes(b'SYNTHETIC-HOST-KEY')
            source.chmod(0o600)
            host.chmod(0o600)
            target = store / 'payout'
            def crypt(operation, raw):
                return b'ENCRYPTED:' + raw if operation == 'encrypt' else raw.removeprefix(b'ENCRYPTED:')
            with patch.object(KEY.C, 'root'), patch.object(KEY, 'STORE', store), patch.object(KEY, 'TARGET', target), patch.object(KEY, 'HOST_KEY', host), patch.object(KEY, 'crypt', crypt):
                first = KEY.prepare(source, True)
                self.assertTrue(first['created'])
                self.assertEqual(first['credential_fingerprint'], hashlib.sha256(b'SYNTHETIC-ONLY').hexdigest())
                self.assertEqual(target.stat().st_mode & 0o777, 0o600)
                before = target.read_bytes()
                self.assertFalse(KEY.prepare(source, True)['created'])
                source.write_bytes(b'DIFFERENT-SYNTHETIC-ONLY')
                with self.assertRaisesRegex(ValueError, 'reviewed rotation'):
                    KEY.prepare(source, True)
                self.assertEqual(target.read_bytes(), before)
                self.assertFalse(first['permissions_checked'])
                self.assertEqual(first['payments_sent'], 0)
