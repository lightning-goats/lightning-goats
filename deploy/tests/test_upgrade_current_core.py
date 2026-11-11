"""Current-schema maintenance orchestration; native/systemd are mocked here.

Root cases use protected disposable files, distinct owners and real program
transactions. Native schema acceptance and actual UID execution are qualified
separately by the guarded disposable systemd rehearsal, never by these mocks.
"""
import json
import os
from pathlib import Path
import unittest
from unittest.mock import patch

import test_upgrade_contact_maintenance as maintenance
import test_upgrade_contact_prepare as preparation

U = maintenance.U


class CurrentCoreBoundaries(unittest.TestCase):
    def test_explicit_patch_mode_cannot_reuse_migration_or_numeric_flags(self):
        for record, mode in (({}, True), ({'current_schema_patch': True}, False),
                             ({'current_schema_patch': 0}, False),
                             ({'current_schema_patch': 1, 'schema_upgrade_authorized': False}, True),
                             ({'current_schema_patch': True, 'schema_upgrade_authorized': True}, True)):
            with self.assertRaisesRegex(ValueError, 'cannot be interchanged'):
                U.patch_mode(record, mode)
        U.patch_mode({}, False)
        U.patch_mode({'current_schema_patch': True, 'schema_upgrade_authorized': False}, True)

    def test_existing_contact_requires_explicit_patch_mode_and_same_owner(self):
        values = preparation.fixture()
        values[0]['contact'] = {'recipient': values[4]['owner'], 'inbox_relays': ['wss://relay.example/']}
        with self.assertRaises(ValueError):
            U.P.configuration_boundary(*values)
        U.P.configuration_boundary(*values, current_schema=True)
        values[0]['contact']['recipient'] = '00' * 32
        with self.assertRaisesRegex(ValueError, 'contact identity'):
            U.P.configuration_boundary(*values, current_schema=True)


