#!/usr/bin/env python3
"""Conservatively reject unsafe pipelines in YAML folded workflow run blocks.

YAML ``run: >`` folds adjacent physical lines into spaces before GitHub Actions
passes the resulting command to the shell. Therefore a source layout like::

    run: >
      set -o pipefail
      false | tee result.txt

executes as ``set -o pipefail false | tee result.txt``; the first source line
does not establish parent-shell pipefail. This guard complements
``check-workflow-shell-safety.py`` by requiring any real pipeline in a folded
shell block to establish pipefail on that same physical source line. For more
complex command structure, use a literal ``run: |`` block instead.
"""

from __future__ import annotations

import argparse
import importlib.util
import sys
from pathlib import Path


BASE_CHECKER = Path(__file__).with_name("check-workflow-shell-safety.py")
_SPEC = importlib.util.spec_from_file_location("workflow_shell_safety", BASE_CHECKER)
if _SPEC is None or _SPEC.loader is None:
    raise RuntimeError(f"cannot load workflow shell checker: {BASE_CHECKER}")
_BASE = importlib.util.module_from_spec(_SPEC)
_SPEC.loader.exec_module(_BASE)


def scan_workflow(path: Path) -> list[object]:
    lines = path.read_text(encoding="utf-8").splitlines()
    findings: list[object] = []
    idx = 0

    while idx < len(lines):
        match = _BASE.RUN_BLOCK_RE.match(lines[idx])
        if not match or match.group("style") != ">":
            idx += 1
            continue

        run_indent = len(match.group("indent"))
        run_has_dash = match.group("dash") is not None
        run_key_indent = run_indent + 2 if run_has_dash else run_indent
        shell = _BASE._shell_for_run(
            lines, idx, run_indent, run_has_dash=run_has_dash
        )
        idx += 1
        block_start = idx

        while idx < len(lines):
            raw = lines[idx]
            if not raw.strip():
                idx += 1
                continue
            indent = len(raw) - len(raw.lstrip(" \t"))
            if indent <= run_key_indent:
                break
            idx += 1

        if not _BASE._is_shell_like(shell):
            continue

        block_lines = lines[block_start:idx]
        nonblank = [line for line in block_lines if line.strip()]
        if not nonblank:
            continue
        content_indent = min(
            len(line) - len(line.lstrip(" \t")) for line in nonblank
        )

        for offset, raw in enumerate(block_lines):
            if not raw.strip():
                continue
            command = raw[content_indent:] if len(raw) >= content_indent else raw
            if _BASE._single_line_pipeline_is_unsafe(command):
                findings.append(
                    _BASE.Finding(
                        path=path,
                        line=block_start + offset + 1,
                        message=(
                            "folded shell pipeline lacks same-line "
                            "`set -o pipefail` before execution"
                        ),
                    )
                )
                break

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
