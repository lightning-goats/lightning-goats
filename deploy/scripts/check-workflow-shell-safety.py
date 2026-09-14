#!/usr/bin/env python3
"""Reject GitHub Actions shell pipelines that can mask command failures.

GitHub's default bash invocation enables ``-e`` but not ``pipefail``. A workflow
step such as ``real-test | tee evidence.log`` can therefore report success when
the command on the left fails. This checker scans multiline workflow ``run``
blocks and requires pipefail before the first real shell pipeline.

The parser is intentionally narrow: it is not a YAML or shell interpreter. It
understands block indentation, step-local shell overrides, comments, simple
quotes, logical ``||``, and simple heredocs. Ambiguous constructs should be
rewritten explicitly rather than weakening this guard.
"""

from __future__ import annotations

import argparse
import re
import sys
from dataclasses import dataclass
from pathlib import Path


RUN_BLOCK_RE = re.compile(
    r"^(?P<indent>[ \t]*)(?P<dash>-[ \t]+)?run:[ \t]*(?P<style>[|>])"
    r"(?:[-+]|[1-9])?[ \t]*(?:#.*)?$"
)
SHELL_RE = re.compile(
    r"^(?P<indent>[ \t]*)(?:-[ \t]+)?shell:[ \t]*(?P<value>[^#]+?)?"
    r"[ \t]*(?:#.*)?$"
)
PIPEFAIL_ON_RE = re.compile(
    r"\bset\b[^;\n]*?(?:-o[ \t]+pipefail|-[A-Za-z]*o[A-Za-z]*[ \t]+pipefail)\b"
)
PIPEFAIL_OFF_RE = re.compile(r"\bset\b[^;\n]*?\+o[ \t]+pipefail\b")
HEREDOC_RE = re.compile(
    r"<<-?[ \t]*(?P<quote>['\"]?)(?P<word>[A-Za-z_][A-Za-z0-9_]*)\1"
)


@dataclass(frozen=True)
class Finding:
    path: Path
    line: int
    message: str


def _strip_quotes_and_comment(line: str) -> str:
    """Blank simple quoted/comment text while preserving shell operators."""
    out: list[str] = []
    quote: str | None = None
    escaped = False
    for char in line:
        if escaped:
            if quote != "'":
                out.append(" ")
            escaped = False
            continue
        if quote == "'":
            if char == "'":
                quote = None
            out.append(" ")
            continue
        if quote == '"':
            if char == "\\":
                escaped = True
            elif char == '"':
                quote = None
            out.append(" ")
            continue
        if char == "\\":
            escaped = True
            out.append(" ")
        elif char in ("'", '"'):
            quote = char
            out.append(" ")
        elif char == "#":
            break
        else:
            out.append(char)
    return "".join(out)


def _pipeline_index(cleaned: str) -> int | None:
    for idx, char in enumerate(cleaned):
        if char != "|":
            continue
        prev_char = cleaned[idx - 1] if idx else ""
        next_char = cleaned[idx + 1] if idx + 1 < len(cleaned) else ""
        if prev_char == "|" or next_char == "|":
            continue
        return idx
    return None


def _shell_for_run(
    lines: list[str], run_index: int, run_indent: int, run_has_dash: bool
) -> str | None:
    """Return a step-local shell override, or None for the workflow default."""
    if run_has_dash:
        return None

    step_indent = max(run_indent - 2, 0)
    for idx in range(run_index - 1, -1, -1):
        raw = lines[idx]
        if not raw.strip():
            continue
        indent = len(raw) - len(raw.lstrip(" \t"))
        if indent < step_indent:
            break

        match = SHELL_RE.match(raw)
        if match and len(match.group("indent")) in {run_indent, step_indent}:
            return (match.group("value") or "").strip()

        # The first list-item marker is the start of this step. Do not cross it
        # into a preceding step looking for a shell override.
        if indent == step_indent and raw.lstrip().startswith("- "):
            break

    return None


def _is_shell_like(shell: str | None) -> bool:
    if shell is None:
        return True
    executable = shell.split()[0].lower()
    return executable.endswith("bash") or executable.endswith("sh")


def scan_workflow(path: Path) -> list[Finding]:
    text = path.read_text(encoding="utf-8")
    lines = text.splitlines()
    findings: list[Finding] = []

    idx = 0
    while idx < len(lines):
        match = RUN_BLOCK_RE.match(lines[idx])
        if not match:
            idx += 1
            continue

        run_indent = len(match.group("indent"))
        shell = _shell_for_run(
            lines, idx, run_indent, run_has_dash=match.group("dash") is not None
        )
        idx += 1
        block_start = idx

        while idx < len(lines):
            raw = lines[idx]
            if not raw.strip():
                idx += 1
                continue
            indent = len(raw) - len(raw.lstrip(" \t"))
            if indent <= run_indent:
                break
            idx += 1

        if not _is_shell_like(shell):
            continue

        block_lines = lines[block_start:idx]
        nonblank = [line for line in block_lines if line.strip()]
        if not nonblank:
            continue
        content_indent = min(
            len(line) - len(line.lstrip(" \t")) for line in nonblank
        )

        pipefail_enabled = False
        heredoc_end: str | None = None

        for offset, raw in enumerate(block_lines):
            command = raw[content_indent:] if len(raw) >= content_indent else raw
            stripped = command.strip()

            if heredoc_end is not None:
                if stripped == heredoc_end:
                    heredoc_end = None
                continue
            if not stripped or stripped.startswith("#"):
                continue

            cleaned = _strip_quotes_and_comment(command)
            pipeline = _pipeline_index(cleaned)

            on_match = PIPEFAIL_ON_RE.search(cleaned)
            off_match = PIPEFAIL_OFF_RE.search(cleaned)
            if off_match and (on_match is None or off_match.start() < on_match.start()):
                pipefail_enabled = False

            enabled_before_pipeline = pipefail_enabled or (
                on_match is not None
                and (pipeline is None or on_match.end() <= pipeline)
            )

            if pipeline is not None and not enabled_before_pipeline:
                findings.append(
                    Finding(
                        path=path,
                        line=block_start + offset + 1,
                        message="shell pipeline executes before `set -o pipefail`",
                    )
                )
                break

            if on_match is not None:
                pipefail_enabled = True
            if off_match and (
                on_match is None or off_match.start() > on_match.start()
            ):
                pipefail_enabled = False

            # Match the heredoc marker on the original command: the quote scrubber
            # intentionally removes quoted delimiters such as <<'PY'.
            heredoc_match = HEREDOC_RE.search(command)
            if heredoc_match:
                heredoc_end = heredoc_match.group("word")

    return findings


def workflow_paths(targets: list[Path]) -> list[Path]:
    paths: set[Path] = set()
    for target in targets:
        if target.is_dir():
            paths.update(target.glob("*.yml"))
            paths.update(target.glob("*.yaml"))
        elif target.suffix in {".yml", ".yaml"}:
            paths.add(target)
        else:
            raise ValueError(f"not a workflow YAML file or directory: {target}")
    return sorted(paths)


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
        paths = workflow_paths(args.targets)
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
