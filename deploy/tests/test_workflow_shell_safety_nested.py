from __future__ import annotations

import subprocess
import sys
import tempfile
import unittest
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[2]
BASE_CHECKER = REPO_ROOT / "deploy" / "scripts" / "check-workflow-shell-safety.py"
NESTED_CHECKER = REPO_ROOT / "deploy" / "scripts" / "check-workflow-nested-shell-safety.py"


class WorkflowNestedShellSafetyTests(unittest.TestCase):
    def run_checker(
        self, checker: Path, workflow: str
    ) -> subprocess.CompletedProcess[str]:
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "workflow.yml"
            path.write_text(workflow, encoding="utf-8")
            return subprocess.run(
                [sys.executable, "-B", str(checker), str(path)],
                text=True,
                capture_output=True,
                check=False,
            )

    def test_existing_base_guard_demonstrates_nested_child_blind_spot(self) -> None:
        workflow = """name: nested-baseline
jobs:
  test:
    runs-on: ubuntu-latest
    steps:
      - run: bash -c 'false | tee result.txt'
"""
        result = self.run_checker(BASE_CHECKER, workflow)
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_rejects_bash_c_pipeline_without_child_pipefail(self) -> None:
        result = self.run_checker(
            NESTED_CHECKER,
            """name: nested-bad
jobs:
  test:
    runs-on: ubuntu-latest
    steps:
      - run: bash -c 'false | tee result.txt'
""",
        )
        self.assertEqual(result.returncode, 1, result.stderr)
        self.assertIn("child shell pipeline", result.stderr)

    def test_parent_pipefail_does_not_protect_child_shell(self) -> None:
        result = self.run_checker(
            NESTED_CHECKER,
            """name: parent-only
jobs:
  test:
    runs-on: ubuntu-latest
    steps:
      - run: |
          set -o pipefail
          bash -c 'false | tee result.txt'
""",
        )
        self.assertEqual(result.returncode, 1, result.stderr)

    def test_accepts_bash_c_with_local_pipefail(self) -> None:
        result = self.run_checker(
            NESTED_CHECKER,
            """name: child-local-good
jobs:
  test:
    runs-on: ubuntu-latest
    steps:
      - run: bash -c 'set -o pipefail; false | tee result.txt'
""",
        )
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_accepts_bash_invocation_with_pipefail_option(self) -> None:
        result = self.run_checker(
            NESTED_CHECKER,
            """name: child-option-good
jobs:
  test:
    runs-on: ubuntu-latest
    steps:
      - run: bash -o pipefail -c 'false | tee result.txt'
""",
        )
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_rejects_bash_plus_o_pipefail_before_c(self) -> None:
        result = self.run_checker(
            NESTED_CHECKER,
            """name: child-plus-o-bad
jobs:
  test:
    runs-on: ubuntu-latest
    steps:
      - run: bash +o pipefail -c 'false | tee result.txt'
""",
        )
        self.assertEqual(result.returncode, 1, result.stderr)

    def test_rejects_bash_plus_upper_o_option_before_c(self) -> None:
        result = self.run_checker(
            NESTED_CHECKER,
            """name: child-plus-upper-o-bad
jobs:
  test:
    runs-on: ubuntu-latest
    steps:
      - run: bash +O extglob -c 'false | tee result.txt'
""",
        )
        self.assertEqual(result.returncode, 1, result.stderr)

    def test_last_plus_o_pipefail_disables_invocation_protection(self) -> None:
        result = self.run_checker(
            NESTED_CHECKER,
            """name: child-last-disable-bad
jobs:
  test:
    runs-on: ubuntu-latest
    steps:
      - run: bash -o pipefail +o pipefail -c 'false | tee result.txt'
""",
        )
        self.assertEqual(result.returncode, 1, result.stderr)

    def test_last_minus_o_pipefail_reenables_invocation_protection(self) -> None:
        result = self.run_checker(
            NESTED_CHECKER,
            """name: child-last-enable-good
jobs:
  test:
    runs-on: ubuntu-latest
    steps:
      - run: bash +o pipefail -o pipefail -c 'false | tee result.txt'
""",
        )
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_rejects_combined_bash_c_option_without_pipefail(self) -> None:
        result = self.run_checker(
            NESTED_CHECKER,
            """name: child-lc-bad
jobs:
  test:
    runs-on: ubuntu-latest
    steps:
      - run: bash -lc 'false | tee result.txt'
""",
        )
        self.assertEqual(result.returncode, 1, result.stderr)

    def test_rejects_sh_c_pipeline_conservatively(self) -> None:
        result = self.run_checker(
            NESTED_CHECKER,
            """name: sh-child-bad
jobs:
  test:
    runs-on: ubuntu-latest
    steps:
      - run: sh -c 'false | tee result.txt'
""",
        )
        self.assertEqual(result.returncode, 1, result.stderr)

    def test_rejects_env_wrapped_bash_child(self) -> None:
        result = self.run_checker(
            NESTED_CHECKER,
            """name: env-child-bad
jobs:
  test:
    runs-on: ubuntu-latest
    steps:
      - run: env EXAMPLE=1 bash -c 'false | tee result.txt'
""",
        )
        self.assertEqual(result.returncode, 1, result.stderr)

    def test_quoted_bash_text_is_not_an_invocation(self) -> None:
        result = self.run_checker(
            NESTED_CHECKER,
            """name: quoted-data
jobs:
  test:
    runs-on: ubuntu-latest
    steps:
      - run: printf '%s\\n' "bash -c 'false | tee result.txt'"
""",
        )
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_child_without_pipeline_is_allowed(self) -> None:
        result = self.run_checker(
            NESTED_CHECKER,
            """name: no-child-pipeline
jobs:
  test:
    runs-on: ubuntu-latest
    steps:
      - run: bash -c 'printf ok'
""",
        )
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_folded_child_invocation_uses_executed_form(self) -> None:
        result = self.run_checker(
            NESTED_CHECKER,
            """name: folded-child
jobs:
  test:
    runs-on: ubuntu-latest
    steps:
      - run: >
          bash -c
          'false | tee result.txt'
""",
        )
        self.assertEqual(result.returncode, 1, result.stderr)

    def test_explicit_non_shell_parent_is_outside_guard(self) -> None:
        result = self.run_checker(
            NESTED_CHECKER,
            """name: python-parent
jobs:
  test:
    runs-on: ubuntu-latest
    steps:
      - shell: python
        run: |
          value = "bash -c 'false | tee result.txt'"
          print(value)
""",
        )
        self.assertEqual(result.returncode, 0, result.stderr)


if __name__ == "__main__":
    unittest.main()
