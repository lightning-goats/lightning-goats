"""Read-only stage preparation. No real service, journal parser or network."""
import copy
import importlib.util
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import MagicMock, patch

SCRIPTS = Path(__file__).resolve().parents[1] / 'scripts'
spec = importlib.util.spec_from_file_location('inactive_file_transaction', SCRIPTS / 'inactive_file_transaction.py')
tx = importlib.util.module_from_spec(spec); spec.loader.exec_module(tx)
with patch.dict(sys.modules, {'inactive_file_transaction': tx}):
    spec = importlib.util.spec_from_file_location('overlay_stage', SCRIPTS / 'stage-cyberherd-overlay.py')
    s = importlib.util.module_from_spec(spec); spec.loader.exec_module(s)

OWNER = '79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798'
OTHER = 'c6047f9441ed7d6d3045406e95c07cd85c778e4b8cef3ca7abac09b95c709ee5'
ACCEPTED = {'accepted': True, 'configuration_only': True, 'ledger_opened': False,
            'credentials_loaded': False, 'provider_operations': 0,
            'services_changed': False, 'payout_capabilities_enabled': False}


def configs():
    core = {'database': {'url': 'sqlite:///private/core.db'}, 'lnurl': {'public_base_url': 'https://lightning-goats.com/'},
            'nostr': {'bunker_pubkey': OWNER}, 'service': {'mode': 'active'},
            'feeder': {'threshold_sats': 1000}, 'gateway': {'url': 'http://10.8.0.6:8789/'},
            'lightning_address': [{'user': user} for user in sorted(s.USERS)]}
    member = {'enabled': False, 'state_dir': '/var/lib/cyberherd', 'core_socket': str(s.SOCKET),
              'project_pubkey': OWNER, 'relays': ['wss://relay.primal.net'], 'timezone': 'America/Denver',
              'midnight_reset': True, 'core': {'core_uid': 10001, 'epoch': 'retained-epoch', 'start_after': 321,
                                            'payees': {u: OTHER if u == 'nova' else OWNER for u in s.USERS}},
              'settings': {'minimum_sats': 10, 'max_members': 10, 'member_percent': 10},
              'rewards': {'minimum_fee_sats': 1}, 'payout_bridge': {'peer_uid': 10004},
              'messaging': {'enabled': False, 'signer': {'client_pubkey': OTHER}},
              'view': {'enabled': False, 'auth': {'operators': [OWNER]}}}
    return core, member


class OverlayStageTests(unittest.TestCase):
    def test_configuration_render_preserves_paid_routing_bindings_and_zero_settings(self):
        core, member = configs(); before = copy.deepcopy((core, member))
        member['settings'].update(minimum_sats=0, member_percent=0)
        new_core, new_member = s.render(core, member, 10002, 10003, 0)
        stripped = copy.deepcopy(new_core); stripped.pop('core_bridge'); stripped['lnurl'].pop('zap')
        self.assertEqual(stripped, core)
        self.assertEqual(new_member['core'], member['core'])
        self.assertEqual(new_member['settings'], member['settings'])
        self.assertEqual(new_core['lnurl']['zap']['payee_pubkeys']['nova'], OTHER)
        self.assertEqual(new_core['core_bridge']['reward_policy']['max_sats_per_feed'], 0)
        self.assertNotIn('payout_peer_uid', new_core['core_bridge'])
        for forbidden in ('rewards', 'payout_bridge'):
            self.assertNotIn(forbidden, new_member)
        self.assertNotIn('signer', new_member['messaging'])
        self.assertFalse(new_member['messaging']['lifecycle'])
        self.assertNotIn('auth', new_member['view'])
        self.assertEqual(core, before[0])
        self.assertEqual(member['core'], before[1]['core'])
        self.assertEqual(s.tomllib.loads(s.W.toml_text(new_core)), new_core)

    def test_render_refuses_existing_bridge_zaps_missing_payees_and_invalid_cutoffs(self):
        for changed in ('bridge', 'zap', 'payee', 'owner', 'socket', 'uid'):
            core, member = configs()
            if changed == 'bridge': core['core_bridge'] = {}
            if changed == 'zap': core['lnurl']['zap'] = {}
            if changed == 'payee': member['core']['payees'].pop('nova')
            if changed == 'owner': core['nostr']['bunker_pubkey'] = OTHER
            if changed == 'socket': member['core_socket'] = '/run/unreviewed.sock'
            if changed == 'uid': member['core']['core_uid'] = 10002
            with self.assertRaises(ValueError): s.render(core, member, 10002, 10003, 0)
        for cutoff in (-1, True, '0'):
            with self.assertRaises(ValueError): s.render(*configs(), 10002, 10003, cutoff)

    def test_partial_false_or_monetary_native_report_cannot_pass(self):
        self.assertEqual(s.check_native_report(ACCEPTED), ACCEPTED)
        for wrong in ({}, dict(ACCEPTED, accepted=False), dict(ACCEPTED, ledger_opened=True),
                      dict(ACCEPTED, provider_operations=False), dict(ACCEPTED, extra=True)):
            with self.assertRaises(ValueError): s.check_native_report(wrong)

    def test_direct_script_refuses_before_substituted_imports(self):
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary)
            shutil.copyfile(SCRIPTS / 'stage-cyberherd-overlay.py', path / 'stage.py')
            (path / 'qualify-cyberherd-installed.py').write_text('raise RuntimeError("POISON")')
            result = subprocess.run([sys.executable, '-I', '-S', '-B', str(path / 'stage.py'), '--help'], capture_output=True, text=True)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn('verified root-owned tool launcher', result.stderr)
            self.assertNotIn('POISON', result.stderr)