@unittest.skipUnless(os.geteuid() == 0, 'actual protected file transactions require root')
class CurrentCoreFiles(unittest.TestCase):
    write = staticmethod(maintenance.ContactMaintenanceFiles.write)
    status = maintenance.ContactMaintenanceFiles.status
    cleanup_carriers = maintenance.ContactMaintenanceFiles.cleanup_carriers

    def setUp(self):
        maintenance.ContactMaintenanceFiles.setUp(self)
        # The legacy setup is retained as a separate completed plan. A new,
        # explicitly tagged preparation is required for this different policy.
        self.execution = self.base / 'current-execution'
        self.rewrite_current_review()
        U.plan(self.review, self.review_sha, self.execution, current_schema=True)

    def rewrite_current_review(self):
        core = U.tomllib.loads(self.config.read_text())
        core['contact'] = {'recipient': self.policy['owner'], 'inbox_relays': ['wss://relay.example/']}
        self.write(self.config, U.S.W.toml_text(core).encode(), 0o600)
        fp = U.TX.fingerprint(self.config)
        self.baseline['files'][str(self.config)] = fp
        original = U.original(self.review, {'original_order': list(self.original_files)}, self.config)
        self.write(original, self.config.read_bytes(), 0o600)
        review = json.loads((self.review / 'PREPARED.json').read_text())
        review.update(baseline=self.baseline, contact_enabled=True,
                      current_schema_patch=True, schema_upgrade_authorized=False)
        (self.review / 'PREPARED.json').unlink()
        U.C.write_json(self.review / 'PREPARED.json', review)
        self.review_sha = U.digest(self.review / 'PREPARED.json')

    def command(self, *arguments, **kwargs):
        return maintenance.ContactMaintenanceFiles.command(self, *arguments, **kwargs)

    def migrate(self, program, config_path, component):
        path = Path(U.tomllib.loads(config_path.read_text())['database']['url'].removeprefix('sqlite://'))
        self.assertNotEqual(path, self.core_db)
        self.assertTrue((self.execution / 'BACKUP-ACCEPTED.json').exists())
        self.assertTrue(any(args[0] == str(self.review / 'payload/cyberherd-statectl') and args[3] == 'verify'
                            for args in self.commands))
        self.assertEqual(path.read_bytes(), self.old_bytes['core'])
        self.carriers.add(config_path.parent)
        self.migrations.append(path)
        return 'mode=active\nfeed_credit_sats=28\nunresolved_feed_attempt_id=none\n'

    def apply(self):
        return U.apply(self.execution, True, current_schema=True)

    def resume(self):
        return U.resume(self.execution, True, current_schema=True)

    def test_patch_changes_programs_only_preserves_contact_and_all_journal_bytes(self):
        configs = {path: U.TX.fingerprint(path) for path in self.original_files if path.endswith('.toml')}
        result = self.apply()
        self.assertIs(result['live_migration_performed'], False)
        self.assertIs(result['financial_images_unchanged'], True)
        self.assertIs(result['contact_enabled'], True)
        self.assertEqual(len(self.migrations), 2)
        self.assertEqual(self.starts, [])
        self.assertFalse((self.execution / 'MIGRATION-STARTED.json').exists())
        self.assertFalse((self.execution / 'migrated').exists())
        for role, component in self.components.items():
            self.assertEqual(Path(component['database']).read_bytes(), self.old_bytes[role])
        for name in U.P.TARGET_NAMES:
            self.assertEqual((self.binary / name).read_bytes(), ('new program: ' + name).encode())
        self.assertTrue(self.resume()['contact_enabled'])
        self.assertEqual(self.starts, ['lightning-goats-receive.service', 'cyberherd.service'])
        for path, fp in configs.items():
            self.assertEqual(U.TX.fingerprint(path), fp)
        for role in ('payout', 'signer'):
            self.assertTrue(U.M.masked(self.etc / (U.P.ROLE_NAMES[role] + '.service')))
        with self.assertRaisesRegex(ValueError, 'already began'):
            self.apply()
        with self.assertRaisesRegex(ValueError, 'already began'):
            self.resume()

    def test_wrong_schema_build_refused_before_copy_open_or_program_replacement(self):
        command = self.command

        def refuse_selected(*args, **kwargs):
            if args[0] == str(self.review / 'payload/cyberherd-statectl') and args[3] == 'verify':
                raise ValueError('selected compiled schema differs')
            return command(*args, **kwargs)

        with patch.object(U.C, 'command', refuse_selected):
            with self.assertRaisesRegex(ValueError, 'compiled schema differs'):
                self.apply()
        self.assertEqual(self.migrations, [])
        self.assertEqual(self.starts, [])
        self.assertTrue(U.M.masked(self.unit))
        for name in U.P.TARGET_NAMES:
            self.assertEqual((self.binary / name).read_bytes(), ('old program: ' + name).encode())
        self.assertEqual(self.core_db.read_bytes(), self.old_bytes['core'])

    def test_changed_candidate_status_fails_before_live_programs_or_journals(self):
        original = self.migrate
        with patch.object(U, 'migrate_as_runtime', side_effect=lambda program, *args:
                          original(program, *args) + ('changed\n' if program.name == 'lightning-goatsctl' else '')):
            with self.assertRaisesRegex(ValueError, 'config/status differs'):
                self.apply()
        self.assertEqual(self.starts, [])
        self.assertEqual(self.core_db.read_bytes(), self.old_bytes['core'])
        self.assertEqual((self.binary / 'lightning-goatsd').read_bytes(), b'old program: lightning-goatsd')

    def test_current_patch_cannot_apply_as_schema_upgrade(self):
        with self.assertRaisesRegex(ValueError, 'cannot be interchanged'):
            U.apply(self.execution, True)
        self.assertEqual(self.commands, [])

    def test_changed_live_image_blocks_resume_without_rollback(self):
        self.apply()
        self.core_db.write_bytes(b'unreviewed financial state')
        with self.assertRaisesRegex(ValueError, 'financial images changed'):
            self.resume()
        self.assertEqual(self.starts, [])
        self.assertTrue(U.M.masked(self.unit))
        self.assertEqual(self.core_db.read_bytes(), b'unreviewed financial state')

    def test_retained_old_verifier_drift_refuses_before_service_stop(self):
        (self.execution / 'payload/cyberherd-statectl').write_bytes(b'unreviewed verifier')
        with self.assertRaisesRegex(ValueError, 'retained current native verifier changed'):
            self.apply()
        self.assertEqual(self.commands, [])
        self.assertFalse(self.unit.is_symlink())
