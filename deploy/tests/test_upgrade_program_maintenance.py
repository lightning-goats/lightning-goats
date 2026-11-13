"""Six-program orchestration; root cases use real protected disposable files.

Native parsers, SQLite and systemd are mocked here. The separate installed CI
rehearsal executes their actual counterparts on synthetic state.
"""
import hashlib
import importlib.util
import io
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tarfile
import tempfile
import unittest
from unittest.mock import patch
import uuid

import test_upgrade_contact_maintenance as support

SCRIPTS = Path(__file__).resolve().parents[1] / 'scripts'
with patch.dict(sys.modules, {'inactive_file_transaction': support.U.TX}):
    spec = importlib.util.spec_from_file_location('six_program_maintenance', SCRIPTS / 'maintain-cyberherd-programs.py')
    P = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(P)
# Share the established disposable-host fixture, not production globals. The
# implementation imports this same helper closure through its verified launcher.
P.U = support.U
P.C, P.Q, P.S, P.M, P.TX = P.U.C, P.U.Q, P.U.S, P.U.M, P.U.TX


class ProgramBoundaries(unittest.TestCase):
    def test_direct_execution_refuses_before_importing_local_code(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            shutil.copyfile(SCRIPTS / 'maintain-cyberherd-programs.py', root / 'programs.py')
            (root / 'maintain-contact-upgrade.py').write_text('raise RuntimeError("INJECTED")')
            result = subprocess.run(['/usr/bin/python3', '-I', '-S', '-B', str(root / 'programs.py'), '--help'],
                                    capture_output=True, text=True, timeout=10)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn('verified root-owned tool launcher', result.stderr)
            self.assertNotIn('INJECTED', result.stderr)

    def test_every_mutation_requires_its_explicit_approval_before_loading_files(self):
        for operation in (P.apply, P.rollback, P.resume):
            with self.subTest(operation=operation.__name__), patch.object(P, 'load') as load:
                with self.assertRaisesRegex(ValueError, 'explicit'):
                    operation(Path('/missing'), '0' * 64, False)
                load.assert_not_called()

    def test_fee_migration_child_uses_own_uid_no_capabilities_and_no_network(self):
        message='Existing payout journal migrated; no runtime, credential or transport started'
        with patch.object(P.C,'root'), patch.object(P.C,'protected'), patch.object(P.C,'command',return_value=message) as command:
            result=P.offline_fee_migration_child(Path('/protected/payout'),Path('/protected/config'),{'uid':10003,'gid':10004})
            self.assertFalse(result['worker_started'])
            self.assertFalse(result['credential_loaded'])
            args=command.call_args.args
            self.assertEqual(args[:4],('/usr/bin/unshare','--net','--','/usr/bin/setpriv'))
            for flag in ('--reuid=10003','--regid=10004','--clear-groups','--inh-caps=-all','--ambient-caps=-all','--bounding-set=-all','--no-new-privs'):
                self.assertIn(flag,args)
            self.assertEqual(args[-5:],('/protected/payout','--config','/protected/config','--migrate-inclusive-fees','--writers-stopped'))
        for component in ({'uid':0,'gid':1},{'uid':1,'gid':0},{'uid':True,'gid':1},{'uid':'1','gid':1}):
            with patch.object(P.C,'root'), patch.object(P.C,'protected') as protected, patch.object(P.C,'command') as command:
                with self.assertRaises(ValueError):
                    P.offline_fee_migration_child(Path('/missing'),Path('/missing'),component)
                protected.assert_not_called();command.assert_not_called()
        with patch.object(P.C,'root'), patch.object(P.C,'protected'), patch.object(P.C,'command',return_value='not confirmed'):
            with self.assertRaisesRegex(ValueError,'preserve stopped state'):
                P.offline_fee_migration_child(Path('/protected/payout'),Path('/protected/config'),{'uid':1,'gid':2})

    def test_inclusive_native_receipt_binds_operation_identity_and_no_resumption(self):
        good={'operation':'inclusive_fee_upgrade_source_verify','id':str(uuid.uuid4()),'accepted':True,'resume_authorized':False}
        values=[good,dict(good,operation='verify'),dict(good,accepted=1),dict(good,resume_authorized=0),dict(good,id=str(uuid.UUID(int=0))),dict(good,extra=True)]
        for value in values:
            with patch.object(P.C,'protected'),patch.object(P.C,'command',return_value=json.dumps(value)):
                if value==good and type(value['accepted']) is bool and type(value['resume_authorized']) is bool:
                    self.assertEqual(P.inclusive_native(Path('/protected'),'verify-inclusive-fee-upgrade-source'),good)
                else:
                    with self.assertRaises(ValueError):
                        P.inclusive_native(Path('/protected'),'verify-inclusive-fee-upgrade-source')
        with patch.object(P.U,'stopped') as stopped:
            with self.assertRaisesRegex(ValueError,'distinct inclusive-fee'):
                P.inclusive_backup(Path('/missing'),{}, {'program_upgrade_only':True})
            stopped.assert_not_called()

    def test_native_original_snapshot_receipt_rejects_aliases_and_activation(self):
        good = {'operation': 'snapshot', 'id': str(uuid.uuid4()), 'accepted': True, 'resume_authorized': False}
        for value in [good, dict(good, accepted=1), dict(good, resume_authorized=0),
                      dict(good, operation='verify'), dict(good, extra=True), dict(good, id='bad')]:
            with patch.object(P.C, 'protected'), patch.object(P.C, 'command', return_value=json.dumps(value)):
                if value == good and all(type(value[k]) is type(v) for k, v in good.items()):
                    self.assertEqual(P.original_verify(Path('/protected'), 'snapshot'), good)
                else:
                    with self.assertRaises(ValueError):
                        P.original_verify(Path('/protected'), 'snapshot')


@unittest.skipUnless(os.geteuid() == 0, 'six-program protected files run in separate root CI')
class ProgramFiles(unittest.TestCase):
    # Use its established numeric owners, actual masks/transactions and isolated
    # test directories, without inheriting or rerunning its unrelated tests.
    status = support.ContactMaintenanceFiles.status
    write = staticmethod(support.ContactMaintenanceFiles.write)
    def cleanup_carriers(self):
        for path in self.carriers:
            if path.exists():
                shutil.rmtree(path)

    def setUp(self):
        support.ContactMaintenanceFiles.setUp(self)
        def replace(obj, name, value):
            context = patch.object(obj, name, value)
            context.start()
            self.addCleanup(context.stop)
        replace(P, 'verified_tool_identity', lambda: P.U.verified_tool_identity())
        self.archive, self.review_archive = self.root / 'selected-core.tar.gz', self.root / 'selected-workers.tar.gz'
        self.archive.write_bytes(b'externally pinned synthetic release data')
        with tarfile.open(self.review_archive, 'w:gz') as writer:
            for name in P.PROGRAMS[2:]:
                data = ('new program: ' + name).encode()
                # Match the real qualified package's conventional ./ spelling.
                member = tarfile.TarInfo('./' + name)
                member.size = len(data)
                writer.addfile(member, io.BytesIO(data))
        self.selected_refusal = False
        self.worker_refusal = False
        self.report_binding_refusal = False
        self.destination = self.base / 'six-programs'
        replace(P.C.PREFLIGHT, 'preflight', lambda archive, source, sha:
                {'source_commit': source, 'archive_sha256': sha})
        replace(P.C.PREFLIGHT.RELEASE, 'verify_archive', self.extract)
        replace(P.Q.I.PACKAGE, 'verify', lambda *args: None)
        self.prepared = P.prepare(self.archive, self.review_archive, 'a' * 40,
            P.U.digest(self.archive), P.U.digest(self.review_archive), self.destination)
        self.sha = P.U.digest(self.destination / 'PREPARED.json')

    def extract(self, archive, source, payload):
        for name in ('lightning-goatsd', 'lightning-goatsctl'):
            self.write(payload / name, ('new program: ' + name).encode(), 0o755)

    def command(self, *arguments, **kwargs):
        if arguments[-1] == '--help':
            self.commands.append(arguments)
            return 'synthetic native help'
        if arguments[0] == '/usr/bin/unshare':
            self.commands.append(arguments)
            self.assertIn('--no-new-privs', arguments)
            self.assertIn('--clear-groups', arguments)
            self.assertIn('--bounding-set=-all', arguments)
            name = Path(arguments[-4]).name
            role = next(r for r, (program, _) in P.WORKERS.items() if program == name)
            identity = self.identity[role]
            self.assertIn('--reuid=' + str(identity['uid']), arguments)
            self.assertIn('--regid=' + str(identity['gid']), arguments)
            config = Path(arguments[-2])
            self.assertEqual(config.stat().st_mode & 0o777, 0o440)
            self.assertEqual(config.stat().st_gid, identity['gid'])
            self.carriers.add(config.parent)
            return 'refused' if self.worker_refusal else P.WORKERS[role][1]
        if len(arguments) == 4 and arguments[-1] == 'check':
            self.commands.append(arguments)
            return 'State maintenance policy is valid; no state, credentials or services accessed'
        if len(arguments) > 3 and arguments[3] == 'review-rewards':
            self.commands.append(arguments)
            target = Path(arguments[arguments.index('--destination') + 1])
            target.mkdir(mode=0o700)
            bundle = Path(arguments[arguments.index('--bundle') + 1])
            manifest = json.loads((bundle / 'MANIFEST.json').read_text())
            review_id = str(uuid.uuid4())
            report = {'id': review_id, 'bundle_id': manifest['id'], 'bundle_manifest_sha256': P.U.digest(bundle / 'MANIFEST.json'),
                'owner': self.policy['owner'], 'epoch': self.policy['epoch'], 'all_barriers_examined': True,
                'activation_authorized': False, 'external_reconciliation_required': True}
            if self.report_binding_refusal:
                report['bundle_manifest_sha256'] = '0' * 64
            P.C.write_json(target / 'REVIEW.json', report)
            return json.dumps({'operation': 'review_rewards', 'id': review_id, 'accepted': True, 'activation_authorized': False})
        if self.selected_refusal and len(arguments) > 3 and arguments[3] == 'verify' and Path(arguments[0]).parent.name == 'payload':
            result = json.loads(support.ContactMaintenanceFiles.command(self, *arguments, **kwargs))
            result['accepted'] = False
            return json.dumps(result)
        return support.ContactMaintenanceFiles.command(self, *arguments, **kwargs)

    def migrate(self, program, config_path, component):
        # Both old and new current-schema native status must see the SAME copy.
        value = P.U.tomllib.loads(config_path.read_text())
        path = Path(value['database']['url'].removeprefix('sqlite://'))
        self.assertNotEqual(path, self.core_db)
        self.assertEqual(path.read_bytes(), self.old_bytes['core'])
        self.carriers.add(config_path.parent)
        self.migrations.append(path)
        return '{"synthetic_status":"unchanged"}'

    def assert_live_images_preserved(self):
        for role, component in self.components.items():
            self.assertEqual(Path(component['database']).read_bytes(), self.old_bytes[role])

    def assert_programs(self, version):
        for name in P.PROGRAMS:
            self.assertEqual((self.binary / name).read_bytes(), (version + ' program: ' + name).encode())

    def test_schema_upgrade_review_cannot_enter_program_only_mutations(self):
        with self.assertRaisesRegex(ValueError, 'matching program-only'):
            P.load(self.destination,self.sha,inclusive_fee_upgrade=True)
        path=self.destination/'PREPARED.json'
        record=P.C.read_json(path)
        record['program_upgrade_only']=False
        record['inclusive_fee_upgrade']=True
        path.write_text(json.dumps(record))
        digest=P.U.digest(path)
        P.load(self.destination,digest,inclusive_fee_upgrade=True)
        for operation in (P.apply,P.rollback,P.resume):
            with self.subTest(operation=operation.__name__):
                with self.assertRaisesRegex(ValueError,'matching program-only'):
                    operation(self.destination,digest,True)
        self.assert_live_images_preserved()
        self.assert_programs('old')
        self.assertEqual(self.starts,[])

    def test_complete_six_program_apply_and_resume_keep_all_configuration_and_history(self):
        self.assertEqual(self.prepared['programs'], 6)
        self.assertFalse(self.prepared['journals_opened'])
        self.assertFalse(self.prepared['activation_authorized'])
        self.assert_programs('old')
        result = P.apply(self.destination, self.sha, True)
        self.assertTrue(result['reward_backlog_reviewed'])
        self.assertFalse(result['services_started'])
        self.assertEqual(len(self.migrations), 2)
        self.assertEqual(self.starts, [])
        self.assert_programs('new')
        self.assert_live_images_preserved()

        result = P.resume(self.destination, self.sha, True)
        self.assertFalse(result['payouts_enabled'])
        self.assertEqual(self.starts, [P.C.UNIT_NAME, 'cyberherd.service'])
        for role in ('payout', 'signer'):
            self.assertTrue(P.M.masked(self.etc / (P.U.P.ROLE_NAMES[role] + '.service')))
        for path, fp in self.original_files.items():
            if Path(path).name not in P.PROGRAMS:
                self.assertEqual(P.TX.fingerprint(path), fp)
        with self.assertRaisesRegex(ValueError, 'already began'):
            P.apply(self.destination, self.sha, True)
        with self.assertRaisesRegex(ValueError, 'already began'):
            P.resume(self.destination, self.sha, True)
        with self.assertRaisesRegex(ValueError, 'already began'):
            P.rollback(self.destination, self.sha, True)

    def test_native_check_scratch_is_removed_after_success_and_parser_refusal(self):
        record = P.C.read_json(self.destination / 'PREPARED.json')
        created = []
        original = P.tempfile.mkdtemp
        def temporary(*args, **kwargs):
            result = original(*args, **kwargs)
            created.append(Path(result))
            return result
        with patch.object(P.tempfile, 'mkdtemp', side_effect=temporary):
            P.native_configuration_checks(self.destination, record)
            self.worker_refusal = True
            with self.assertRaisesRegex(ValueError, 'positively accept'):
                P.native_configuration_checks(self.destination, record)
        self.assertEqual(len(created), 2)
        self.assertTrue(all(not path.exists() for path in created))

    def test_native_check_partial_copy_failure_does_not_retain_scratch(self):
        record = P.C.read_json(self.destination / 'PREPARED.json')
        created = []
        original_temp, original_copy = P.tempfile.mkdtemp, P.TX.copy_durable
        def temporary(*args, **kwargs):
            result = original_temp(*args, **kwargs)
            created.append(Path(result))
            return result
        def copying(source, destination, fingerprint):
            if destination.parent in created:
                destination.write_bytes(b'partial native executable')
                raise OSError(28, 'synthetic scratch exhaustion')
            return original_copy(source, destination, fingerprint)
        with patch.object(P.tempfile, 'mkdtemp', side_effect=temporary), patch.object(P.TX, 'copy_durable', side_effect=copying):
            with self.assertRaises(OSError):
                P.native_configuration_checks(self.destination, record)
        self.assertEqual(len(created), 1)
        self.assertFalse(created[0].exists())
    def test_current_schema_refusal_retains_all_programs_masks_and_live_images(self):
        self.selected_refusal = True
        with self.assertRaisesRegex(ValueError, 'not positively accepted'):
            P.apply(self.destination, self.sha, True)
        self.assert_programs('old')
        self.assert_live_images_preserved()
        self.assertEqual(self.starts, [])
        self.assertEqual(self.migrations, [])
        self.assertTrue(P.M.masked(self.unit))
        self.assertFalse((self.destination / 'BACKUP-ACCEPTED.json').exists())

    def test_new_worker_refusal_stops_before_any_service_change(self):
        self.worker_refusal = True
        with self.assertRaisesRegex(ValueError, 'positively accept'):
            P.apply(self.destination, self.sha, True)
        self.assertFalse((self.destination / 'MAINTENANCE-STARTED.json').exists())
        self.assert_programs('old')
        self.assert_live_images_preserved()
        self.assertFalse(self.unit.is_symlink())

    def test_fresh_review_misbinding_refuses_before_program_replacement(self):
        self.report_binding_refusal = True
        with self.assertRaisesRegex(ValueError, 'binding changed'):
            P.apply(self.destination, self.sha, True)
        self.assert_programs('old')
        self.assert_live_images_preserved()
        self.assertEqual(self.starts, [])

    def test_interrupted_replacement_can_restore_only_programs_before_resumption(self):
        execute = P.TX.execute
        def interrupted(directory, guard, **kwargs):
            def after(index):
                if index == 2:
                    raise ValueError('synthetic replacement interruption')
            return execute(directory, guard, after_replace=after, **kwargs)
        with patch.object(P.TX, 'execute', interrupted):
            with self.assertRaisesRegex(ValueError, 'replacement interruption'):
                P.apply(self.destination, self.sha, True)
        self.assertFalse((self.destination / 'APPLIED.json').exists())
        self.assertEqual(P.rollback(self.destination, self.sha, True),
            {'programs_restored': True, 'database_images_restored': False, 'services_started': False})
        self.assert_programs('old')
        self.assert_live_images_preserved()
        with self.assertRaisesRegex(ValueError, 'rollback was selected'):
            P.resume(self.destination, self.sha, True)
        self.assertTrue(P.resume(self.destination, self.sha, True, original=True)['original_programs'])

    def test_live_image_drift_blocks_rollback_and_resumption_without_rewriting_it(self):
        P.apply(self.destination, self.sha, True)
        self.core_db.write_bytes(b'new paid state must be preserved')
        for operation in (P.resume, P.rollback):
            with self.assertRaisesRegex(ValueError, 'financial images changed'):
                operation(self.destination, self.sha, True)
        self.assertEqual(self.core_db.read_bytes(), b'new paid state must be preserved')
        self.assertEqual(self.starts, [])
        self.assert_programs('new')

    def test_selected_payload_and_staging_substitution_refuse_before_stopping(self):
        path = self.destination / 'payload/cyberherd-payoutd'
        path.write_bytes(b'unqualified payout program')
        with self.assertRaisesRegex(ValueError, 'selected native payload changed'):
            P.apply(self.destination, self.sha, True)
        self.assertFalse((self.destination / 'MAINTENANCE-STARTED.json').exists())
        self.assert_programs('old')

    def test_archive_preflight_scratch_stays_on_private_review_disk_and_restores_setting_on_failure(self):
        destination = self.base / 'failed-preflight'
        before = P.tempfile.tempdir
        def refused(*args):
            self.assertEqual(P.tempfile.tempdir, str(destination))
            self.assertEqual(destination.stat().st_mode & 0o777, 0o700)
            raise ValueError('synthetic archive verification refusal')
        with patch.object(P.C.PREFLIGHT, 'preflight', refused):
            with self.assertRaisesRegex(ValueError, 'archive verification refusal'):
                P.prepare(self.archive, self.review_archive, 'a' * 40,
                    P.U.digest(self.archive), P.U.digest(self.review_archive), destination)
        self.assertEqual(P.tempfile.tempdir, before)
        self.assertTrue((destination / 'INCOMPLETE').exists())
        self.assertFalse((destination / 'PREPARED.json').exists())
        self.assert_programs('old')
        self.assert_live_images_preserved()

    def test_backup_and_report_substitution_block_resumption(self):
        P.apply(self.destination, self.sha, True)
        path = self.destination / 'reward-review/REVIEW.json'
        path.write_bytes(b'{"substituted":true}')
        with self.assertRaisesRegex(ValueError, 'private backlog receipt changed'):
            P.resume(self.destination, self.sha, True)
        self.assertEqual(self.starts, [])
        self.assertTrue(P.M.masked(self.unit))
