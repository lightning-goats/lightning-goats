#!/usr/bin/env python3
"""Reject YAML anchors/aliases in executable GitHub Actions step fields.

GitHub Actions supports YAML anchors and aliases. The repository's shell-safety
checks intentionally use a small raw-text parser rather than a YAML dependency,
so executable fields that begin with ``&anchor`` or ``*alias`` are ambiguous to
those checks. Reject that indirection in ``run:`` and ``shell:`` while allowing
anchors/aliases in non-executable workflow fields.

Literal block-scalar contents are skipped so shell text that merely contains a
line such as ``run: *example`` is not mistaken for workflow YAML.
"""

from __future__ import annotations

import argparse
import importlib.util
import re
import sys
from pathlib import Path


BASE_CHECKER = Path(__file__).with_name("check-workflow-shell-safety.py")
_SPEC = importlib.util.spec_from_file_location("workflow_shell_safety_alias_base", BASE_CHECKER)
if _SPEC is None or _SPEC.loader is None:
    raise RuntimeError(f"cannot load workflow shell checker: {BASE_CHECKER}")
_BASE = importlib.util.module_from_spec(_SPEC)
sys.modules[_SPEC.name] = _BASE
_SPEC.loader.exec_module(_BASE)

RUN_REFERENCE_RE = re.compile(
    r"^(?P<indent>[ \t]*)(?P<dash>-[ \t]+)?run:[ \t]+"
    r"(?P<kind>[*&])(?P<name>[^ \t#|>]+)(?:[ \t]+.*)?$"
)
SHELL_REFERENCE_RE = re.compile(
    r"^(?P<indent>[ \t]*)(?P<dash>-[ \t]+)?shell:[ \t]+"
    r"(?P<kind>[*&])(?P<name>[^ \t#]+)(?:[ \t]+.*)?$"
)


def _reference_finding(path: Path, line: int, field: str, kind: str) -> object:
    label = "alias" if kind == "*" else "anchor"
    action = "inspected" if field == "run" else "classified"
    literal = "command" if field == "run" else "shell"
    return _BASE.Finding(
        path=path,
        line=line,
        message=(
            f"YAML {label} in `{field}:` cannot be statically {action}; "
            f"use a literal {literal}"
        ),
    )


def scan_workflow(path: Path) -> list[object]:
    lines = path.read_text(encoding="utf-8").splitlines()
    findings: list[object] = []
    idx = 0

    while idx < len(lines):
        raw = lines[idx]
        block_match = _BASE.RUN_BLOCK_RE.match(raw)
        if block_match is not None:
            run_indent = len(block_match.group("indent"))
            run_has_dash = block_match.group("dash") is not None
            run_key_indent = run_indent + 2 if run_has_dash else run_indent
            idx += 1
            while idx < len(lines):
                block_raw = lines[idx]
                if not block_raw.strip():
                    idx += 1
                    continue
                indent = len(block_raw) - len(block_raw.lstrip(" \t"))
                if indent <= run_key_indent:
                    break
                idx += 1
            continue

        run_reference = RUN_REFERENCE_RE.match(raw)
        if run_reference is not None:
            findings.append(
                _reference_finding(
                    path, idx + 1, "run", run_reference.group("kind")
                )
            )

        shell_reference = SHELL_REFERENCE_RE.match(raw)
        if shell_reference is not None:
            findings.append(
                _reference_finding(
                    path, idx + 1, "shell", shell_reference.group("kind")
                )
            )

        idx += 1

    return findings


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "targets",
        nargs="*",
        type=Path,
        default=[Path(".github/workflows")],
        help="workflow YAML files or directories (default: .github/workflows)",
    )
    args = parser.parse_args(argv)
    try:
        paths = _BASE.workflow_paths(args.targets)
    except ValueError as exc:
        parser.error(str(exc))
    if not paths:
        print("no workflow YAML files found", file=sys.stderr)
        return 2
    findings = [finding for path in paths for finding in scan_workflow(path)]
    for finding in findings:
        print(f"{finding.path}:{finding.line}: {finding.message}", file=sys.stderr)
    return 1 if findings else 0


if __name__ == "__main__":
    raise SystemExit(main())
