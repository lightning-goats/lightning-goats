"""Synthetic process arguments only; no real credential or household access."""
import contextlib
import hashlib
import importlib.util
import io
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch


SCRIPT = Path(__file__).resolve().parents[1] / 'scripts/check-household-cyberherd-clients.py'
spec = importlib.util.spec_from_file_location('household_client_inspection', SCRIPT)
q = importlib.util.module_from_spec(spec)
spec.loader.exec_module(q)


class HouseholdClientInspectionTests(unittest.TestCase):
    def test_only_explicit_public_key_flags_are_selected(self):
        keys, malformed = q.public_clients(['nak', '--sec', 'SECRET', '--authorized-keys', q.CLIENTS['receive'],
                                           '-k', q.CLIENTS['membership'], '--authorized-keys=' + q.CLIENTS['payout_signer'],
                                           '--authorized-secrets', 'SECRET', '--authorized-keys', 'not-a-public-key'])
        self.assertEqual(keys, {q.CLIENTS[n] for n in ('receive', 'membership', 'payout_signer')})
        self.assertEqual(malformed, 1)
        self.assertNotIn('SECRET', str((keys, malformed)))

    def test_report_has_only_known_public_comparisons_not_raw_arguments(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary); process = root / '42'; process.mkdir()
            program = b'public test-only executable bytes'
            (process / 'exe').write_bytes(program)
            arguments = ['nak', '--sec', 'VERY_PRIVATE_TEST_SECRET', '--authorized-keys', q.CLIENTS['receive'],
                         '--authorized-keys', q.CLIENTS['membership']]
            (process / 'cmdline').write_bytes(('\0'.join(arguments) + '\0').encode())
            environment = root / 'operator.env'
            environment.write_text('BUNKER_AUTHORIZED_CLIENTS="' + ' '.join(q.CLIENTS.values()) + '"\nNAK_SHA256=' + hashlib.sha256(program).hexdigest() + '\n')
            values = {'Id': q.UNIT, 'MainPID': '42', 'ActiveState': 'active', 'SubState': 'running', 'NRestarts': '0', 'ExecMainStatus': '0'}
            with (patch.object(q.os, 'geteuid', return_value=0), patch.object(q, 'ENVIRONMENT', environment),
                  patch.object(q, 'PROC', root), patch.object(q, 'status', return_value=values)):
                report = q.inspect()
            self.assertTrue(report['running_clients']['membership'])
            self.assertFalse(report['running_clients']['payout_signer'])
            self.assertTrue(all(report['configured_clients'].values()))
            self.assertTrue(report['running_program_matches_configured_digest'])
            encoded = json.dumps(report)
            self.assertNotIn('VERY_PRIVATE', encoded)
            self.assertNotIn(q.CLIENTS['membership'], encoded)
            self.assertFalse(any(report[n] for n in ('changes_made', 'credential_files_read',
                                                      'process_environment_read', 'raw_arguments_emitted')))

    def test_unexpected_error_prints_no_input_or_traceback(self):
        output = io.StringIO()
        with patch.object(q, 'inspect', side_effect=ValueError('VERY_PRIVATE')), contextlib.redirect_stdout(output):
            with self.assertRaises(SystemExit) as caught:
                q.main()
        self.assertNotIn('VERY_PRIVATE', str(caught.exception))
        self.assertEqual(output.getvalue(), '')

    def test_multiple_bunker_inventory_never_emits_secret_arguments_or_unknown_keys(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            for pid, program, command, client in [('42', 'nak', 'bunker', q.CLIENTS['membership']),
                                                   ('43', 'nak-old', 'bunker', 'f' * 64),
                                                   ('44', 'nak', 'event', q.CLIENTS['receive'])]:
                process = root / pid; process.mkdir()
                (process / 'exe').write_bytes(b'public executable')
                (process / 'cmdline').write_bytes(('\0'.join([program, command, '--sec', 'VERY_PRIVATE',
                                                            '--authorized-keys', client]) + '\0').encode())
            with patch.object(q, 'PROC', root):
                report = q.bunker_processes('42')
            self.assertEqual(report['bunker_process_count'], 2)
            self.assertEqual(sum(r['selected_service'] for r in report['bunker_processes']), 1)
            self.assertTrue(any(r['known_clients']['membership'] for r in report['bunker_processes']))
            encoded = json.dumps(report)
            for secret in ('VERY_PRIVATE', 'f' * 64, q.CLIENTS['membership']):
                self.assertNotIn(secret, encoded)


if __name__ == '__main__':
    unittest.main()
