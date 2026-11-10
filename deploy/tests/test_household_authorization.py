"""Disposable allowlist files; no household process or signing credential access."""
import contextlib
import importlib.util
import io
import os
from pathlib import Path
import tempfile
from types import SimpleNamespace
import unittest
from unittest import mock


SCRIPT = Path(__file__).resolve().parents[1] / 'scripts' / 'authorize-household-nostr-client.py'


class HouseholdAuthorizationTests(unittest.TestCase):
    def setUp(self):
        spec = importlib.util.spec_from_file_location('household_authorization', SCRIPT)
        self.module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(self.module)
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.path = Path(self.directory.name) / 'operator.env'
        self.original = ('BUNKER_AUTHORIZED_CLIENTS="' + '1' * 64 + '"\n'
                         'BUNKER_RELAYS="wss://relay.damus.io/"\n')
        self.path.write_text(self.original)
        self.path.chmod(0o600)

    def invoke(self, clients):
        lstat = Path.lstat
        metadata = SimpleNamespace(st_mode=self.path.lstat().st_mode, st_uid=0, st_gid=0)
        # Simulate only root ownership/chown; files and replacement are real,
        # and no privileged household operation occurs in this fixture.
        def fixture_stat(path, *args, **kwargs):
            return metadata if path == self.path else lstat(path, *args, **kwargs)
        original_umask = os.umask(0o077)
        try:
            with (mock.patch.object(self.module, 'ENVIRONMENT', self.path),
                  mock.patch.object(self.module.os, 'geteuid', return_value=0),
                  mock.patch.object(self.module.os, 'chown'),
                  mock.patch.object(Path, 'lstat', fixture_stat),
                  mock.patch.object(self.module.subprocess, 'run') as run,
                  mock.patch('sys.argv', ['household-authorization', *clients]),
                  contextlib.redirect_stdout(io.StringIO())):
                self.module.main()
                return run.call_args_list
        finally:
            os.umask(original_umask)

    def test_two_clients_preserve_existing_allowlist_and_restart_only_once(self):
        clients = ['2' * 64, '3' * 64]
        calls = self.invoke(clients)
        after = self.path.read_text()
        for client in ['1' * 64, *clients]:
            self.assertEqual(after.count(client), 1)
        self.assertIn('BUNKER_RELAYS="wss://relay.damus.io/"', after)
        self.assertEqual(self.path.stat().st_mode & 0o777, 0o600)
        backups = list(self.path.parent.glob('operator.env.lg-*'))
        self.assertEqual(len(backups), 1)
        self.assertEqual(backups[0].read_text(), self.original)
        self.assertEqual(sum(c.args[0][1:2] == ['restart'] for c in calls), 1)
        retry = self.invoke(clients)
        self.assertEqual(self.path.read_text(), after)
        self.assertEqual(list(self.path.parent.glob('operator.env.lg-*')), backups)
        self.assertFalse(any(c.args[0][1:2] == ['restart'] for c in retry))

    def test_invalid_second_client_cannot_partially_replace_or_back_up_file(self):
        with self.assertRaisesRegex(ValueError, 'public key'):
            self.invoke(['2' * 64, 'invalid-public-client'])
        self.assertEqual(self.path.read_text(), self.original)
        self.assertEqual(list(self.path.parent.iterdir()), [self.path])


if __name__ == '__main__':
    unittest.main()
