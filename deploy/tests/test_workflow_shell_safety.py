from __future__ import annotations

import subprocess
import sys
import tempfile
import unittest
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[2]
CHECKER = REPO_ROOT / "deploy" / "scripts" / "check-workflow-shell-safety.py"


class WorkflowShellSafetyTests(unittest.TestCase):
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

    def test_rejects_pipeline_without_pipefail(self) -> None:
        result = self.run_checker(
            """name: bad
jobs:
  test:
    runs-on: ubuntu-latest
    steps:
      - shell: bash
        run: |
          set -eu
          python3 test.py | tee result.txt
"""
        )
        self.assertEqual(result.returncode, 1)
        self.assertIn("pipeline executes before", result.stderr)

    def test_rejects_single_line_pipeline_without_pipefail(self) -> None:
        result = self.run_checker(
            """name: single-line-bad
jobs:
  test:
    runs-on: ubuntu-latest
    steps:
      - run: false | tee result.txt
"""
        )
        self.assertEqual(result.returncode, 1)
        self.assertIn("pipeline executes before", result.stderr)

    def test_accepts_pipefail_before_pipeline(self) -> None:
        result = self.run_checker(
            """name: good
jobs:
  test:
    runs-on: ubuntu-latest
    steps:
      - shell: bash
        run: |
          set -euo pipefail
          python3 test.py | tee result.txt
"""
        )
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_accepts_pipefail_on_same_line_before_pipeline(self) -> None:
        result = self.run_checker(
            """name: same-line
jobs:
  test:
    runs-on: ubuntu-latest
    steps:
      - run: |
          set -o pipefail; false | tee result.txt
"""
        )
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_accepts_initial_set_prologue_before_pipeline(self) -> None:
        result = self.run_checker(
            """name: set-prologue
jobs:
  test:
    runs-on: ubuntu-latest
    steps:
      - run: |
          set -eu
          set -o pipefail
          false | tee result.txt
"""
        )
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_rejects_pipefail_disabled_later_on_same_line(self) -> None:
        result = self.run_checker(
            """name: same-line-disable
jobs:
  test:
    runs-on: ubuntu-latest
    steps:
      - run: |
          set -o pipefail; set +o pipefail
          false | tee result.txt
"""
        )
        self.assertEqual(result.returncode, 1)

    def test_rejects_subshell_pipefail_that_does_not_enable_parent(self) -> None:
        result = self.run_checker(
            """name: subshell-bypass
jobs:
  test:
    runs-on: ubuntu-latest
    steps:
      - run: |
          (set -o pipefail)
          false | tee result.txt
"""
        )
        self.assertEqual(result.returncode, 1)

    def test_rejects_short_circuited_pipefail_that_never_executes(self) -> None:
        result = self.run_checker(
            """name: short-circuit-bypass
jobs:
  test:
    runs-on: ubuntu-latest
    steps:
      - run: |
          false && set -o pipefail
          false | tee result.txt
"""
        )
        self.assertEqual(result.returncode, 1)

    def test_rejects_uninvoked_function_pipefail(self) -> None:
        result = self.run_checker(
            """name: function-bypass
jobs:
  test:
    runs-on: ubuntu-latest
    steps:
      - run: |
          enable_pipefail() {
            set -o pipefail
          }
          false | tee result.txt
"""
        )
        self.assertEqual(result.returncode, 1)

    def test_rejects_pipefail_enabled_after_pipeline(self) -> None:
        result = self.run_checker(
            """name: too-late
jobs:
  test:
    runs-on: ubuntu-latest
    steps:
      - run: |
          false | tee result.txt
          set -o pipefail
"""
        )
        self.assertEqual(result.returncode, 1)

    def test_ignores_quotes_comments_logical_or_and_heredoc_body(self) -> None:
        result = self.run_checker(
            """name: syntax-noise
jobs:
  test:
    runs-on: ubuntu-latest
    steps:
      - run: |
          printf '%s\\n' 'left|right'
          false || true
          # not | a pipeline
          python3 - <<'PY'
          value = 1 | 2
          print(value)
          PY
"""
        )
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_ignores_explicit_non_shell_run_block(self) -> None:
        for shell in ("python", "pwsh"):
            with self.subTest(shell=shell):
                result = self.run_checker(
                    f"""name: non-shell
jobs:
  test:
    runs-on: ubuntu-latest
    steps:
      - shell: {shell}
        run: |
          value = 1 | 2
          print(value)
"""
                )
                self.assertEqual(result.returncode, 0, result.stderr)

    def test_rejects_unsafe_pipeline_with_quoted_bash_shell(self) -> None:
        for shell in ("'bash'", '"bash"', "'/bin/bash'", '"bash -e {0}"'):
            with self.subTest(shell=shell):
                result = self.run_checker(
                    f"""name: quoted-shell
jobs:
  test:
    runs-on: ubuntu-latest
    steps:
      - shell: {shell}
        run: |
          false | tee result.txt
"""
                )
                self.assertEqual(result.returncode, 1, result.stderr)

    def test_quoted_non_shell_remains_ignored(self) -> None:
        for shell in ("'python'", '"pwsh"'):
            with self.subTest(shell=shell):
                result = self.run_checker(
                    f"""name: quoted-non-shell
jobs:
  test:
    runs-on: ubuntu-latest
    steps:
      - shell: {shell}
        run: |
          value = 1 | 2
          print(value)
"""
                )
                self.assertEqual(result.returncode, 0, result.stderr)

    def test_does_not_inherit_shell_from_previous_step(self) -> None:
        result = self.run_checker(
            """name: separate-steps
jobs:
  test:
    runs-on: ubuntu-latest
    steps:
      - shell: python
        run: |
          print('previous')
      - run: |
          false | tee result.txt
"""
        )
        self.assertEqual(result.returncode, 1)

    def test_run_first_key_does_not_consume_following_step_metadata(self) -> None:
        result = self.run_checker(
            """name: run-first
jobs:
  test:
    runs-on: ubuntu-latest
    steps:
      - run: |
          echo safe
        env:
          EXAMPLE: left|right
"""
        )
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_current_repository_workflows_are_safe(self) -> None:
        result = subprocess.run(
            [
                sys.executable,
                "-B",
                str(CHECKER),
                str(REPO_ROOT / ".github" / "workflows"),
            ],
            text=True,
            capture_output=True,
            check=False,
        )
        self.assertEqual(result.returncode, 0, result.stderr)


if __name__ == "__main__":
    unittest.main()
