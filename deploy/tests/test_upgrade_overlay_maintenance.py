"""Real disposable file recovery; systemd/native commands are explicitly mocked.

The native four-journal/no-core-bridge fixture is a separate mandatory Rust test.
These tests prove no live service, credential, SQLite parser or network behavior.
"""
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import unittest
import uuid
from unittest.mock import patch

import test_upgrade_overlay_stage as prep

s, configs = prep.s, prep.configs

SCRIPTS = Path(__file__).resolve().parents[1] / 'scripts'
with patch.dict(sys.modules, {'inactive_file_transaction': s.C.TX}):
    spec = importlib.util.spec_from_file_location('overlay_maintenance_test', SCRIPTS / 'maintain-cyberherd-overlay.py')
    m = importlib.util.module_from_spec(spec); spec.loader.exec_module(m)


class MaintenanceBoundaries(unittest.TestCase):
    def test_attestations_refuse_before_any_host_access(self):
        with patch.object(m, 'load', side_effect=AssertionError('host reached')):
            for method in (m.apply, m.rollback, m.resume):
                with self.assertRaisesRegex(ValueError, 'explicit'):
                    method(Path('/missing'), False)

    def test_direct_execution_refuses_before_local_import(self):
        import tempfile
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary)
            shutil.copyfile(SCRIPTS / 'maintain-cyberherd-overlay.py', path / 'maintain.py')
            (path / 'stage-cyberherd-overlay.py').write_text('raise RuntimeError("POISON")')
            result = subprocess.run([sys.executable, '-I', '-S', '-B', str(path / 'maintain.py'), '--help'], capture_output=True, text=True)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn('verified root-owned tool launcher', result.stderr)
            self.assertNotIn('POISON', result.stderr)


