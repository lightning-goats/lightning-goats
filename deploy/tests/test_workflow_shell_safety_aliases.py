from __future__ import annotations

import subprocess
import sys
import tempfile
import unittest
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[2]
CHECKER = REPO_ROOT / "deploy" / "scripts" / "check-workflow-shell-safety.py"


class WorkflowShellSafetyAliasTests(unittest.TestCase):
    def run_checker(self, workflow: str) -> subprocess.CompletedProcess[str]:
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "workflow.yml"
            path.write_text(workflow, encoding="utf-8")
            return subprocess.run(
                [sys.executable, "-B", str(CHECKER), str(path)],
                text=True,
                capture_output=True,
                check=False,
            )

    def test_rejects_run_scalar_yaml_alias(self) -> None:
        result = self.run_checker(
            """name: aliased-run

env:
  PIPE_COMMAND: &unsafe_run false | tee result.txt

jobs:
  test:
    runs-on: ubuntu-latest
    steps:
      - run: *unsafe_run
"""
        )
        self.assertEqual(result.returncode, 1, result.stderr)
        self.assertIn("YAML alias", result.stderr)

    def test_rejects_shell_yaml_alias_before_pipeline(self) -> None:
        result = self.run_checker(
            """name: aliased-shell

env:
  SHELL_KIND: &bash_shell bash

jobs:
  test:
    runs-on: ubuntu-latest
    steps:
      - run: false | tee result.txt
        shell: *bash_shell
"""
        )
        self.assertEqual(result.returncode, 1, result.stderr)
        self.assertIn("YAML alias", result.stderr)

    def test_quoted_asterisk_is_not_a_yaml_alias(self) -> None:
        result = self.run_checker(
            """name: quoted-asterisk
jobs:
  test:
    runs-on: ubuntu-latest
    steps:
      - run: printf '%s\\n' '*not-an-alias'
"""
        )
        self.assertEqual(result.returncode, 0, result.stderr)


if __name__ == "__main__":
    unittest.main()
