"""Cutoff coordinator: protected files and mocked native/service operations.

The mandatory Rust UID fixture qualifies native audit application/comparison.
These tests never operate a real service, SQL journal, payment or feeder.
"""
import importlib.util
import json
import os
from pathlib import Path
import shutil
import tempfile
import sys
import unittest
import uuid
from unittest.mock import patch

SCRIPTS = Path(__file__).resolve().parents[1] / 'scripts'
spec = importlib.util.spec_from_file_location('reward_cutover_fixture', SCRIPTS / 'maintain-cyberherd-reward-cutover.py')
K = importlib.util.module_from_spec(spec)
import test_upgrade_financial_activation as support
with patch.dict(sys.modules, {'inactive_file_transaction': support.H.TX}):
    spec.loader.exec_module(K)


class CutoffBoundaries(unittest.TestCase):
    def test_approval_refuses_before_loading_or_locking(self):
        with patch.object(K.H, 'load') as load, patch.object(K.M, 'exclusive') as lock:
            for fn, args in [(K.apply, [Path('/no-read'), 'a', 'b', 'c', 'd', False]),
                             (K.resume, [Path('/no-read'), 'a', 'b', 'c', 'd', 'e', False])]:
                with self.assertRaises(ValueError):
                    fn(*args)
            load.assert_not_called()
            lock.assert_not_called()

    def test_native_acknowledgments_are_exact_and_grant_no_activation(self):
        value = {'operation': 'initial_reward_cutover_compare', 'id': str(uuid.uuid4()),
                 'accepted': True, 'resume_authorized': False, 'decision_sha256': 'ab' * 32}
        with patch.object(K.C, 'protected'), patch.object(K.C, 'command', return_value=json.dumps(value)):
            self.assertEqual(K.native(Path('/private'), 'compare-initial-reward-cutover'), value)
        for change in [{'accepted': False}, {'resume_authorized': True}, {'extra': 1},
                       {'operation': 'snapshot'}, {'id': 'not-uuid'}]:
            with self.subTest(change=change), patch.object(K.C, 'protected'), patch.object(K.C, 'command',
                    return_value=json.dumps(dict(value, **change))), self.assertRaises((ValueError, KeyError)):
                K.native(Path('/private'), 'compare-initial-reward-cutover')

    def test_decision_digest_matches_native_struct_order_and_pins_inventory(self):
        value = {'id': '11111111-1111-4111-8111-111111111111',
            'epoch': '22222222-2222-4222-8222-222222222222', 'through': 2,
            'backup_manifest_sha256': 'ab' * 32, 'decided_at': 100,
            'historical': [{'request_id': '33333333-3333-4333-8333-333333333333',
                            'core_seq': 2, 'proof_hash': 'cd' * 32}]}
        expected = '325896aa1c22239c825da04a80199489a115bc1f4b982a30e2ec529abbc1927d'
        self.assertEqual(K.decision_hash(value), expected)
        reordered = dict(reversed(list(value.items())))
        reordered['historical'] = [dict(reversed(list(value['historical'][0].items())))]
        self.assertEqual(K.decision_hash(reordered), expected)
        self.assertNotEqual(K.decision_hash(dict(value, decided_at=101)), expected)
        with self.assertRaises(ValueError):
            K.decision_hash(dict(value, extra=True))

    def test_direct_script_execution_refuses_before_loading_support(self):
        import subprocess
        result = subprocess.run(['python3', str(SCRIPTS / 'maintain-cyberherd-reward-cutover.py'), '--help'],
                                capture_output=True, text=True, check=False)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('externally verified root-owned', result.stderr)


