#!/usr/bin/env python3
"""Conservatively reject unsafe pipelines in YAML folded workflow run blocks.

YAML ``run: >`` folds adjacent base-indented physical lines into spaces before
GitHub Actions passes the resulting command to the shell. Therefore a source
layout like::

    run: >
      set -o pipefail
      false | tee result.txt

executes as ``set -o pipefail false | tee result.txt``; the first source line
does not establish parent-shell pipefail. This guard complements
``check-workflow-shell-safety.py`` by reconstructing folded paragraphs before
checking pipeline safety. Blank or more-indented boundaries are treated
conservatively: each resulting paragraph containing a pipeline must establish
its own pipefail state. Use a literal ``run: |`` block for more complex shell
control flow.
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
sys.modules[_SPEC.name] = _BASE
_SPEC.loader.exec_module(_BASE)


def _folded_paragraphs(
    block_lines: list[str], content_indent: int
) -> list[tuple[int, str]]:
    """Approximate YAML folded paragraphs without adding a YAML dependency.

    Consecutive nonblank lines at the block's base content indentation fold to
    spaces. Blank lines and more-indented content preserve a line boundary, so
    they terminate the current paragraph. Treating each preserved boundary as a
    fresh safety proof is intentionally conservative and cannot turn an unsafe
    folded pipeline into an accepted one.
    """
    paragraphs: list[tuple[int, str]] = []
    parts: list[str] = []
    start_offset = 0

    def flush() -> None:
        nonlocal parts
        if parts:
            paragraphs.append((start_offset, " ".join(parts)))
            parts = []

    for offset, raw in enumerate(block_lines):
        if not raw.strip():
            flush()
            continue

        command = raw[content_indent:] if len(raw) >= content_indent else raw
        relative_indent = len(command) - len(command.lstrip(" \t"))
        if relative_indent:
            flush()
            paragraphs.append((offset, command.strip()))
            continue

        if not parts:
            start_offset = offset
        parts.append(command.strip())

    flush()
    return paragraphs


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

        for offset, command in _folded_paragraphs(block_lines, content_indent):
            if _BASE._single_line_pipeline_is_unsafe(command):
                findings.append(
                    _BASE.Finding(
                        path=path,
                        line=block_start + offset + 1,
                        message=(
                            "folded shell pipeline lacks local "
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
