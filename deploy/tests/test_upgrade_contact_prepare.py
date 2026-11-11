"""Preparation-only tests; root cases use disposable files and mocked service state."""
import copy
import hashlib
import importlib.util
import io
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tarfile
import tempfile
import unittest
from unittest.mock import patch

SCRIPTS = Path(__file__).resolve().parents[1] / 'scripts'
spec = importlib.util.spec_from_file_location('inactive_file_transaction', SCRIPTS / 'inactive_file_transaction.py')
TX = importlib.util.module_from_spec(spec)
spec.loader.exec_module(TX)
with patch.dict(sys.modules, {'inactive_file_transaction': TX}):
    spec = importlib.util.spec_from_file_location('contact_upgrade_preparation', SCRIPTS / 'prepare-contact-upgrade.py')
    PREPARE = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(PREPARE)


def fixture():
    owner, epoch = 'a' * 64, '11111111-1111-4111-8111-111111111111'
    provider = {'environment': 'strike-production', 'account_id': epoch, 'credential_fingerprint': 'b' * 64}
    reward = {'owner': owner, 'member_percent_ceiling': 0, 'max_sats_per_feed': 0, 'max_sats_per_24h': 0}
    core = {'database': {'url': 'sqlite://' + str(PREPARE.C.DATABASE)}, 'nostr': {'bunker_pubkey': owner},
            'core_bridge': {'reward_policy': reward, 'payout_execution_enabled': False, 'delegated_feeder_messages': False}}
    member = {'enabled': True, 'project_pubkey': owner, 'core': {'epoch': epoch, 'core_uid': 10001},
              'messaging': {'enabled': True, 'lifecycle': False}}
    payout = {'enabled': False, 'policy': {'execution_enabled': False, 'member_percent_ceiling': 0,
              'max_sats_per_feed': 0, 'max_sats_per_24h': 0, 'max_fee_sats': 0,
              'scope': {'epoch': epoch, 'owner': owner, 'provider': provider}}}
    signer = {'enabled': False, 'policy': {'max_net_sats': 0, 'max_new_per_hour': 0}}
    policy = {'owner': owner, 'epoch': epoch, 'provider': provider, 'components': {'core': {'uid': 10001}}}
    return [core, member, payout, signer, policy]


