from __future__ import annotations

import subprocess
import sys
import tempfile
import unittest
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[2]
CHECKER = REPO_ROOT / "deploy" / "scripts" / "check-workflow-folded-shell-safety.py"


class WorkflowShellSafetyFoldedTests(unittest.TestCase):
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

    def test_rejects_folded_pipefail_on_prior_physical_line(self) -> None:
        result = self.run_checker(
            """name: folded-bad
jobs:
  test:
    runs-on: ubuntu-latest
    steps:
      - run: >
          set -o pipefail
          false | tee result.txt
"""
        )
        self.assertEqual(result.returncode, 1, result.stderr)
        self.assertIn("folded shell pipeline", result.stderr)

    def test_accepts_folded_same_line_pipefail_before_pipeline(self) -> None:
        result = self.run_checker(
            """name: folded-same-line-good
jobs:
  test:
    runs-on: ubuntu-latest
    steps:
      - run: >
          set -o pipefail; false | tee result.txt
"""
        )
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_folded_quoted_pipe_is_not_a_pipeline(self) -> None:
        result = self.run_checker(
            """name: folded-quoted-pipe
jobs:
  test:
    runs-on: ubuntu-latest
    steps:
      - run: >
          printf '%s\\n' 'left|right'
"""
        )
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_ignores_folded_explicit_non_shell_override(self) -> None:
        result = self.run_checker(
            """name: folded-python
jobs:
  test:
    runs-on: ubuntu-latest
    steps:
      - shell: python
        run: >
          value = 1 | 2
          print(value)
"""
        )
        self.assertEqual(result.returncode, 0, result.stderr)


if __name__ == "__main__":
    unittest.main()
