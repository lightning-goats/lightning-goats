"""First financial transaction boundaries; actual files, mocked services/native IO.

The separate disposable installed rehearsal exercises real native parsers/stores
and systemd. No test here is production qualification or payment authority.
"""
import copy
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import types
import unittest
from unittest.mock import patch

import test_upgrade_financial_stage as candidate_support
import test_upgrade_program_maintenance as program_support

SCRIPTS = Path(__file__).resolve().parents[1] / 'scripts'
with patch.dict(sys.modules, {'inactive_file_transaction': program_support.P.TX}):
    spec = importlib.util.spec_from_file_location('financial_activation_fixture', SCRIPTS / 'activate-cyberherd-financial.py')
    H = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(H)
H.F = candidate_support.F
H.U = program_support.P.U
H.C, H.Q, H.S, H.TX, H.M = H.U.C, H.U.Q, H.U.S, H.U.TX, H.U.M


def backlog():
    return {'version': 1, 'all_barriers_examined': True, 'activation_authorized': False,
        'external_reconciliation_required': True, 'stored_member_percent': 10, 'stored_max_members': 3,
        'received_through': 10, 'core_event_head': 11, 'pending_ingress': 3, 'pending_feeds': 1,
        'unreceived_payment_events': 0, 'unreceived_feeding_events': 0, 'pending_feeding_ingress': 0,
        'unfinalized_core_reservations': 0, 'unresolved_provider_payments': 0, 'payout_held_sats': 0,
        'stages': {'unplanned_zero_allocation': 2, 'unplanned_no_strike_funding': 1, 'no_payout': 1}}