@unittest.skipUnless(os.geteuid() == 0, 'protected root coordinator fixtures run separately in CI')
class CutoffFiles(unittest.TestCase):
    def replace(self, obj, name, value):
        ctx = patch.object(obj, name, value)
        ctx.start()
        self.addCleanup(ctx.stop)

    def write(self, path, value, mode=0o600):
        path.write_bytes(value)
        path.chmod(mode)

    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix='lg-cutoff-coordinator-', dir='/run')
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.root.chmod(0o700)
        self.directory = self.root / 'financial'
        self.directory.mkdir(mode=0o700)
        (self.directory / 'LOCK').touch(mode=0o600)
        self.replace(K.C, 'BASE', self.root)
        self.replace(K.M, 'C', K.C)
        self.replace(K, 'verified_tool_identity', lambda: {'fixture': 'root-private-cutoff'})
        self.replace(K.C, 'command', self.command)
        self.carriers, self.starts, self.member_calls = set(), [], []
        self.addCleanup(self.cleanup)
        self.stopped = True
        self.refuse_compare = False
        self.refuse_post_compare = False
        self.legacy_compare = False
        self.policy = {'owner': 'ab' * 32, 'epoch': str(uuid.uuid4()), 'provider': {'account_id': str(uuid.uuid4())}, 'components': {}}
        self.identities = {role: {'uid': 50001 + i, 'gid': 50001 + i}
                           for i, role in enumerate(K.U.P.ROLE_NAMES)}
        runtime = self.root / 'runtime'
        runtime.mkdir(mode=0o700)
        for role, identity in self.identities.items():
            state = runtime / role
            state.mkdir(mode=0o700)
            os.chown(state, identity['uid'], identity['gid'])
            name = 'cyberherd.db' if role == 'membership' else role + '.db'
            path = state / name
            self.write(path, ('retained-original-' + role).encode())
            os.chown(path, identity['uid'], identity['gid'])
            self.policy['components'][role] = dict(identity, database=str(path))
        self.policy['max_component_bytes'] = 1024 * 1024
        self.binary = self.root / 'bin'
        self.binary.mkdir(mode=0o755)
        self.replace(K.Q, 'BIN', self.binary)
        self.write(self.binary / 'cyberherdd', b'synthetic worker; never executed', 0o755)
        self.write(self.binary / 'cyberherd-statectl', b'synthetic verifier; never executed', 0o755)
        payload = self.directory / 'payload'
        payload.mkdir(mode=0o700)
        self.write(payload / 'cyberherd-statectl', b'synthetic verifier; never executed', 0o755)
        self.config = self.root / 'member.toml'
        self.write(self.config, ('state_dir = "' + str(runtime / 'membership') + '"\nenabled = true\n').encode())
        self.replace(K.H, 'configurations', lambda: {'membership': self.config})
        self.r = {'baseline': {'policy': self.policy, 'identities': self.identities,
            'files': {str(self.binary / name): K.TX.fingerprint(self.binary / name) for name in ('cyberherdd', 'cyberherd-statectl')}}}
        self.m = {'review': str(self.directory)}
        self.replace(K.H, 'load', lambda directory, expected: ({'configurations': []}, self.m, self.r))
        self.replace(K.U, 'sources_match', self.sources_match)
        self.replace(K.U, 'state_files', lambda r: self.images())
        self.replace(K.U, 'stopped', lambda *a, **k: self.assertTrue(self.stopped))
        self.replace(K.U, 'stable', lambda *a, **k: None)
        self.replace(K.H, 'restore_units', lambda *a, **k: self.assertTrue(k['original']))
        self.replace(K.M, 'running', lambda *a: None)
        self.replace(K.M, 'health', lambda *a: None)
        self.replace(K.C, 'status', lambda name: {'LoadState': 'masked', 'UnitFileState': 'masked', 'MainPID': '0'})
        self.backup = self.directory / 'backup'
        self.snapshot(self.backup)
        self.checked = {'native_backup': self.ack('verify', self.backup), 'live_images': self.images(),
            'backup_manifest_sha256': K.U.digest(self.backup / 'MANIFEST.json')}
        K.C.write_json(self.directory / 'CHECKPOINT.json', self.checked)
        report = self.directory / 'reward-review'
        report.mkdir(mode=0o700)
        K.C.write_json(report / 'REVIEW.json', {'synthetic': True})
        self.checkpoint_sha = K.U.digest(self.directory / 'CHECKPOINT.json')
        self.report_sha = K.U.digest(report / 'REVIEW.json')
        self.replace(K.H, 'checked_checkpoint', self.checked_checkpoint)
        K.C.write_json(self.directory / 'PREPARED.json', {'first_financial_transaction': True,
            'baseline': self.r['baseline'], 'activation_authorized': False, 'services_changed': False, 'journals_opened': False})
        self.common = (self.directory, K.U.digest(self.directory / 'PREPARED.json'), self.checkpoint_sha, self.report_sha)
        self.original = self.images()

    def cleanup(self):
        for carrier in self.carriers:
            if carrier.exists():
                shutil.rmtree(carrier)

    def images(self):
        return {role: K.TX.fingerprint(Path(c['database'])) for role, c in self.policy['components'].items()}

    def sources_match(self, m, r, observed, **kw):
        self.assertTrue(self.stopped)
        if self.images() != observed:
            raise ValueError('live images changed')

    def checked_checkpoint(self, directory, checkpoint, report, m, r, installed):
        if checkpoint != K.U.digest(directory / 'CHECKPOINT.json') or report != K.U.digest(directory / 'reward-review/REVIEW.json'):
            raise ValueError('checkpoint changed')
        self.sources_match(m, r, self.checked['live_images'])
        return self.checked, {}

    def snapshot(self, target):
        target.mkdir(mode=0o700)
        for role, c in self.policy['components'].items():
            self.write(target / (role + '.db'), Path(c['database']).read_bytes())
        K.C.write_json(target / 'MANIFEST.json', {'id': str(uuid.uuid4())})

    def ack(self, operation, bundle):
        return {'operation': operation, 'id': json.loads((bundle / 'MANIFEST.json').read_text())['id'],
                'accepted': True, 'resume_authorized': False}

    def command(self, *args, **kw):
        if args[0] == '/usr/bin/unshare':
            self.assertIn('--clear-groups', args)
            self.assertIn('--bounding-set=-all', args)
            self.assertIn('--no-new-privs', args)
            self.assertIn('--reuid=' + str(self.identities['membership']['uid']), args)
            config = Path(args[args.index('--config') + 1])
            preview = Path(args[args.index('--record-initial-reward-cutover') + 1])
            self.carriers.add(config.parent)
            value = json.loads(preview.read_text())
            self.assertEqual(K.U.digest(preview), args[args.index('--reward-cutover-sha256') + 1])
            state = Path(K.H.tomllib.loads(config.read_text())['state_dir'])
            if state.parent != Path(self.policy['components']['membership']['database']).parent.parent:
                self.carriers.add(state.parent)
            db = state / 'cyberherd.db'
            db.write_bytes(db.read_bytes() + b'|canonical-cutoff-audit')
            self.member_calls.append(db)
            return json.dumps(K.receipt(value))
        if args[:2] == ('/usr/bin/systemctl', 'start'):
            self.starts.extend(args[3:])
            return ''
        operation = args[3]
        bundle = Path(args[args.index('--bundle') + 1]) if '--bundle' in args else None
        if operation == 'preview-initial-reward-cutover':
            target = Path(args[args.index('--destination') + 1])
            target.mkdir(mode=0o700)
            decision = args[args.index('--decision-id') + 1]
            self.preview = {'version': 1, 'id': decision, 'owner': self.policy['owner'],
                'bundle_id': self.checked['native_backup']['id'], 'source_policy_sha256': 'bc' * 32,
                'decision': {'id': decision, 'epoch': self.policy['epoch'], 'through': 2,
                    'backup_manifest_sha256': self.checked['backup_manifest_sha256'], 'decided_at': 100,
                    'historical': [{'request_id': str(uuid.uuid4()), 'core_seq': 2, 'proof_hash': 'cd' * 32}]},
                'cutover_applied': False, 'activation_authorized': False}
            K.C.write_json(target / 'PREVIEW.json', self.preview)
            return json.dumps({'operation': 'preview_initial_reward_cutover', 'id': decision,
                              'accepted': True, 'cutover_applied': False, 'activation_authorized': False})
        if operation == 'compare-initial-reward-cutover':
            if self.refuse_compare or (self.refuse_post_compare and len(self.member_calls) == 2):
                raise ValueError('synthetic native comparison refused')
            candidate = Path(args[args.index('--candidate-copy') + 1])
            for role in self.policy['components']:
                original = (bundle / (role + '.db')).read_bytes()
                expected = original + b'|canonical-cutoff-audit' if role == 'membership' else original
                self.assertEqual((candidate / (role + '.db')).read_bytes(), expected)
            result = self.ack('initial_reward_cutover_compare', bundle)
            if not self.legacy_compare:
                result['decision_sha256'] = K.decision_hash(self.preview['decision'])
            return json.dumps(result)
        if operation == 'snapshot':
            target = Path(args[args.index('--destination') + 1])
            self.snapshot(target)
            return json.dumps(self.ack('snapshot', target))
        if operation == 'verify':
            return json.dumps(self.ack('verify', bundle))
        if operation == 'review-rewards':
            target = Path(args[args.index('--destination') + 1])
            target.mkdir(mode=0o700)
            summary = {'decision_id': self.preview['id'], 'epoch': self.policy['epoch'], 'through': 2,
                'backup_manifest_sha256': self.checked['backup_manifest_sha256'], 'historical_feedings': 1, 'decision_sha256': K.decision_hash(self.preview['decision'])}
            ident = str(uuid.uuid4())
            K.C.write_json(target / 'REVIEW.json', {'id': ident, 'bundle_id': self.ack('verify', bundle)['id'],
                'bundle_manifest_sha256': K.U.digest(bundle / 'MANIFEST.json'), 'initial_reward_cutover': summary,
                'owner': self.policy['owner'], 'epoch': self.policy['epoch'], 'all_barriers_examined': True,
                'activation_authorized': False, 'external_reconciliation_required': True})
            return json.dumps({'operation': 'review_rewards', 'id': ident, 'accepted': True, 'activation_authorized': False})
        raise AssertionError(args)

    def prepare(self):
        self.assertTrue(K.prepare(*self.common, str(uuid.uuid4()))['rehearsed'])
        self.cutoff = self.directory / 'reward-cutover'
        self.cutoff_sha = K.U.digest(self.cutoff / 'PREPARED.json')

    def test_prepare_apply_resume_preserves_other_journals_and_starts_only_original_roles(self):
        self.prepare()
        self.assertEqual(self.images(), self.original)
        self.assertEqual(self.starts, [])
        self.assertTrue(K.apply(*self.common, self.cutoff_sha, True)['retained_rows_unchanged'])
        current = self.images()
        for role in ('core', 'payout', 'signer'):
            self.assertEqual(current[role], self.original[role])
        applied_sha = K.U.digest(self.cutoff / 'APPLIED.json')
        result = K.resume(*self.common, self.cutoff_sha, applied_sha, True)
        self.assertFalse(result['outgoing_roles_enabled'])
        self.assertEqual(self.starts, [K.U.P.ROLE_NAMES[r] + '.service' for r in ('core', 'membership')])
        self.assertEqual(len(self.member_calls), 2)
        with self.assertRaises(ValueError):
            K.resume(*self.common, self.cutoff_sha, applied_sha, True)

    def test_changed_preparation_or_live_images_refuses_before_live_child(self):
        self.prepare()
        with self.assertRaises(ValueError):
            K.apply(*self.common, '00' * 32, True)
        db = Path(self.policy['components']['core']['database'])
        db.write_bytes(db.read_bytes() + b'changed')
        with self.assertRaises(ValueError):
            K.apply(*self.common, self.cutoff_sha, True)
        self.assertEqual(len(self.member_calls), 1)
        self.assertEqual(self.starts, [])

    def test_native_rehearsal_refusal_retains_live_history_and_incomplete_receipt(self):
        self.refuse_compare = True
        with self.assertRaises(ValueError):
            K.prepare(*self.common, str(uuid.uuid4()))
        self.assertEqual(self.images(), self.original)
        self.assertTrue((self.directory / 'reward-cutover/INCOMPLETE').exists())
        self.assertEqual(self.starts, [])

    def test_post_apply_failure_keeps_writers_stopped_and_refuses_automatic_replay(self):
        self.prepare()
        self.refuse_post_compare = True
        with self.assertRaises(ValueError):
            K.apply(*self.common, self.cutoff_sha, True)
        self.assertTrue((self.cutoff / 'APPLICATION-STARTED.json').exists())
        self.assertFalse((self.cutoff / 'APPLIED.json').exists())
        self.assertTrue(self.stopped)
        self.assertEqual(self.starts, [])
        self.assertEqual(len(self.member_calls), 2)
        with self.assertRaises(ValueError):
            K.apply(*self.common, self.cutoff_sha, True)
        self.assertEqual(len(self.member_calls), 2)

    def test_resume_requires_reviewed_completed_application_before_starting_services(self):
        self.prepare()
        with self.assertRaises((ValueError, FileNotFoundError)):
            K.resume(*self.common, self.cutoff_sha, '00' * 32, True)
        self.assertEqual(self.starts, [])
        K.apply(*self.common, self.cutoff_sha, True)
        with self.assertRaises(ValueError):
            K.resume(*self.common, self.cutoff_sha, '00' * 32, True)
        self.assertEqual(self.starts, [])

    def test_completed_review_recomputes_native_scope_and_refuses_drift_before_binding_finance(self):
        self.prepare()
        K.apply(*self.common, self.cutoff_sha, True)
        applied_sha = K.U.digest(self.cutoff / 'APPLIED.json')
        K.resume(*self.common, self.cutoff_sha, applied_sha, True)
        binding = K.H.completed_cutover_review(self.cutoff, self.cutoff_sha, applied_sha, self.r['baseline'])
        self.assertEqual(binding['summary']['decision_id'], self.preview['id'])
        self.assertEqual(binding['preparation_sha256'], self.cutoff_sha)
        for prepared, applied in [('00' * 32, applied_sha), (self.cutoff_sha, '00' * 32)]:
            with self.assertRaises(ValueError):
                K.H.completed_cutover_review(self.cutoff, prepared, applied, self.r['baseline'])
        self.preview['id'] = str(uuid.uuid4())
        # Saved proofs and report hashes are unchanged; the independently fresh
        # native report must still agree with the approved decision.
        with self.assertRaises(ValueError):
            K.H.completed_cutover_review(self.cutoff, self.cutoff_sha, applied_sha, self.r['baseline'])

    def test_older_native_acknowledgment_refuses_during_clone_preparation_before_live_audit(self):
        self.legacy_compare = True
        with self.assertRaises(ValueError):
            K.prepare(*self.common, str(uuid.uuid4()))
        self.assertEqual(self.images(), self.original)
        self.assertEqual(len(self.member_calls), 1)
        self.assertEqual(self.starts, [])
        self.assertFalse((self.directory / 'reward-cutover/PREPARED.json').exists())
