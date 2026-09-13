"""Launch boundary tests; root cases touch only disposable /run files."""
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

SCRIPTS = Path(__file__).resolve().parents[1] / 'scripts'
SPEC = importlib.util.spec_from_file_location('tool_launch', SCRIPTS / 'launch-vps-upgrade.py')
LAUNCH = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(LAUNCH)


class ToolLaunchTests(unittest.TestCase):
    def test_direct_coordinator_refuses_before_substituted_helper(self):
        with tempfile.TemporaryDirectory() as d:
            root = Path(d)
            shutil.copyfile(SCRIPTS / 'upgrade-vps-canary.py', root / 'upgrade-vps-canary.py')
            for name in ['inactive_file_transaction.py', 'argparse.py']:
                (root / name).write_text('raise RuntimeError("INJECTED")')
            result = subprocess.run(['/usr/bin/python3', '-B', str(root / 'upgrade-vps-canary.py'), 'snapshot'], capture_output=True, text=True)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn('verified root-owned tool launcher', result.stderr)
            self.assertNotIn('INJECTED', result.stderr)

    @unittest.skipUnless(os.geteuid() == 0, 'root launch fixture runs separately')
    def test_authenticated_launch_and_substitution_boundaries(self):
        with tempfile.TemporaryDirectory(prefix='lg-tool-fixture-', dir='/run') as d:
            root = Path(d)
            bundle = root / 'bundle'
            bundle.mkdir(mode=0o700)
            for name in LAUNCH.FILES:
                shutil.copyfile(SCRIPTS / name, bundle / name)
                (bundle / name).chmod(0o600)
            manifest = {'version': 1, 'source_commit': 'a' * 40, 'files': {
                name: hashlib.sha256((bundle / name).read_bytes()).hexdigest() for name in LAUNCH.FILES}}
            path = bundle / 'TOOL.json'
            path.write_text(json.dumps(manifest, sort_keys=True))
            digest = hashlib.sha256(path.read_bytes()).hexdigest()
            poison = root / 'poison'
            poison.mkdir()
            marker = root / 'executed'
            payload = f'open({str(marker)!r},"w").write("BAD")\nraise RuntimeError("INJECTED")\n'
            for name in ['sitecustomize.py', 'usercustomize.py', 'json.py', 'inactive_file_transaction.py']:
                (poison / name).write_text(payload)
            command = ['/usr/bin/python3', '-I', '-S', '-B', str(bundle / 'launch-vps-upgrade.py'), digest, '--help']
            def run(ok=False):
                result = subprocess.run(command, cwd=poison, env=dict(os.environ, PYTHONPATH=str(poison)), capture_output=True, text=True, timeout=10)
                self.assertEqual(result.returncode == 0, ok, result.stderr)
                self.assertFalse(marker.exists())
                return result
            self.assertIn('snapshot', run(ok=True).stdout)
            for name in LAUNCH.FILES - {'launch-vps-upgrade.py'}:
                target = bundle / name
                original = target.read_bytes()
                target.write_text(payload)
                self.assertIn('tool payload drift', run().stderr)
                target.write_bytes(original)
                target.chmod(0o622)
                self.assertIn('untrusted tool path', run().stderr)
                target.chmod(0o600)
                target.unlink()
                target.symlink_to(poison / 'json.py')
                self.assertIn('untrusted tool path', run().stderr)
                target.unlink()
                target.write_bytes(original)
                target.chmod(0o600)
            cache = bundle / '__pycache__'
            cache.mkdir()
            self.assertIn('unexpected tool bundle contents', run().stderr)
            cache.rmdir()
            command[5] = '0' * 64
            self.assertIn('tool manifest digest mismatch', run().stderr)
            command[5] = digest
            run(ok=True)
