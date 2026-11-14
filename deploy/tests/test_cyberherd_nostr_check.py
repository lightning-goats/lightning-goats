"""Signing-check flow, frozen bytes and output secrecy; no remote signing here."""
import copy
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import pwd
import stat
import sys
import tempfile
import time
from types import SimpleNamespace
import unittest
from unittest.mock import patch
import uuid


SCRIPT = Path(__file__).resolve().parents[1] / 'scripts/check-cyberherd-nostr.py'
spec = importlib.util.spec_from_file_location('cyberherd_nostr_check', SCRIPT)
q = importlib.util.module_from_spec(spec)
spec.loader.exec_module(q)
CLIENT = '79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798'


class PinnedFixture:
    def __init__(self, path):
        self.path = path

    def __str__(self):
        return str(self.path)

    def read_bytes(self):
        return self.path.read_bytes()

    def lstat(self):
        return SimpleNamespace(st_mode=stat.S_IFREG | 0o755, st_uid=0)


class NostrCheckTests(unittest.TestCase):
    def event(self, expected):
        body = copy.deepcopy(expected)
        body['pubkey'] = q.AUTHOR
        raw = json.dumps([0, q.AUTHOR, body['created_at'], body['kind'], body['tags'], body['content']],
                         ensure_ascii=False, separators=(',', ':')).encode()
        body.update(id=hashlib.sha256(raw).hexdigest(), sig='1' * 128)
        return body

    def test_each_frozen_field_and_hash_must_match(self):
        for kind in (1, 1311, 9734):
            expected = q.challenge(kind, str(uuid.uuid4()), 100)
            signed = self.event(expected)
            q.verify_bytes(json.dumps(signed).encode(), expected)
            for field, bad in [('pubkey', CLIENT), ('kind', 0), ('created_at', 101),
                               ('tags', []), ('content', 'altered'), ('id', '0' * 64), ('sig', 'bad'), ('kind', True)]:
                wrong = dict(signed, **{field: bad})
                with self.assertRaises(q.Refusal):
                    q.verify_bytes(json.dumps(wrong).encode(), expected)
        for raw in (b'private malformed response', b'null', b'{"sig":5}'):
            with self.assertRaises(q.Refusal) as caught:
                q.verify_bytes(raw, expected)
            self.assertNotIn('private malformed', str(caught.exception))

    def test_roles_have_fixed_purposes_and_nonpublic_synthetic_targets(self):
        self.assertEqual(q.ROLES['membership'], ('cyberherd', (1, 1311)))
        self.assertEqual(q.ROLES['payout'], ('cyberherd-signer', (9734,)))
        self.assertEqual(q.ROLES['receive'], ('lightning-goats', (1,)))
        nonce = str(uuid.uuid4())
        chat = q.challenge(1311, nonce, 100)
        self.assertIn(['a', '30311:' + q.AUTHOR + ':signing-only-' + nonce, '', 'root'], chat['tags'])
        zap = q.challenge(9734, nonce, 100)
        self.assertIn(['p', q.RECIPIENT], zap['tags'])
        self.assertNotEqual(q.RECIPIENT, q.AUTHOR)
        with self.assertRaises(q.Refusal):
            q.challenge(0, nonce, 100)

    def test_private_nak_diagnostics_emit_only_known_labels_and_key_match(self):
        key = b'0' * 63 + b'1'
        raw = b'PRIVATE with client key ' + key + b'\nfailed to connect: error from bunker: response error: unauthorized SECRET'
        report = q.safe_trace(raw, key)
        self.assertTrue(report['nak_client_key_used_matches'])
        self.assertEqual(report['helper_categories'], ['unauthorized', 'error from bunker', 'failed to connect', 'response error: unauthorized'])
        encoded = json.dumps(report)
        self.assertNotIn('PRIVATE', encoded)
        self.assertNotIn('SECRET', encoded)
        self.assertNotIn(key.decode(), encoded)
        self.assertFalse(q.safe_trace(raw, b'1' * 64)['nak_client_key_used_matches'])
        self.assertIsNone(q.safe_trace(b'no key diagnostic', key)['nak_client_key_used_matches'])

    def test_real_child_private_diagnostic_is_not_forwarded_on_error_or_success(self):
        key = b'0' * 63 + b'1'
        code = "import sys;sys.stderr.write('PRIVATE with client key " + key.decode() + "\\nunauthorized SECRET');sys.exit(3)"
        with self.assertRaises(q.Refusal) as caught:
            q.run([sys.executable, '-c', code], b'', {}, expected_client_key=key)
        message = str(caught.exception)
        self.assertIn('"nak_client_key_used_matches": true', message)
        for private in ('PRIVATE', 'SECRET', key.decode()):
            self.assertNotIn(private, message)
        code = "import sys;sys.stderr.write('with client key " + key.decode() + "\\n');sys.stdout.write('OK')"
        self.assertEqual(q.run([sys.executable, '-c', code], b'', {}, expected_client_key=key), b'OK')
        with self.assertRaisesRegex(q.Refusal, 'not positively verified'):
            q.run([sys.executable, '-c', code], b'', {}, expected_client_key=b'1' * 64)

    def test_actual_child_output_and_deadline_are_bounded_and_sanitized(self):
        for code in ("import sys;sys.stdout.write('PRIVATE'*12000)",
                     "import sys;sys.stderr.write('PRIVATE'*12000)",
                     "import sys;sys.stderr.write('PRIVATE unauthorized');sys.exit(3)"):
            with self.assertRaises(q.Refusal) as caught:
                q.run([sys.executable, '-c', code], b'', {})
            self.assertNotIn('PRIVATE', str(caught.exception))
        started = time.monotonic()
        with self.assertRaisesRegex(q.Refusal, 'timed out'):
            q.run([sys.executable, '-c', 'import time;time.sleep(10)'], b'', {}, deadline=.1)
        self.assertLess(time.monotonic() - started, 2)
        with self.assertRaisesRegex(q.Refusal, 'input exceeds'):
            q.run(['/never/exec'], b'x' * 4097, {})

    def test_bounded_observer_returns_only_collected_stdout_at_fixed_deadline(self):
        code = "import sys,time;print('ENCRYPTED',flush=True);sys.stderr.write('PRIVATE');time.sleep(10)"
        self.assertEqual(q.run([sys.executable, '-c', code], b'', {}, deadline=.2,
                               collect_until_deadline=True), b'ENCRYPTED\n')

    def test_connect_observation_reports_conflicts_without_response_or_rpc_ids(self):
        events = []
        for index in range(3):
            events.append(json.dumps({'kind': 24133, 'pubkey': q.AUTHOR, 'tags': [['p', CLIENT]],
                                      'id': str(index), 'content': 'ENCRYPTED_PRIVATE_' + str(index)}).encode())
        responses = iter([{'id': 'PRIVATE_RPC-connect', 'result': 'ack', 'error': ''},
                          {'id': 'PRIVATE_RPC-connect', 'result': '', 'error': 'unauthorized'},
                          {'id': 'SECRET-sign_event', 'result': 'PRIVATE_SIGNED_BYTES', 'error': ''}])
        def helper(arguments, data, environment, **kwargs):
            if arguments[-1] == 'verify':
                return b''
            self.assertEqual(arguments[-4:-2], ['decrypt', '--pubkey'])
            self.assertIn('NOSTR_SECRET_KEY', environment)
            return json.dumps(next(responses)).encode()
        with patch.object(q, 'run', side_effect=helper):
            result = q.connect_observation(b'\n'.join(events), b'0' * 63 + b'1', CLIENT, ['nak'])
        self.assertEqual(result['connect_rpc_groups'], 1)
        self.assertEqual(result['conflicting_groups'], 1)
        self.assertEqual(result['ack_groups'], 1)
        self.assertEqual(result['unauthorized_groups'], 1)
        self.assertNotIn('PRIVATE', json.dumps(result))
        self.assertNotIn('SECRET', json.dumps(result))
        with self.assertRaisesRegex(q.Refusal, 'event bound'):
            q.connect_observation(b'{}\n' * 33, b'', CLIENT, ['nak'])

    def test_root_refuses_before_helper_or_credential_access(self):
        with patch.object(q.os, 'geteuid', return_value=0), patch.object(q.pwd, 'getpwnam', return_value=SimpleNamespace(pw_uid=0, pw_gid=0)):
            with self.assertRaisesRegex(q.Refusal, 'non-admin'):
                q.check('membership', CLIENT, ['wss://relay.primal.net'])

    def test_temporary_units_keep_credentials_separate_and_hide_all_journals(self):
        for role, user, credential in [('membership', 'cyberherd', 'cyberherd-nostr-client'),
                                        ('payout', 'cyberherd-signer', 'cyberherd-signer-client')]:
            text = (SCRIPT.parent.parent / 'systemd' / ('cyberherd-nostr-' + role + '-check.service.example')).read_text()
            self.assertIn('User=' + user + '\n', text)
            self.assertIn('LoadCredentialEncrypted=nostr-client-key:/etc/credstore.encrypted/' + credential, text)
            self.assertIn('--role ' + role, text)
            self.assertIn('Type=oneshot', text)
            self.assertIn('NoNewPrivileges=yes', text)
            self.assertIn('IPAddressDeny=10.0.0.0/8', text)
            self.assertNotIn('StateDirectory=', text)
            self.assertNotIn('WantedBy=', text)
            self.assertNotIn('strike-payout-api-key', text)
            for state in ('lightning-goats', 'cyberherd', 'cyberherd-payout', 'cyberherd-signer'):
                self.assertIn('-/var/lib/' + state, text)

    def test_fake_helper_flow_has_no_publication_argument_or_inherited_secret(self):
        # This tests orchestration, not cryptography; actual signature checks use
        # the independently pinned nak in the installed household round trip.
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            runtime = root / 'runtime'; runtime.mkdir(mode=0o700)
            creds = root / 'creds'; creds.mkdir(mode=0o700)
            (creds / 'nostr-client-key').write_text('0' * 63 + '1')
            log = root / 'calls.jsonl'
            helper = root / 'nak'
            helper.write_text('#!' + sys.executable + '\n' + f'''
import hashlib,json,os,sys
raw=sys.stdin.buffer.read(); args=sys.argv[1:]
with open({str(log)!r},'a') as f:
 f.write(json.dumps({{'args':args,'secret_inherited':'UNRELATED_PRIVATE' in os.environ,'auth': 'NOSTR_CLIENT_KEY' in os.environ}})+'\\n')
if args[-2:]==['key','public']:
 print({CLIENT!r})
elif args[-1]=='event':
 e=json.loads(raw);e['pubkey']={q.AUTHOR!r}
 canonical=json.dumps([0,e['pubkey'],e['created_at'],e['kind'],e['tags'],e['content']],ensure_ascii=False,separators=(',',':')).encode()
 e.update(id=hashlib.sha256(canonical).hexdigest(),sig='1'*128);print(json.dumps(e))
elif args[-1]=='verify':
 assert 'NOSTR_CLIENT_KEY' not in os.environ
else:
 raise SystemExit(97)
''')
            helper.chmod(0o700)
            identity = pwd.getpwuid(os.geteuid())
            with (patch.object(q, 'NAK', PinnedFixture(helper)),
                  patch.object(q, 'DIGEST', hashlib.sha256(helper.read_bytes()).hexdigest()),
                  patch.object(q.pwd, 'getpwnam', return_value=identity),
                  patch.dict(os.environ, {'RUNTIME_DIRECTORY': str(runtime), 'CREDENTIALS_DIRECTORY': str(creds), 'UNRELATED_PRIVATE': 'not inherited'})):
                results = [q.check(role, CLIENT, ['wss://relay.primal.net']) for role in ('membership', 'payout')]
            self.assertEqual([r['kinds_verified'] for r in results], [[1, 1311], [9734]])
            self.assertTrue(all(r['project_events_published'] == r['invoice_requests'] == r['provider_operations'] == 0 for r in results))
            calls = [json.loads(line) for line in log.read_text().splitlines()]
            self.assertEqual(len(calls), 8)
            self.assertFalse(any(c['secret_inherited'] for c in calls))
            self.assertEqual(sum(c['auth'] for c in calls), 3)
            for call in calls:
                self.assertIn(call['args'][-1], ('public', 'event', 'verify'))
                self.assertFalse(any(a.startswith(('ws:', 'wss:')) for a in call['args']))


if __name__ == '__main__':
    unittest.main()