@unittest.skipUnless(os.geteuid() == 0, 'protected recovery fixture runs separately as root')
class OverlayMaintenanceFiles(prep.OverlayStageFiles):
    # Reuse only the protected /run fixture setup, not its two inherited cases.
    test_preparation_freezes_candidates_and_exact_originals_without_installation = None
    test_native_rejection_retains_incomplete_data_and_original_configuration = None

    def setUp(self):
        super().setUp()
        previous = os.umask(0o077); self.addCleanup(os.umask, previous)
        for name, value in [('S', s), ('C', s.C), ('Q', s.Q), ('TX', s.C.TX), ('MASKS', self.etc)]:
            context = patch.object(m, name, value); context.start(); self.addCleanup(context.stop)
        for name, value in [('stable', lambda r: None), ('running', lambda *args: {}),
                            ('verified_tool_identity', lambda: {'source': 'maintenance-fixture'})]:
            context = patch.object(m, name, value); context.start(); self.addCleanup(context.stop)
        context = patch.object(s.C, 'UNIT_NAME', self.unit.name); context.start(); self.addCleanup(context.stop)
        self.original['installation']['core']['properties'] = {'DropInPaths': ''}
        core, member = configs()
        self.core_path.write_text(s.W.toml_text(core)); self.member_path.write_text(s.W.toml_text(member))
        self.original['installation']['core']['files'][str(self.core_path)] = s.C.TX.fingerprint(self.core_path)
        self.components = {}
        for i, role in enumerate(('core', 'membership', 'payout', 'signer')):
            parent = self.root / role; parent.mkdir(mode=0o700)
            path = parent / (role + '.db'); path.write_bytes(('RETAIN FINANCIAL AND SIGNED HISTORY ' + role).encode()); path.chmod(0o600)
            uid = 10001 + i; os.chown(path, uid, uid)
            self.components[role] = {'database': str(path), 'uid': uid, 'gid': uid}
        self.policy.write_text(s.W.toml_text({'max_component_bytes': 1024*1024, 'components': self.components}))
        self.original['maintenance'] = s.C.TX.fingerprint(self.policy)
        self.running_units = {self.unit.name: True, **{r + '.service': False for r in s.Q.ROLES}}
        self.start_counts = {name: 0 for name in self.running_units}
        for role in s.Q.ROLES:
            (self.etc / (role + '.service')).symlink_to('/dev/null')
        self.native_accepted = True; self.native_snapshots = 0; self.command_log = []
        self.command.side_effect = self.command_impl
        context = patch.object(s.C, 'status', side_effect=self.status); context.start(); self.addCleanup(context.stop)
        context = patch.object(s.Q, 'no_role_processes', side_effect=lambda p: self.assertEqual(set(p), set(self.components)))
        context.start(); self.addCleanup(context.stop)
        self.invoke()
        self.execution = self.base / 'execution'
        self.review_sha = m.digest(self.destination / 'PREPARED.json')
        self.initial_bytes = {c['database']: Path(c['database']).read_bytes() for c in self.components.values()}
        m.plan(self.destination, self.review_sha, self.execution)
        self.initial_core = self.core_path.read_bytes(); self.initial_member = self.member_path.read_bytes()

    def status(self, name):
        path = self.etc / name
        masked = path.is_symlink() and os.readlink(path) == '/dev/null'
        live = self.running_units[name]
        return {'Id': name, 'LoadState': 'masked' if masked else 'loaded',
                'ActiveState': 'active' if live else 'inactive', 'SubState': 'running' if live else 'dead',
                'MainPID': '12345' if live else '0', 'UnitFileState': 'masked' if masked else 'disabled',
                'NeedDaemonReload': 'no', 'ControlGroup': '', 'User': 'lightning-goats' if name == self.unit.name else name[:-8],
                'FragmentPath': str(self.unit if name == self.unit.name else s.Q.UNITS / name), 'DropInPaths': ''}

    def command_impl(self, *args, **kwargs):
        self.command_log.append(args)
        if args[0] == '/usr/bin/systemctl':
            if args[1] in ('stop', 'start'):
                name = args[-1]; wanted = args[1] == 'start'
                if wanted and not self.running_units[name]: self.start_counts[name] += 1
                self.running_units[name] = wanted
            return ''
        if Path(args[0]).name == 'cyberherd-statectl':
            if 'snapshot' in args:
                self.native_snapshots += 1
                bundle = Path(args[args.index('--destination') + 1]); bundle.mkdir(mode=0o700)
                images = []
                for role, c in self.components.items():
                    data = Path(c['database']).read_bytes(); path = bundle / (role + '.db')
                    path.write_bytes(data); path.chmod(0o600)
                    images.append({'name': path.name, 'sha256': hashlib.sha256(data).hexdigest(), 'bytes': len(data)})
                s.C.write_json(bundle / 'MANIFEST.json', {'id': str(uuid.uuid4()), 'source_policy_sha256': m.digest(self.policy), 'images': images})
                return '{}'
            bundle = Path(args[args.index('--bundle') + 1])
            manifest = json.loads((bundle / 'MANIFEST.json').read_text())
            return json.dumps({'operation': 'verify', 'id': manifest['id'], 'accepted': self.native_accepted, 'resume_authorized': False})
        if args[0] in ('/usr/bin/curl', '/usr/bin/systemd-analyze'): return ''
        return json.dumps({'accepted': True, 'configuration_only': True, 'ledger_opened': False,
                           'credentials_loaded': False, 'provider_operations': 0,
                           'services_changed': False, 'payout_capabilities_enabled': False})

    def assert_journals(self, expected=None):
        for path, data in (self.initial_bytes if expected is None else expected).items():
            self.assertEqual(Path(path).read_bytes(), data)

    def test_plan_is_inactive_and_receipt_or_candidate_drift_refuses_before_stop(self):
        self.assertTrue(self.running_units[self.unit.name]); self.assertEqual(self.native_snapshots, 0)
        self.assertFalse(s.SOCKET.parent.exists()); self.assertFalse(s.TMPFILE.exists())
        candidate = self.destination / 'membership.toml'; candidate.write_text('changed')
        with self.assertRaisesRegex(ValueError, 'candidate changed'): m.apply(self.execution, True)
        self.assertTrue(self.running_units[self.unit.name]); self.assertFalse((self.execution / 'MAINTENANCE-STARTED.json').exists())
        self.assert_journals()

    def test_apply_then_explicit_resume_and_config_only_rollback_preserve_new_paid_state(self):
        m.apply(self.execution, True)
        self.assertTrue(all(not v for v in self.running_units.values()))
        self.assertTrue(m.masked(self.unit)); self.assertEqual(self.native_snapshots, 1)
        self.assertNotEqual(self.core_path.read_bytes(), self.initial_core)
        self.assert_journals()
        m.resume(self.execution, True)
        self.assertEqual(self.start_counts[self.unit.name], 1)
        self.assertEqual(self.start_counts['cyberherd.service'], 1)
        self.assertEqual(self.start_counts['cyberherd-payout.service'], 0)
        with self.assertRaisesRegex(ValueError, 'already completed'): m.resume(self.execution, True)
        after = {}
        for c in self.components.values():
            path = Path(c['database']); path.write_bytes(path.read_bytes() + b' NEW PAID OR REPLAY HISTORY'); after[str(path)] = path.read_bytes()
        m.rollback(self.execution, True)
        self.assertEqual(self.core_path.read_bytes(), self.initial_core)
        self.assertEqual(self.member_path.read_bytes(), self.initial_member)
        self.assert_journals(after)
        self.assertTrue(all(not v for v in self.running_units.values()))
        self.assertFalse(s.TMPFILE.exists())
        with self.assertRaisesRegex(ValueError, 'rollback already selected'): m.resume(self.execution, True)
        m.resume(self.execution, True, core_only=True)
        self.assertTrue(self.running_units[self.unit.name]); self.assertFalse(self.running_units['cyberherd.service'])
        self.assertEqual(self.unit.read_bytes(), (self.destination / 'original-1').read_bytes())
        self.assert_journals(after)

    def test_partial_configuration_rename_resumes_without_new_backup_or_history_rewind(self):
        execute = s.C.TX.execute
        def cut(*args, **kwargs):
            def interrupted(index):
                if index == 0: raise RuntimeError('interrupted after rename')
            return execute(*args, **kwargs, after_replace=interrupted)
        with patch.object(s.C.TX, 'execute', side_effect=cut):
            with self.assertRaisesRegex(RuntimeError, 'after rename'): m.apply(self.execution, True)
        self.assertTrue(m.masked(self.unit)); self.assertNotEqual(self.core_path.read_bytes(), self.initial_core)
        self.assertEqual(self.member_path.read_bytes(), self.initial_member)
        m.apply(self.execution, True)
        self.assertEqual(self.native_snapshots, 1); self.assert_journals()
        self.assertFalse(any(self.start_counts.values()))

    def test_failed_backup_blocks_replacement_and_original_core_can_be_resumed_after_rollback(self):
        self.native_accepted = False
        with self.assertRaisesRegex(ValueError, 'backup not positively'): m.apply(self.execution, True)
        self.assertEqual(self.core_path.read_bytes(), self.initial_core)
        self.assertTrue(m.masked(self.unit)); self.assertFalse(any(self.start_counts.values()))
        m.rollback(self.execution, True); m.resume(self.execution, True, core_only=True)
        self.assert_journals(); self.assertFalse(self.running_units['cyberherd.service'])

    def test_interrupted_resumption_reuses_running_core_and_refuses_changed_configuration(self):
        m.apply(self.execution, True)
        normal = self.command_impl
        def cut(*args, **kwargs):
            if args[:2] == ('/usr/bin/systemctl', 'start') and args[-1] == 'cyberherd.service':
                raise RuntimeError('interrupted before intake start')
            return normal(*args, **kwargs)
        self.command.side_effect = cut
        with self.assertRaisesRegex(RuntimeError, 'before intake'): m.resume(self.execution, True)
        self.assertTrue(self.running_units[self.unit.name]); self.assertFalse(self.running_units['cyberherd.service'])
        self.command.side_effect = normal
        m.resume(self.execution, True)
        self.assertEqual(self.start_counts[self.unit.name], 1)
        self.assertEqual(self.start_counts['cyberherd.service'], 1); self.assert_journals()


if __name__ == '__main__': unittest.main()
