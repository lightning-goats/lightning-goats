from __future__ import annotations

import subprocess
import sys
import tempfile
import unittest
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[2]
NESTED_CHECKER = REPO_ROOT / "deploy" / "scripts" / "check-workflow-nested-shell-safety.py"


class WorkflowNestedEnvWrapperSafetyTests(unittest.TestCase):
    def run_checker(self, workflow: str) -> subprocess.CompletedProcess[str]:
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "workflow.yml"
            path.write_text(workflow, encoding="utf-8")
            return subprocess.run(
                [sys.executable, "-B", str(NESTED_CHECKER), str(path)],
                text=True,
                capture_output=True,
                check=False,
            )

    def test_rejects_repeated_env_options_before_bash_child(self) -> None:
        result = self.run_checker(
            """name: env-repeated-options-bad
jobs:
  test:
    runs-on: ubuntu-latest
    steps:
      - run: env -u FOO -u BAR bash -c 'false | tee result.txt'
"""
        )
        self.assertEqual(result.returncode, 1, result.stderr)

    def test_rejects_env_split_string_child_shell_pipeline(self) -> None:
        result = self.run_checker(
            """name: env-split-string-bad
jobs:
  test:
    runs-on: ubuntu-latest
    steps:
      - run: env -S "bash -c 'false | tee result.txt'"
"""
        )
        self.assertEqual(result.returncode, 1, result.stderr)


if __name__ == "__main__":
    unittest.main()