class ContactPreparationBoundaries(unittest.TestCase):
    def test_direct_execution_refuses_before_substituted_imports(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            shutil.copyfile(SCRIPTS / 'prepare-contact-upgrade.py', root / 'prepare.py')
            (root / 'stage-cyberherd-overlay.py').write_text('raise RuntimeError("INJECTED")')
            result = subprocess.run(['/usr/bin/python3', '-I', '-S', '-B', str(root / 'prepare.py'), '--help'],
                                    capture_output=True, text=True, timeout=10)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn('externally verified root-owned tool launcher', result.stderr)
            self.assertNotIn('INJECTED', result.stderr)

    def test_only_existing_zero_budget_overlay_stage_is_accepted(self):
        values = fixture()
        before = copy.deepcopy(values)
        PREPARE.configuration_boundary(*values)
        self.assertEqual(values, before)
        changes = [(0, ('contact',), {}), (0, ('core_bridge', 'payout_execution_enabled'), True),
                   (0, ('core_bridge', 'reward_policy', 'max_sats_per_feed'), 1),
                   (1, ('messaging', 'signer'), {}), (1, ('messaging', 'lifecycle'), True),
                   (1, ('rewards',), {}), (1, ('core', 'epoch'), 'changed'),
                   (2, ('policy', 'execution_enabled'), True), (2, ('policy', 'max_fee_sats'), True),
                   (3, ('policy', 'max_new_per_hour'), 1)]
        for index, keys, value in changes:
            changed = copy.deepcopy(values)
            target = changed[index]
            for key in keys[:-1]:
                target = target[key]
            target[keys[-1]] = value
            with self.subTest(keys=keys), self.assertRaises(ValueError):
                PREPARE.configuration_boundary(*changed)

    def test_archives_are_pinned_and_never_overwritten(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            original, staged = root / 'archive', root / 'staged'
            original.write_bytes(b'synthetic data only')
            digest = hashlib.sha256(original.read_bytes()).hexdigest()
            PREPARE.stage_archive(original, staged, digest)
            self.assertEqual(staged.read_bytes(), original.read_bytes())
            with self.assertRaises(FileExistsError):
                PREPARE.stage_archive(original, staged, digest)
            with self.assertRaises(ValueError):
                PREPARE.stage_archive(original, root / 'rejected', '0' * 64)
            self.assertTrue((root / 'rejected').exists())
            alias = root / 'alias'
            alias.symlink_to(original)
            with self.assertRaises(ValueError):
                PREPARE.stage_archive(alias, root / 'aliased-copy', digest)
            self.assertFalse((root / 'aliased-copy').exists())


@unittest.skipUnless(os.geteuid() == 0, 'protected preparation fixture requires root')
class ContactPreparationFiles(unittest.TestCase):
    def test_preparation_preserves_installed_files_and_has_no_activation_command(self):
        old_umask = os.umask(0o077)
        self.addCleanup(os.umask, old_umask)
        with tempfile.TemporaryDirectory(prefix='lg-contact-prepare-', dir='/run') as directory:
            root = Path(directory)
            base, binary = root / 'upgrades', root / 'bin'
            base.mkdir(mode=0o700)
            binary.mkdir(mode=0o755)
            old = {}
            for name in PREPARE.TARGET_NAMES:
                path = binary / name
                path.write_text('old program: ' + name)
                path.chmod(0o755)
                old[str(path)] = TX.fingerprint(path)
            baseline = {'files': old, 'states': {'core': {'MainPID': '123'}, 'membership': {'MainPID': '456'}},
                        'identities': {'core': {'uid': 50001, 'gid': 50001}}}
            archive, review = root / 'core.tar.gz', root / 'review.tar.gz'
            archive.write_bytes(b'fixture archive, verified by test substitute')
            with tarfile.open(review, 'w:gz') as output:
                data = b'new native state verifier'
                member = tarfile.TarInfo('cyberherd-statectl')
                member.size = len(data)
                output.addfile(member, io.BytesIO(data))

            def extract(_archive, _source, payload):
                for name in ('lightning-goatsd', 'lightning-goatsctl'):
                    path = payload / name
                    path.write_text('new program: ' + name)
                    path.chmod(0o755)

            commands = []
            with patch.object(PREPARE.C, 'root'), patch.object(PREPARE.C, 'BASE', base), \
                    patch.object(PREPARE.Q, 'BIN', binary), patch.object(PREPARE, 'baseline', return_value=baseline), \
                    patch.object(PREPARE, 'verified_tool_identity', return_value={'source': 'fixture'}), \
                    patch.object(PREPARE.C.PREFLIGHT, 'preflight', return_value={'source_commit': 'a' * 40}), \
                    patch.object(PREPARE.C.PREFLIGHT.RELEASE, 'verify_archive', side_effect=extract), \
                    patch.object(PREPARE.Q.I.PACKAGE, 'verify') as review_check, \
                    patch.object(PREPARE.C, 'command', side_effect=lambda *args, **kwargs: commands.append(args)):
                result = PREPARE.prepare(archive, review, 'a' * 40,
                                         hashlib.sha256(archive.read_bytes()).hexdigest(),
                                         hashlib.sha256(review.read_bytes()).hexdigest(), base / 'prepared')
                self.assertTrue(result['prepared'])
                self.assertFalse(result['activation_supported'])
                self.assertFalse(result['journals_opened'])
                self.assertEqual(review_check.call_count, 1)
                self.assertEqual(len(commands), 5)
                self.assertEqual([c[1:] for c in commands[:3]], [('--help',)] * 3)
                self.assertEqual(commands[3][1:], ('--config', str(PREPARE.S.MAINTENANCE), 'check'))
                self.assertEqual(commands[-1][:4], ('/usr/bin/unshare', '--net', '--', '/usr/bin/setpriv'))
                self.assertEqual(commands[-1][-1], '--help')
                self.addCleanup(shutil.rmtree, Path(commands[-1][-2]).parent)
                self.assertFalse((base / 'prepared/INCOMPLETE').exists())
                self.assertTrue((base / 'prepared/PREPARED.json').exists())
                for path, fingerprint in old.items():
                    self.assertEqual(TX.fingerprint(path), fingerprint)
                with self.assertRaises(FileExistsError):
                    PREPARE.prepare(archive, review, 'a' * 40,
                                    hashlib.sha256(archive.read_bytes()).hexdigest(),
                                    hashlib.sha256(review.read_bytes()).hexdigest(), base / 'prepared')
            self.assertFalse(hasattr(PREPARE, 'apply'))
            self.assertFalse(hasattr(PREPARE, 'resume'))
