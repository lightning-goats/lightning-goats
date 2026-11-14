"""Fresh NIP-46 client namespaces; all test key/nak/crypto outputs are synthetic."""
import hashlib
import importlib.util
import os
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch

SCRIPTS = Path(__file__).resolve().parents[1] / 'scripts'
TX_SPEC = importlib.util.spec_from_file_location('inactive_file_transaction', SCRIPTS / 'inactive_file_transaction.py')
TX = importlib.util.module_from_spec(TX_SPEC)
TX_SPEC.loader.exec_module(TX)
with patch.dict(sys.modules, {'inactive_file_transaction': TX}):
    SPEC = importlib.util.spec_from_file_location('new_clients', SCRIPTS / 'prepare-cyberherd-nostr-clients.py')
    CLIENTS = importlib.util.module_from_spec(SPEC)
    SPEC.loader.exec_module(CLIENTS)


class ClientFlags(unittest.TestCase):
    def test_explicit_flag_and_distinct_public_bindings_are_required_before_key_access(self):
        with patch.object(CLIENTS.C, 'root'), patch.object(CLIENTS.C, 'protected', side_effect=AssertionError('key access')):
            for project, core, approved in [('a' * 64, 'b' * 64, False), ('a' * 64, 'a' * 64, True), ('invalid', 'b' * 64, True)]:
                with self.assertRaises(ValueError):
                    CLIENTS.prepare(project, core, approved)


@unittest.skipUnless(os.geteuid() == 0, 'protected client files require root')
class ClientFiles(unittest.TestCase):
    def test_distinct_creation_exact_reuse_and_cross_client_copy_refusal(self):
        old_umask = os.umask(0o077)
        self.addCleanup(os.umask, old_umask)
        with tempfile.TemporaryDirectory(prefix='lg-client-fixture-', dir='/run') as directory:
            root = Path(directory)
            root.chmod(0o700)
            nak, host = root / 'nak', root / 'host'
            nak.write_bytes(b'SYNTHETIC-NOT-NAK')
            nak.chmod(0o755)
            host.write_bytes(b'SYNTHETIC-HOST-KEY')
            host.chmod(0o600)
            counter = 0
            publics = {'01' * 32: '79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798',
                       '02' * 32: 'c6047f9441ed7d6d3045406e95c07cd85c778e4b8cef3ca7abac09b95c709ee5'}
            def fake(args, *, data=None, environment=None):
                nonlocal counter
                self.assertNotIn('01' * 32, repr(args))
                self.assertNotIn('02' * 32, repr(args))
                if args[-2:] == ['key', 'generate']:
                    counter += 1
                    return (f'{counter:02x}' * 32).encode()
                if args[-2:] == ['key', 'public']:
                    return publics[data.strip().decode()].encode()
                if args[0] == 'systemd-creds' and args[1] == 'encrypt':
                    Path(args[-1]).write_bytes(b'ENCRYPTED:' + data)
                    return b''
                if args[0] == 'systemd-creds' and args[1] == 'decrypt':
                    return Path(args[-2]).read_bytes().removeprefix(b'ENCRYPTED:')
                if args[0] == '/usr/sbin/restorecon':
                    return b''
                raise AssertionError('unexpected subprocess')
            with patch.object(CLIENTS.C, 'root'), patch.object(CLIENTS, 'HOST_KEY', host), patch.object(CLIENTS.FACTORY, 'STORE', root), patch.object(CLIENTS.FACTORY, 'NAK', nak), patch.object(CLIENTS.FACTORY, 'DIGEST', hashlib.sha256(nak.read_bytes()).hexdigest()), patch.object(CLIENTS.FACTORY, 'run', fake):
                first = CLIENTS.prepare('a' * 64, 'b' * 64, True)
                self.assertEqual(counter, 2)
                keys = {row['client_public_key'] for row in first['clients'].values()}
                self.assertEqual(len(keys), 2)
                self.assertEqual(first['events_published'], 0)
                self.assertFalse(first['household_changed'])
                before = {role: Path(row['credential']).read_bytes() for role, row in first['clients'].items()}
                second = CLIENTS.prepare('a' * 64, 'b' * 64, True)
                self.assertEqual(counter, 2)
                self.assertFalse(any(row['created'] for row in second['clients'].values()))
                for role, row in second['clients'].items():
                    path = Path(row['credential'])
                    self.assertEqual(path.read_bytes(), before[role])
                    self.assertEqual(path.stat().st_nlink, 1)
                    self.assertEqual(path.stat().st_mode & 0o777, 0o600)
                signer = Path(first['clients']['cyberherd-signer']['credential'])
                signer.write_bytes(before['cyberherd'])
                with self.assertRaisesRegex(ValueError, 'separate from project/core'):
                    CLIENTS.prepare('a' * 64, 'b' * 64, True)
                self.assertEqual(signer.read_bytes(), before['cyberherd'])
