"""Fresh inactive installer boundaries; no real NSS or systemd modification.

Root fixtures use protected disposable /run paths and real UID-owned files.
Native init and manager calls are simulated; the native root harness separately
qualifies actual journal initialization. These are not installed acceptance.
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
from types import SimpleNamespace
import unittest
from unittest.mock import patch

SCRIPTS = Path(__file__).resolve().parents[1] / 'scripts'
TX_SPEC = importlib.util.spec_from_file_location('inactive_file_transaction', SCRIPTS / 'inactive_file_transaction.py')
TX = importlib.util.module_from_spec(TX_SPEC)
TX_SPEC.loader.exec_module(TX)
with patch.dict(sys.modules, {'inactive_file_transaction': TX}):
    SPEC = importlib.util.spec_from_file_location('inactive_install', SCRIPTS / 'install-cyberherd-inactive.py')
    INSTALL = importlib.util.module_from_spec(SPEC)
    SPEC.loader.exec_module(INSTALL)


class InactiveBoundaries(unittest.TestCase):
    def test_direct_invocation_refuses_before_local_imports(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            shutil.copyfile(SCRIPTS / 'install-cyberherd-inactive.py', root / 'install-cyberherd-inactive.py')
            (root / 'upgrade-cyberherd-core.py').write_text('raise RuntimeError("INJECTED")')
            result = subprocess.run(['/usr/bin/python3', '-I', '-S', '-B', str(root / 'install-cyberherd-inactive.py'), '--help'],
                                    capture_output=True, text=True, timeout=10)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn('externally verified root-owned tool launcher', result.stderr)
            self.assertNotIn('INJECTED', result.stderr)

    def test_unapproved_operations_refuse_before_nss_or_unit_changes(self):
        with patch.object(INSTALL.C, 'root'), patch.object(INSTALL, 'require_uninstalled', side_effect=AssertionError('host reached')):
            with self.assertRaisesRegex(ValueError, 'identity-creation'):
                INSTALL.identities(False)
            with self.assertRaisesRegex(ValueError, 'disabled-installation'):
                INSTALL.install(None, None, None, None, None, None, None, False)

    def test_sudo_denial_must_be_complete_without_a_policy_grant_or_error(self):
        for code, stdout, stderr, accepted in [
            (1, 'User cyberherd is not allowed to run sudo on fixture.\n', '', True),
            (0, 'User cyberherd is not allowed to run sudo on fixture.\n', '', True),
            (0, '(ALL) NOPASSWD: ALL\n', '', False),
            (1, '', 'cannot load policy', False),
            (1, 'User cyberherd-payout is not allowed to run sudo on fixture.\n', '', False)]:
            with patch.object(INSTALL.subprocess, 'run', return_value=subprocess.CompletedProcess([], code, stdout, stderr)):
                if accepted:
                    INSTALL.require_no_sudo('cyberherd')
                else:
                    with self.assertRaises(ValueError):
                        INSTALL.require_no_sudo('cyberherd')


@unittest.skipUnless(os.geteuid() == 0, 'protected root filesystem fixture runs separately')
class InactiveFiles(unittest.TestCase):
    def setUp(self):
        old_umask = os.umask(0o077)
        self.addCleanup(os.umask, old_umask)
        self.temporary = tempfile.TemporaryDirectory(prefix='lg-inactive-fixture-', dir='/run')
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.base, self.bin, self.etc, self.state, self.masks, self.units = [self.root / name for name in ('upgrades', 'bin', 'etc', 'state', 'masks', 'units')]
        for parent in [self.base, self.bin, self.etc, self.state, self.masks]:
            parent.mkdir(mode=0o755)
            parent.chmod(0o700 if parent == self.base else 0o755)
        self.core = self.bin / 'lightning-goatsctl'
        self.core.write_bytes(b'original protected core CLI')
        self.core.chmod(0o755)
        self.configs = self.root / 'configs'
        self.configs.mkdir(mode=0o755)
        self.configs.chmod(0o755)
        self.bundle = self.root / 'legacy'
        self.bundle.mkdir(mode=0o700)
        self.epoch = 'f4491209-8087-4b5e-8be4-ad67fdce03ec'
        self.evidence = {'policy': {'core': {'uid': 50010}}, 'report': {'epoch': self.epoch}}
        INSTALL.C.write_json(self.bundle / 'MANIFEST.json', self.evidence)
        self.policy = self.root / 'core-policy.toml'
        self.policy.write_bytes(b'private original core policy')
        self.policy.chmod(0o600)
        self.runtime = {role: {'uid': 50011 + i, 'gid': 50011 + i} for i, role in enumerate(INSTALL.ROLES)}
        self.owner = '11' * 32
        self.raw = {
            'cyberherd': f'enabled=false\nstate_dir="{self.state}/cyberherd"\nproject_pubkey="{self.owner}"\n[core]\ncore_uid=50010\nepoch="{self.epoch}"\n',
            'cyberherd-payout': f'enabled=false\nstate_dir="{self.state}/cyberherd-payout"\nintake_uid=50011\nsigner_uid=50013\n[policy]\nexecution_enabled=false\nmember_percent_ceiling=0\nmax_sats_per_feed=0\nmax_sats_per_24h=0\nmax_fee_sats=0\n[policy.scope]\ncore_uid=50010\nepoch="{self.epoch}"\nowner="{self.owner}"\n[policy.scope.provider]\nenvironment="strike-production"\ncredential_fingerprint="' + '22' * 32 + '"\n',
            'cyberherd-signer': f'enabled=false\nstate_dir="{self.state}/cyberherd-signer"\ngroup_gid=50030\n[policy]\nmax_net_sats=0\nmax_new_per_hour=0\npeer_uid=50012\nowner="{self.owner}"\n',
        }
        for role, text in self.raw.items():
            path = self.configs / (role + '.toml')
            path.write_text(text)
            path.chmod(0o644)
        self.calls = []
        self.payload = {binary: ('fixture-' + binary).encode() for binary, _, _ in INSTALL.ROLES.values()}
        self.payload['cyberherd-statectl'] = b'fixture-statectl'
        for role in INSTALL.ROLES:
            self.payload[f'deploy/systemd/{role}.service.example'] = ('[Service]\nUser=' + role + '\n').encode()
        self.archive = self.root / 'review.tar.gz'
        with tarfile.open(self.archive, 'w:gz') as writer:
            for name, data in self.payload.items():
                member = tarfile.TarInfo('./' + name)
                member.size = len(data)
                writer.addfile(member, io.BytesIO(data))
        self.digest = hashlib.sha256(self.archive.read_bytes()).hexdigest()
        self.destination = self.base / 'install'
        replacements = [('BIN', self.bin), ('ETC', self.etc), ('STATE', self.state), ('MASKS', self.masks), ('UNITS', self.units)]
        for name, value in replacements:
            mock = patch.object(INSTALL, name, value)
            mock.start()
            self.addCleanup(mock.stop)
        for object_, name, value in [
            (INSTALL.C, 'BASE', self.base), (INSTALL.C, 'root', lambda: None),
            (INSTALL.C, 'command', self.command), (INSTALL.C, 'status', self.status),
            (INSTALL, 'check_identities', lambda: self.runtime),
            (INSTALL, 'verified_tool_identity', lambda: {'source': 'fixture'}),
            (INSTALL.pwd, 'getpwnam', lambda name: SimpleNamespace(pw_uid=50010)),
            (INSTALL.grp, 'getgrnam', lambda name: SimpleNamespace(gr_gid=50030)),
            (INSTALL.PACKAGE, 'verify', lambda archive, source: {'source_commit': source, 'activation': 'none'})]:
            mock = patch.object(object_, name, value)
            mock.start()
            self.addCleanup(mock.stop)

    def status(self, name):
        path = self.masks / name
        masked = path.is_symlink()
        return {'LoadState': 'masked' if masked else 'not-found', 'ActiveState': 'inactive', 'SubState': 'dead',
                'MainPID': '0', 'UnitFileState': 'masked' if masked else '', 'NeedDaemonReload': 'no'}

    def command(self, *args, **kwargs):
        self.calls.append(args)
        if 'verify-core-upgrade' in args:
            return json.dumps({'accepted': True, 'resume_authorized': False, 'id': 'fixture-core-evidence'})
        if '--initialize-state' in args:
            role = next(role for role, (binary, _, _) in INSTALL.ROLES.items() if args[-4] == str(self.bin / binary))
            journal = self.state / role / INSTALL.ROLES[role][2]
            self.assertIn('--clear-groups', args)
            self.assertIn('--no-new-privs', args)
            self.assertTrue((self.masks / (role + '.service')).is_symlink())
            journal.write_bytes(b'fixture empty schema')
            journal.chmod(0o600)
            os.chown(journal, self.runtime[role]['uid'], self.runtime[role]['gid'])
        return ''

    def invoke(self, digest=None):
        return INSTALL.install(self.archive, 'a' * 40, digest or self.digest, self.configs, self.policy,
                               self.bundle, self.destination, True)

    def test_fresh_install_keeps_masks_zero_policy_private_journals_and_original_core(self):
        before = self.core.read_bytes()
        result = self.invoke()
        self.assertFalse(result['credentials_loaded'])
        self.assertFalse(result['services_started'])
        self.assertTrue(result['services_masked'])
        self.assertFalse((self.destination / 'INCOMPLETE').exists())
        for role, (_, _, journal) in INSTALL.ROLES.items():
            self.assertEqual(os.readlink(self.masks / (role + '.service')), '/dev/null')
            self.assertEqual((self.etc / role).stat().st_mode & 0o777, 0o755)
            self.assertEqual((self.etc / role / 'config.toml').stat().st_mode & 0o777, 0o644)
            self.assertEqual((self.state / role).stat().st_mode & 0o777, 0o700)
            self.assertEqual((self.state / role / journal).stat().st_uid, self.runtime[role]['uid'])
            self.assertTrue((self.units / (role + '.service')).is_file())
        self.assertEqual(self.core.read_bytes(), before)
        self.assertFalse(any(call[:2] in [('/usr/bin/systemctl', 'start'), ('/usr/bin/systemctl', 'enable'), ('/usr/bin/systemctl', 'stop')] for call in self.calls))
        with self.assertRaisesRegex(ValueError, 'remain absent'):
            self.invoke()

    def test_native_initialization_failure_retains_mask_and_partial_state(self):
        original = self.command
        def fail(*args, **kwargs):
            if '--initialize-state' in args:
                raise ValueError('synthetic native init refusal')
            return original(*args, **kwargs)
        with patch.object(INSTALL.C, 'command', fail):
            with self.assertRaisesRegex(ValueError, 'native init refusal'):
                self.invoke()
        self.assertTrue((self.destination / 'INCOMPLETE').exists())
        self.assertFalse((self.destination / 'INSTALLED.json').exists())
        for role in INSTALL.ROLES:
            self.assertEqual(os.readlink(self.masks / (role + '.service')), '/dev/null')
        self.assertEqual(self.core.read_bytes(), b'original protected core CLI')

    def test_archive_digest_refusal_precedes_all_installed_paths(self):
        with self.assertRaisesRegex(ValueError, 'archive digest'):
            self.invoke('0' * 64)
        self.assertTrue((self.destination / 'INCOMPLETE').exists())
        self.assertEqual(list(self.masks.iterdir()), [])
        self.assertEqual(list(self.state.iterdir()), [])
        self.assertEqual(list(self.bin.iterdir()), [self.core])

    def test_nonzero_or_foreign_scope_configuration_refuses_before_installation(self):
        path = self.configs / 'cyberherd-payout.toml'
        for old, new in [('max_sats_per_feed=0', 'max_sats_per_feed=1000'),
                         ('max_fee_sats=0', 'max_fee_sats=0\nmax_net_sats=1'),
                         ('max_fee_sats=0', 'max_fee_sats=0\nmax_net_sats=false'),
                         ('intake_uid=50011', 'intake_uid=50010'),
                         (self.epoch, '00000000-0000-0000-0000-000000000001'),
                         ('22' * 32, '0' * 64)]:
            path.write_text(self.raw['cyberherd-payout'].replace(old, new))
            with self.assertRaises(ValueError):
                self.invoke()
            self.assertFalse(self.destination.exists())
            self.assertEqual(list(self.masks.iterdir()), [])
        self.assertEqual(self.core.read_bytes(), b'original protected core CLI')
