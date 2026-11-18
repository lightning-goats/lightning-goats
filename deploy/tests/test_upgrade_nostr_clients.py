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
    CASHU_SPEC = importlib.util.spec_from_file_location('cashu_client', SCRIPTS / 'prepare-cashu-signer-client.py')
    CASHU = importlib.util.module_from_spec(CASHU_SPEC)
    CASHU_SPEC.loader.exec_module(CASHU)


class ClientFlags(unittest.TestCase):
    def test_explicit_flag_and_distinct_public_bindings_are_required_before_key_access(self):
        with patch.object(CLIENTS.C, 'root'), patch.object(CLIENTS.C, 'protected', side_effect=AssertionError('key access')):
            for project, core, approved in [('a' * 64, 'b' * 64, False), ('a' * 64, 'a' * 64, True), ('invalid', 'b' * 64, True)]:
                with self.assertRaises(ValueError):
                    CLIENTS.prepare(project, core, approved)

    def test_cashu_requires_explicit_operations_before_opening_any_credential(self):
        with patch.object(CASHU.C, 'root'), patch.object(CASHU, 'bundle_guard', side_effect=AssertionError('key access')):
            with self.assertRaises(ValueError):
                CASHU.prepare(Path('/unused'), 'a' * 64, [], False)
            with self.assertRaises(ValueError):
                CASHU.install(Path('/unused'), 'b' * 64, False)


@unittest.skipUnless(os.geteuid() == 0, 'protected client files require root')
class CashuClientFiles(unittest.TestCase):
    project = 'a' * 64
    reserved = 'b' * 64
    public = 'c' * 64
    secret = b'01' * 32

    def setUp(self):
        old_umask = os.umask(0o077)
        self.addCleanup(os.umask, old_umask)
        root = Path(self.enterContext(tempfile.TemporaryDirectory(prefix='lg-cashu-client-', dir='/run')))
        base, store = root / 'upgrades', root / 'creds'
        base.mkdir(mode=0o700)
        store.mkdir(mode=0o700)
        nak, host = root / 'nak', root / 'host'
        nak.write_bytes(b'SYNTHETIC-NOT-NAK')
        nak.chmod(0o755)
        host.write_bytes(b'SYNTHETIC-HOST-KEY')
        self.review = base / 'test'
        self.retained = self.review / 'cashu-client.encrypted'
        self.installed = store / 'lg-cashu-nostr-client'
        self.generated = 0
        self.decryptions = 0
        for owner, attribute, value in (
            (CASHU.C, 'root', lambda: None), (CASHU.C, 'BASE', base),
            (CASHU, 'HOST_KEY', host), (CASHU.P, 'require_inactive_cashu', lambda: None),
            (CASHU, 'verified_tool_identity', lambda: 'e' * 64),
            (CASHU.FACTORY, 'STORE', store), (CASHU.FACTORY, 'NAK', nak),
            (CASHU.FACTORY, 'DIGEST', hashlib.sha256(nak.read_bytes()).hexdigest()),
            (CASHU.FACTORY, 'run', self.run_fake),
        ):
            self.enterContext(patch.object(owner, attribute, value))

    def run_fake(self, args, *, data=None, environment=None):
        self.assertNotIn(self.secret.decode(), repr(args))
        if args[-2:] == ['key', 'generate']:
            self.generated += 1
            return self.secret
        if args[-2:] == ['key', 'public']:
            self.assertEqual(data, self.secret + b'\n')
            return self.public.encode()
        if args[0] == 'systemd-creds':
            self.assertIn('--name=cashu-nostr-client', args)
            if args[1] == 'encrypt':
                self.assertIn('--with-key=host', args)
                self.assertEqual(data, self.secret + b'\n')
                Path(args[-1]).write_bytes(b'SYNTHETIC-ENCRYPTED')
                return b''
            self.assertEqual(args[1], 'decrypt')
            self.assertEqual(Path(args[-2]).read_bytes(), b'SYNTHETIC-ENCRYPTED')
            self.decryptions += 1
            return self.secret
        self.assertEqual(args[0], '/usr/sbin/restorecon')
        return b''

    def prepare(self, reserved=None):
        return CASHU.prepare(self.review, self.project, reserved or [self.reserved], True)

    def test_review_and_install_preserve_one_dedicated_key(self):
        result = self.prepare()
        self.assertTrue(result['credential_created'])
        self.assertFalse(result['installed'])
        self.assertFalse(self.installed.exists())
        self.assertEqual(result['client_public_key'], self.public)
        saved = self.retained.read_bytes()
        second = self.prepare()
        self.assertEqual(second['review_sha256'], result['review_sha256'])
        self.assertFalse(second['credential_created'])
        with self.assertRaisesRegex(ValueError, 'different client request'):
            self.prepare(['d' * 64])
        first_install = CASHU.install(self.review, result['review_sha256'], True)
        self.assertTrue(first_install['credential_created'])
        self.assertFalse(first_install['services_started'])
        second_install = CASHU.install(self.review, result['review_sha256'], True)
        self.assertFalse(second_install['credential_created'])
        self.assertEqual(second_install['client_public_key'], self.public)
        self.assertEqual(self.generated, 1)
        for ciphertext in (self.retained, self.installed):
            self.assertEqual(ciphertext.read_bytes(), saved)
            self.assertEqual(ciphertext.stat().st_mode & 0o777, 0o600)
            self.assertEqual(ciphertext.stat().st_nlink, 1)

    def test_mismatched_review_and_ciphertext_fail_before_decryption(self):
        result = self.prepare()
        with self.assertRaisesRegex(ValueError, 'review digest differs'):
            CASHU.install(self.review, '1' * 64, True)
        self.retained.write_bytes(b'TAMPERED')
        with self.assertRaisesRegex(ValueError, 'reviewed ciphertext differs'):
            CASHU.install(self.review, result['review_sha256'], True)
        self.assertEqual(self.decryptions, 1)
        self.assertFalse(self.installed.exists())

    def test_preserves_existing_destination_and_refuses_overlapping_client(self):
        with self.assertRaisesRegex(ValueError, 'overlaps a reserved role'):
            self.prepare([self.public])
        self.assertFalse(self.retained.exists())
        self.review = self.review.with_name('new-review')
        result = self.prepare()
        self.installed.write_bytes(b'EXISTING')
        with self.assertRaisesRegex(ValueError, 'installed credential differs'):
            CASHU.install(self.review, result['review_sha256'], True)
        self.assertEqual(self.installed.read_bytes(), b'EXISTING')


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
