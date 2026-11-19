"""Exercise the sealed first-wallet handoff using synthetic key material."""
import importlib.util
import os
from pathlib import Path
import sys
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch

SCRIPTS = Path(__file__).resolve().parents[1] / 'scripts'
TX_SPEC = importlib.util.spec_from_file_location('inactive_file_transaction', SCRIPTS / 'inactive_file_transaction.py')
TX = importlib.util.module_from_spec(TX_SPEC)
TX_SPEC.loader.exec_module(TX)
with patch.dict(sys.modules, {'inactive_file_transaction': TX}):
    SPEC = importlib.util.spec_from_file_location('cashu_wallet_keys', SCRIPTS / 'prepare-cashu-wallet-keys.py')
    KEYS = importlib.util.module_from_spec(SPEC)
    SPEC.loader.exec_module(KEYS)


@unittest.skipUnless(os.geteuid() == 0, 'wallet key fixtures require root and distinct owners')
class WalletKeyStage(unittest.TestCase):
    operator = (50510, 50510)
    wallet = (50511, 50511)
    project = 'a' * 64
    reserve = 'b' * 64
    seed = bytes(range(64))
    scalar = b'\x07' * 31 + b'\n'
    public = '03' + 'c' * 64

    def setUp(self):
        self.addCleanup(os.umask, os.umask(0o077))
        root = Path(self.enterContext(tempfile.TemporaryDirectory(prefix='lg-cashu-keys-', dir='/run')))
        base = root / 'upgrades'
        source = root / 'operator'
        base.mkdir()
        source.mkdir()
        self.source = source / 'wallet-seed.bin'
        self.source.write_bytes(self.seed)
        os.chown(self.source, *self.operator)
        os.chown(source, *self.operator)
        self.runtime = root / 'state'
        self.review = base / 'test-review'
        self.envelope = self.review / 'wallet-keys.encrypted'
        self.generated = self.decrypted = 0
        self.native = KEYS.FACTORY.run
        for owner, field, value in (
            (KEYS.C, 'BASE', base), (KEYS.C, 'root', lambda: None),
            (KEYS, 'verified_tool_identity', lambda: 'fixture tool identity'),
            (KEYS.pwd, 'getpwnam', lambda _: SimpleNamespace(pw_uid=self.operator[0])),
            (KEYS, 'guard', lambda: self.wallet), (KEYS, 'STATE', self.runtime),
            (KEYS.FACTORY, 'run', self.fake),
        ):
            self.enterContext(patch.object(owner, field, value))

    def fake(self, args, *, data=None, environment=None):
        self.assertNotIn(self.scalar.hex(), repr(args))
        if args[-2:] == ['key', 'generate']:
            self.generated += 1
            return self.scalar.hex().encode()
        if args[-3:] == ['key', 'public', '--with-parity']:
            self.assertEqual(data, self.scalar.hex().encode() + b'\n')
            return self.public.encode()
        if args[0] == '/usr/bin/systemd-creds':
            self.assertIn('--name=cashu-wallet-initial-keys', args)
            if 'encrypt' in args:
                self.assertIn('--with-key=host', args)
                self.assertEqual(data, self.source.read_bytes() + self.scalar)
                return b'TEST-WALLET-ENCRYPTED'
            self.assertIn('decrypt', args)
            self.assertEqual(data, b'TEST-WALLET-ENCRYPTED')
            self.decrypted += 1
            return self.seed + self.scalar
        if args[0] == '/usr/bin/mv':
            return self.native(args)
        self.assertEqual(args[:2], ['/usr/sbin/restorecon', '-R'])
        return b''

    def prepare(self, reserved=None, approved=True):
        return KEYS.prepare(self.review, self.source, 'fixture',
                            self.project, reserved or [self.reserve], approved)

    def test_exact_pair_installs_atomically_and_retries_without_an_operator_copy(self):
        first = self.prepare()
        self.assertEqual(first['receiving_key'], self.public)
        self.assertTrue(first['keys_staged'])
        self.assertFalse(self.runtime.exists())
        self.assertEqual(self.envelope.read_bytes(), b'TEST-WALLET-ENCRYPTED')
        repeated = self.prepare()
        self.assertFalse(repeated['keys_staged'])
        self.assertEqual(repeated['review_sha256'], first['review_sha256'])
        with self.assertRaisesRegex(ValueError, 'explicit wallet key installation'):
            KEYS.install(self.review, first['review_sha256'], False)
        self.source.unlink()
        installed = KEYS.install(self.review, first['review_sha256'], True)
        self.assertTrue(installed['state_created'])
        self.assertFalse(installed['services_started'])
        self.assertFalse(installed['journal_initialized'])
        again = KEYS.install(self.review, first['review_sha256'], True)
        self.assertFalse(again['state_created'])
        self.assertEqual(self.generated, 1)
        self.assertEqual({path.name for path in self.runtime.iterdir()}, set(KEYS.STATE_FILES))
        for name, value in zip(KEYS.STATE_FILES, (self.seed, self.scalar), strict=True):
            file = self.runtime / name
            self.assertEqual(file.read_bytes(), value)
            info = file.stat()
            self.assertEqual((info.st_uid, info.st_gid, info.st_mode & 0o777), (*self.wallet, 0o600))

    def test_review_and_ciphertext_drift_fail_before_decryption(self):
        first = self.prepare()
        with self.assertRaisesRegex(ValueError, 'reviewed wallet key binding differs'):
            KEYS.install(self.review, '1' * 64, True)
        self.envelope.write_bytes(b'DRIFT')
        with self.assertRaisesRegex(ValueError, 'reviewed wallet key binding differs'):
            KEYS.install(self.review, first['review_sha256'], True)
        self.assertEqual(self.decrypted, 1)
        self.assertFalse(self.runtime.exists())

    def test_retry_uses_original_pair_after_interruption_before_review(self):
        retain = KEYS.retain_record

        def interrupt(workspace, target, value):
            if target.name == 'REVIEW.json':
                raise RuntimeError('fixture interrupted review publication')
            retain(workspace, target, value)

        with patch.object(KEYS, 'retain_record', interrupt):
            with self.assertRaisesRegex(RuntimeError, 'interrupted review'):
                self.prepare()
        self.assertTrue(self.envelope.exists())
        retried = self.prepare()
        self.assertFalse(retried['keys_staged'])
        self.assertEqual(retried['receiving_key'], self.public)
        self.assertEqual(self.generated, 1)

    def test_existing_runtime_is_preserved_and_changed_operator_seed_is_refused(self):
        first = self.prepare()
        self.source.write_bytes(b'\x08' * 64)
        with self.assertRaisesRegex(ValueError, 'operator seed differs'):
            self.prepare()
        self.runtime.mkdir(mode=0o700)
        os.chown(self.runtime, *self.wallet)
        marker = self.runtime / 'acquisition.sqlite'
        marker.write_bytes(b'ORIGINAL')
        with self.assertRaisesRegex(ValueError, 'first-time wallet directory differs'):
            KEYS.install(self.review, first['review_sha256'], True)
        self.assertEqual(marker.read_bytes(), b'ORIGINAL')

    def test_symlinks_and_reserved_keys_refuse_before_installation(self):
        with self.assertRaisesRegex(ValueError, 'explicit wallet key preparation'):
            self.prepare(approved=False)
        incoming = self.source.with_name('incoming')
        self.source.rename(incoming)
        self.source.symlink_to(incoming)
        with self.assertRaises(OSError):
            self.prepare()
        self.assertEqual(self.generated, 0)
        self.source.unlink()
        incoming.rename(self.source)
        self.review = self.review.with_name('reserved-review')
        with self.assertRaisesRegex(ValueError, 'overlaps a reserved role'):
            self.prepare([self.public[2:]])
        self.assertFalse((self.review / 'wallet-keys.encrypted').exists())
        self.assertFalse(self.runtime.exists())