class ActivationBoundaries(unittest.TestCase):
    def test_explicit_approvals_are_required_before_loading_any_review(self):
        with patch.object(H, 'load') as load:
            for operation in (H.checkpoint,):
                with self.assertRaisesRegex(ValueError, 'explicit'):
                    operation(Path('/missing'), '0' * 64, False)
            for operation in (H.apply, H.rollback):
                with self.assertRaisesRegex(ValueError, 'explicit'):
                    operation(Path('/missing'), *(['0' * 64] * 3), False)
            for approved, reviewed in [(False, False), (False, True), (True, False)]:
                with self.assertRaisesRegex(ValueError, 'explicit'):
                    H.resume(Path('/missing'), *(['0' * 64] * 3), approved, reviewed)
            load.assert_not_called()

    def test_financial_tail_gate_accepts_information_lag_and_retains_deferred_social_and_physical_work(self):
        report = backlog()
        H.first_activation_backlog(report, candidate_support.LIMITS)
        self.assertEqual(report['pending_ingress'], 3)
        self.assertEqual(report['pending_feeds'], 1)
        self.assertLess(report['received_through'], report['core_event_head'])
        for name in ['unreceived_payment_events', 'unreceived_feeding_events', 'pending_feeding_ingress',
                     'unfinalized_core_reservations', 'unresolved_provider_payments', 'payout_held_sats']:
            for value in [1, -1, False, '0', None]:
                with self.subTest(name=name, value=value), self.assertRaises(ValueError):
                    H.first_activation_backlog(dict(report, **{name: value}), candidate_support.LIMITS)
            missing = dict(report); missing.pop(name)
            with self.assertRaises(ValueError):
                H.first_activation_backlog(missing, candidate_support.LIMITS)

    def test_frozen_funded_plans_unknown_stages_and_stored_allocation_drift_never_authorize_first_activation(self):
        for stage in ['unplanned_funded_allocation', 'awaiting_reservation', 'reserved', 'unknown']:
            report = backlog(); report['stages'][stage] = 1
            with self.assertRaises(ValueError):
                H.first_activation_backlog(report, candidate_support.LIMITS)
        for stages in [None, [], {'no_payout': -1}, {'no_payout': True}]:
            with self.assertRaises(ValueError):
                H.first_activation_backlog(dict(backlog(), stages=stages), candidate_support.LIMITS)
        for value in [0, 11, True, '10']:
            with self.assertRaises(ValueError):
                H.first_activation_backlog(dict(backlog(), stored_member_percent=value), candidate_support.LIMITS)
        for value in [0, 10, True, '3', None]:
            with self.assertRaises(ValueError):
                H.first_activation_backlog(dict(backlog(), stored_max_members=value), candidate_support.LIMITS)

    def test_completed_schema_review_reverifies_retained_native_images_and_refuses_drift(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            records = {}
            for folder in ('backup', 'migrated'):
                (root / folder).mkdir()
                (root / folder / 'MANIFEST.json').write_text(folder)
            def ack(operation):
                return {'operation': operation, 'id': '11111111-1111-4111-8111-111111111111',
                        'accepted': True, 'resume_authorized': False}
            source, verified, compared = [ack(name) for name in
                ('inclusive_fee_upgrade_source_verify', 'verify', 'inclusive_fee_upgrade_compare')]
            applied = {'inclusive_fee_upgrade': True, 'retained_rows_unchanged': True,
                'activation_authorized': False, 'services_started': False, 'configurations_changed': False,
                'backup_manifest_sha256': H.U.digest(root / 'backup/MANIFEST.json'),
                'migrated_manifest_sha256': H.U.digest(root / 'migrated/MANIFEST.json'),
                'verified': verified, 'compared': compared}
            backup = {'inclusive_fee_upgrade': True, 'selected_source': source,
                      'manifest_sha256': applied['backup_manifest_sha256']}
            records.update({'APPLIED.json': applied, 'BACKUP-ACCEPTED.json': backup})
            operations = {'verify-inclusive-fee-upgrade-source': source, 'verify': verified,
                          'compare-inclusive-fee-upgrade': compared}
            def command(*args, **kwargs):
                self.assertEqual(args[:3], (str(H.Q.BIN / 'cyberherd-statectl'), '--config', str(H.S.MAINTENANCE)))
                self.assertNotIn('snapshot', args)
                return json.dumps(operations[args[3]])
            with patch.object(H.C, 'protected'), patch.object(H.C, 'read_json', side_effect=lambda p: records[p.name]), \
                    patch.object(H.C, 'command', side_effect=command) as native:
                H.completed_inclusive_review(root)
                self.assertEqual(native.call_count, 3)
                self.assertIn('--candidate-copy', native.call_args.args)
                for field in ('inclusive_fee_upgrade', 'retained_rows_unchanged', 'activation_authorized', 'services_started'):
                    old = applied[field]; applied[field] = not old
                    with self.subTest(field=field), self.assertRaises(ValueError):
                        H.completed_inclusive_review(root)
                    applied[field] = old
                for folder in ('backup', 'migrated'):
                    path = root / folder / 'MANIFEST.json'
                    original = path.read_bytes(); path.write_bytes(b'changed')
                    native.reset_mock()
                    with self.assertRaisesRegex(ValueError, 'images changed'):
                        H.completed_inclusive_review(root)
                    native.assert_not_called(); path.write_bytes(original)
                operations['compare-inclusive-fee-upgrade'] = dict(compared, accepted=False)
                with self.assertRaisesRegex(ValueError, 'native comparison changed'):
                    H.completed_inclusive_review(root)
                operations['compare-inclusive-fee-upgrade'] = compared
                backup['selected_source'] = dict(source, id='22222222-2222-4222-8222-222222222222')
                with self.assertRaisesRegex(ValueError, 'native comparison changed'):
                    H.completed_inclusive_review(root)

    def test_direct_execution_refuses_before_substituted_local_code_import(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            shutil.copyfile(SCRIPTS / 'activate-cyberherd-financial.py', root / 'activate.py')
            (root / 'prepare-cyberherd-financial-stage.py').write_text('raise RuntimeError("INJECTED")')
            result = subprocess.run(['/usr/bin/python3', '-I', '-S', '-B', str(root / 'activate.py'), '--help'],
                                    capture_output=True, text=True, timeout=10)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn('externally verified root-owned tool launcher', result.stderr)
            self.assertNotIn('INJECTED', result.stderr)


@unittest.skipUnless(os.geteuid() == 0, 'protected financial activation files run in separate root CI')
class ActivationFiles(unittest.TestCase):
    write = staticmethod(program_support.ProgramFiles.write)
    cleanup_carriers = program_support.ProgramFiles.cleanup_carriers
    migrate = program_support.ProgramFiles.migrate
    extract = program_support.ProgramFiles.extract

    def replace(self, obj, name, value):
        context = patch.object(obj, name, value)
        context.start()
        self.addCleanup(context.stop)

    def setUp(self):
        # Its initial program-only fixture remains distinct; our synthetic
        # completed program receipt below is DATA, not native qualification.
        self.financial_ready = False
        program_support.ProgramFiles.setUp(self)
        self.replace(H, 'verified_tool_identity', lambda: H.U.verified_tool_identity())
        for name, value in [('U', H.U), ('C', H.C), ('Q', H.Q), ('S', H.S), ('TX', H.TX)]:
            self.replace(H.F, name, value)
        self.replace(H.F, 'verified_tool_identity', lambda: H.verified_tool_identity())
        self.replace(H.F, 'observed_allocation', lambda owner, percent:
                     {'owner': owner, 'member_percent': percent, 'capacity': 3, 'generated_at': 100})
        self.replace(H.C.grp, 'getgrnam', lambda name: types.SimpleNamespace(gr_gid=10006))
        original = candidate_support.fixture()
        original['core']['database']['url'] = 'sqlite://' + str(self.core_db)
        original['core']['core_bridge']['peer_uid'] = self.identity['membership']['uid']
        original['membership']['core']['core_uid'] = self.identity['core']['uid']
        original['payout']['intake_uid'] = self.identity['membership']['uid']
        original['payout']['signer_uid'] = self.identity['signer']['uid']
        original['payout']['policy']['scope']['core_uid'] = self.identity['core']['uid']
        original['signer']['policy']['peer_uid'] = self.identity['payout']['uid']
        self.policy.update(owner=original['membership']['project_pubkey'],
            epoch=original['membership']['core']['epoch'], provider=original['payout']['policy']['scope']['provider'])
        self.write(self.policy_path, H.S.W.toml_text(self.policy).encode(), 0o600)
        for role, target in H.configurations().items():
            self.write(target, H.S.W.toml_text(original[role]).encode(), 0o600)
        self.original_files = {name: H.TX.fingerprint(name) for name in self.original_files}
        self.baseline.clear()
        self.baseline.update(files=self.original_files, identities=self.identity, policy=self.policy,
                             states={role: self.status(name + '.service') for role, name in H.U.P.ROLE_NAMES.items()})
        self.program_review = self.base / 'qualified-program-data'
        self.program_review.mkdir(mode=0o700)
        H.C.write_json(self.program_review / 'PREPARED.json', {'version': 1, 'program_upgrade_only': True,
            'source_commit': 'a' * 40, 'baseline': self.baseline,
            'selected_program_sha256': {name: H.U.digest(self.binary / name) for name in H.PROGRAMS},
            **{k: False for k in ['activation_authorized', 'schema_upgrade_authorized', 'services_changed',
                                 'journals_opened', 'credentials_loaded']}})
        H.C.write_json(self.program_review / 'RESUMED.json', {'core_running': True, 'membership_running': True,
            'original_programs': False, 'payouts_enabled': False, 'configurations_changed': False})
        self.program_sha = H.U.digest(self.program_review / 'PREPARED.json')
        self.financial_ready = True
        self.review_value = backlog()
        self.validator = self.root / 'financial-checker'
        self.write(self.validator, b'synthetic native checker data; never executed', 0o755)
        self.candidate = self.base / 'financial-candidate'
        H.F.prepare(self.candidate, self.validator, H.U.digest(self.validator), candidate_support.LIMITS)
        self.candidate_sha = H.U.digest(self.candidate / 'PREPARED.json')
        self.financial = self.base / 'financial-transaction'
        H.plan(self.candidate, self.candidate_sha, self.program_review, self.program_sha, self.financial)
        self.financial_sha = H.U.digest(self.financial / 'PREPARED.json')
        self.starts.clear()

    def command(self, *arguments, **kwargs):
        if self.financial_ready and arguments[:2] == ('/usr/bin/systemctl', 'enable'):
            self.commands.append(arguments)
            parent = self.etc / 'multi-user.target.wants'
            parent.mkdir(mode=0o755, exist_ok=True)
            for name in arguments[3:]:
                (parent / name).symlink_to(self.vendor / name)
            return ''
        if self.financial_ready and arguments[0] == '/usr/bin/unshare' and arguments[-1] != '--check':
            self.commands.append(arguments)
            self.assertIn('--reuid=' + str(self.identity['core']['uid']), arguments)
            self.assertIn('--clear-groups', arguments); self.assertIn('--bounding-set=-all', arguments)
            for name in arguments[-4:]:
                path = Path(name)
                self.carriers.add(path.parent)
                self.assertEqual(path.stat().st_mode & 0o777, 0o440)
            return json.dumps(candidate_support.REPORT)
        value = program_support.ProgramFiles.command(self, *arguments, **kwargs)
        if self.financial_ready and len(arguments) > 3 and arguments[3] == 'review-rewards':
            destination = Path(arguments[arguments.index('--destination') + 1]) / 'REVIEW.json'
            report = json.loads(destination.read_text())
            report.update(copy.deepcopy(self.review_value))
            destination.write_text(json.dumps(report))
        return value

    def status(self, unit):
        value = program_support.ProgramFiles.status(self, unit)
        if value['LoadState'] != 'masked' and (self.etc / 'multi-user.target.wants' / unit).is_symlink():
            value['UnitFileState'] = 'enabled'
        return value

    def checkpoint(self):
        value = H.checkpoint(self.financial, self.financial_sha, True)
        self.assertTrue(value['private_reward_review_ready'])
        self.checkpoint_sha = H.U.digest(self.financial / 'CHECKPOINT.json')
        self.reward_sha = H.U.digest(self.financial / 'reward-review/REVIEW.json')

    def args(self):
        return self.financial, self.financial_sha, self.checkpoint_sha, self.reward_sha

    def preserved(self):
        for role, component in self.components.items():
            self.assertEqual(Path(component['database']).read_bytes(), self.old_bytes[role])
        for name in H.PROGRAMS:
            self.assertEqual((self.binary / name).read_bytes(), ('old program: ' + name).encode())

    def test_checkpoint_apply_and_explicit_four_role_start_preserve_programs_paid_images_and_originals(self):
        self.assertFalse((self.financial / 'CHECKPOINT.json').exists())
        self.assertEqual(self.starts, [])
        self.checkpoint()
        for name, fp in self.original_files.items():
            if name != str(self.unit):
                self.assertEqual(H.TX.fingerprint(name), fp)
        result = H.apply(*self.args(), True)
        self.assertTrue(result['financial_images_unchanged'])
        self.assertEqual(self.starts, [])
        self.preserved()
        for role, target in H.configurations().items():
            self.assertEqual(target.read_bytes(), (self.financial / ('candidate-' + role + '.toml')).read_bytes())
            old = self.original_files[str(target)]
            self.assertEqual({k: v for k, v in H.TX.fingerprint(target).items() if k != 'sha256'},
                             {k: v for k, v in old.items() if k != 'sha256'})
        resumed = H.resume(*self.args(), True, True)
        self.assertTrue(resumed['outgoing_roles_started'])
        self.assertTrue(resumed['outgoing_roles_enabled'])
        self.assertFalse(resumed['provider_execution_qualified'])
        self.assertFalse(resumed['external_recovery_adopted'])
        self.assertEqual(self.starts, [H.C.UNIT_NAME, 'cyberherd-signer.service', 'cyberherd.service', 'cyberherd-payout.service'])
        self.preserved()
        with self.assertRaisesRegex(ValueError, 'already began'):
            H.resume(*self.args(), True, True)
        with self.assertRaisesRegex(ValueError, 'already began'):
            H.rollback(*self.args(), True)

    def test_checkpoint_with_existing_funded_work_never_applies_it_and_can_resume_original_zero_stage(self):
        self.review_value['stages']['unplanned_funded_allocation'] = 1
        self.checkpoint()
        with self.assertRaisesRegex(ValueError, 'existing funded reward work'):
            H.apply(*self.args(), True)
        self.assertFalse((self.financial / 'APPLICATION-STARTED.json').exists())
        H.rollback(*self.args(), True)
        result = H.resume(*self.args(), True, False, original=True)
        self.assertFalse(result['outgoing_roles_started'])
        self.assertEqual(self.starts, [H.C.UNIT_NAME, 'cyberherd.service'])
        self.preserved()
        for name, fp in self.original_files.items():
            self.assertEqual(H.TX.fingerprint(name), fp)

    def test_partial_configuration_replacement_can_restore_only_configuration_before_start(self):
        self.checkpoint()
        execute = H.TX.execute
        def interrupted(directory, guard, **kwargs):
            def after(index):
                if index == 1: raise ValueError('synthetic configuration interruption')
            return execute(directory, guard, after_replace=after, **kwargs)
        with patch.object(H.TX, 'execute', interrupted):
            with self.assertRaisesRegex(ValueError, 'configuration interruption'):
                H.apply(*self.args(), True)
        self.assertFalse((self.financial / 'APPLIED.json').exists())
        self.assertEqual(self.starts, [])
        H.rollback(*self.args(), True)
        with self.assertRaisesRegex(ValueError, 'rollback was selected'):
            H.resume(*self.args(), True, True)
        H.resume(*self.args(), True, False, original=True)
        self.preserved()

    def test_changed_paid_images_are_never_rewritten_or_resumed(self):
        self.checkpoint()
        self.core_db.write_bytes(b'new money evidence must survive')
        for operation in [H.apply, H.rollback]:
            with self.assertRaisesRegex(ValueError, 'financial images changed'):
                operation(*self.args(), True)
        self.assertEqual(self.core_db.read_bytes(), b'new money evidence must survive')
        self.assertEqual(self.starts, [])

    def test_changed_review_and_old_native_tail_fields_are_refused_before_financial_install(self):
        self.review_value.pop('unreceived_feeding_events')
        self.checkpoint()
        with self.assertRaisesRegex(ValueError, 'classified financial intake'):
            H.apply(*self.args(), True)
        with self.assertRaisesRegex(ValueError, 'checkpoint and reward report'):
            H.apply(self.financial, self.financial_sha, '0' * 64, self.reward_sha, True)
        self.assertEqual(self.starts, [])
        self.preserved()

    def test_program_review_must_be_completed_selected_programs_and_exact_current_binary_hashes(self):
        other = self.base / 'refused-plan'
        resumed = self.program_review / 'RESUMED.json'
        value = json.loads(resumed.read_text()); value['original_programs'] = True
        resumed.write_text(json.dumps(value))
        with self.assertRaisesRegex(ValueError, 'completed program upgrade'):
            H.plan(self.candidate, self.candidate_sha, self.program_review, self.program_sha, other)
        self.assertFalse(other.exists())
        value['original_programs'] = False; resumed.write_text(json.dumps(value))
        changed = copy.deepcopy(self.baseline)
        changed['files'][str(self.binary / 'cyberherd-payoutd')]['sha256'] = '0' * 64
        with self.assertRaisesRegex(ValueError, 'separately qualified program review'):
            H.program_review(self.program_review, self.program_sha, changed)

    def test_unacknowledged_activation_leaves_all_writers_masked(self):
        self.checkpoint()
        H.apply(*self.args(), True)
        with self.assertRaisesRegex(ValueError, 'provider/recipient review'):
            H.resume(*self.args(), True, False)
        self.assertFalse((self.financial / 'RESUME-REQUESTED.json').exists())
        self.assertEqual(self.starts, [])
        self.preserved()

    def test_paid_image_change_after_configuration_install_blocks_every_start(self):
        self.checkpoint()
        H.apply(*self.args(), True)
        payout = Path(self.components['payout']['database'])
        payout.write_bytes(b'new original payment evidence')
        with self.assertRaisesRegex(ValueError, 'financial images changed'):
            H.resume(*self.args(), True, True)
        self.assertFalse((self.financial / 'RESUME-REQUESTED.json').exists())
        self.assertEqual(payout.read_bytes(), b'new original payment evidence')
        self.assertEqual(self.starts, [])

    def test_installed_parser_refusal_precedes_service_stop_or_snapshot(self):
        self.worker_refusal = True
        with self.assertRaisesRegex(ValueError, 'native worker did not accept'):
            H.checkpoint(self.financial, self.financial_sha, True)
        self.assertFalse((self.financial / 'MAINTENANCE-STARTED.json').exists())
        self.assertFalse(self.unit.is_symlink())
        self.assertEqual(self.starts, [])
        self.preserved()
