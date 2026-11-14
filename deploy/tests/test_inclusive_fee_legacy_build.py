"""Legacy verification build is inert outside unprivileged disposable CI."""
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

SCRIPT=Path(__file__).resolve().parents[1]/'scripts/build-inclusive-fee-legacy-verifier.sh'

class LegacyBuildBoundary(unittest.TestCase):
    def test_ordinary_environment_refuses_before_creating_output_or_fetching(self):
        with tempfile.TemporaryDirectory() as directory:
            output=Path(directory)/'uncreated'
            env={'PATH':'/usr/bin:/bin','LC_ALL':'C'}
            result=subprocess.run(['/bin/bash',str(SCRIPT),str(output)],env=env,capture_output=True,text=True,timeout=10)
            self.assertNotEqual(result.returncode,0)
            self.assertIn('unprivileged disposable Linux CI',result.stderr)
            self.assertFalse(output.exists())