@unittest.skipUnless(os.geteuid() == 0, 'protected preparation fixture runs separately as root')
class OverlayStageFiles(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix='lg-overlay-prepare-', dir='/run')
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name); self.root.chmod(0o700)
        self.base = self.root / 'upgrades'; self.base.mkdir(mode=0o700)
        self.etc = self.root / 'etc'; self.etc.mkdir(mode=0o755)
        (self.etc / 'cyberherd').mkdir(mode=0o755)
        (self.etc / 'tmpfiles.d').mkdir(mode=0o755)
        self.core_path = self.etc / 'core.toml'; self.core_path.write_text('ORIGINAL CORE'); self.core_path.chmod(0o600)
        self.member_path = self.etc / 'cyberherd/config.toml'; self.member_path.write_text('ORIGINAL MEMBER'); self.member_path.chmod(0o644)
        self.unit = self.etc / 'core.service'; self.unit.write_text('[Service]\nRuntimeDirectoryMode=0700\n'); self.unit.chmod(0o644)
        self.policy = self.etc / 'policy.toml'; self.policy.write_text('ORIGINAL POLICY'); self.policy.chmod(0o600)
        self.original = {'installation': {'runtime': {'cyberherd': {'uid': 10002, 'gid': 10002}},
                                         'core': {'files': {str(p): tx.fingerprint(p) for p in (self.core_path, self.unit)}}},
                         'maintenance': tx.fingerprint(self.policy)}
        self.destination = self.base / 'prepared'
        for module, name, value in [(s.C, 'BASE', self.base), (s.C, 'CONFIG', self.core_path),
                                     (s.C, 'UNIT', self.unit), (s.Q, 'ETC', self.etc),
                                     (s, 'MAINTENANCE', self.policy), (s, 'TMPFILE', self.etc / 'tmpfiles.d/bridge.conf'),
                                     (s, 'SOCKET', self.root / 'runtime/core.sock')]:
            context = patch.object(module, name, value); context.start(); self.addCleanup(context.stop)
        for module, name, value in [(s.C, 'root', lambda: None), (s.C.grp, 'getgrnam', lambda _: SimpleNamespace(gr_gid=10003)),
                                     (s.C.pwd, 'getpwnam', lambda _: SimpleNamespace(pw_gid=10001)),
                                     (s, 'verified_tool_identity', lambda: {'source': 'fixture'})]:
            context = patch.object(module, name, value); context.start(); self.addCleanup(context.stop)
        self.network = patch.object(s.socket, 'socket'); self.network.start(); self.addCleanup(self.network.stop)
        self.base_call = patch.object(s, 'baseline', side_effect=lambda *args: (copy.deepcopy(self.original), *configs()))
        self.base_call.start(); self.addCleanup(self.base_call.stop)
        self.commands = []
        def command(*args):
            self.commands.append(args)
            return json.dumps(ACCEPTED)
        context = patch.object(s.C, 'command', side_effect=command); self.command = context.start(); self.addCleanup(context.stop)

    def invoke(self):
        return s.prepare(self.root / 'installation', self.root / 'native-validator', '1' * 64, self.destination, 0)

    def test_preparation_freezes_candidates_and_exact_originals_without_installation(self):
        before = {p: tx.fingerprint(p) for p in (self.core_path, self.member_path, self.unit, self.policy)}
        result = self.invoke()
        self.assertTrue(result['prepared']); self.assertFalse(result['activation_supported'])
        self.assertEqual({p: tx.fingerprint(p) for p in before}, before)
        self.assertFalse((self.destination / 'INCOMPLETE').exists())
        metadata = json.loads((self.destination / 'PREPARED.json').read_text())
        self.assertFalse(metadata['state_opened']); self.assertFalse(metadata['services_changed'])
        self.assertEqual(len(self.commands), 1)
        self.assertNotIn('systemctl', str(self.commands))
        self.assertIn('RuntimeDirectoryMode=0700', (self.destination / 'core.service').read_text())
        self.assertIn('SupplementaryGroups=cyberherd-bridge', (self.destination / 'core.service').read_text())
        self.assertFalse(s.TMPFILE.exists())
        self.assertFalse(Path(s.SOCKET).parent.exists())
        with self.assertRaises(FileExistsError): self.invoke()

    def test_native_rejection_retains_incomplete_data_and_original_configuration(self):
        self.command.return_value = None
        self.command.side_effect = lambda *args: json.dumps(dict(ACCEPTED, accepted=False))
        before = self.core_path.read_bytes()
        with self.assertRaises(ValueError): self.invoke()
        self.assertTrue((self.destination / 'INCOMPLETE').exists())
        self.assertFalse((self.destination / 'PREPARED.json').exists())
        self.assertEqual(self.core_path.read_bytes(), before)


if __name__ == '__main__': unittest.main()
