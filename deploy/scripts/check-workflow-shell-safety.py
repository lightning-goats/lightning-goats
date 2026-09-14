#!/usr/bin/env python3
"""Reject GitHub Actions shell pipelines that can mask command failures.

GitHub's default bash invocation enables ``-e`` but not ``pipefail``. A workflow
step such as ``real-test | tee evidence.log`` can therefore report success when
the command on the left fails. This checker scans multiline workflow ``run``
blocks and requires pipefail before the first real shell pipeline.

The parser is intentionally narrow: it is not a YAML or shell interpreter. It
understands block indentation, step-local shell overrides, comments, simple
quotes, logical ``||``, simple heredocs, and an initial prologue of direct ``set``
builtins. A ``set -o pipefail`` hidden in a subshell, conditional, function, or
other control-flow construct does not establish parent-shell state. Ambiguous
constructs should be rewritten explicitly rather than weakening this guard.
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
DIRECT_SET_RE = re.compile(
    r"^[ \t]*set\b(?P<args>[^;|&\n]*)(?P<rest>.*)$"
)
PIPEFAIL_ON_ARGS_RE = re.compile(
    r"(?:^|[ \t])(?:-o[ \t]+pipefail|-[A-Za-z]*o[A-Za-z]*[ \t]+pipefail)"
    r"(?:[ \t]|$)"
)
PIPEFAIL_OFF_ARGS_RE = re.compile(
    r"(?:^|[ \t])\+o[ \t]+pipefail(?:[ \t]|$)"
)
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
    return Path(executable).name in {"bash", "sh"}


def _set_state(cleaned: str) -> tuple[bool, bool, str] | None:
    """Return direct-set pipefail on/off flags and the suffix after the set command.

    Only a direct ``set`` builtin at the start of a cleaned command qualifies.
    The argument slice stops before shell control operators so constructs such as
    ``(set -o pipefail)`` and ``false && set -o pipefail`` never masquerade as
    parent-shell state changes.
    """
    match = DIRECT_SET_RE.match(cleaned)
    if not match:
        return None
    args = match.group("args")
    return (
        PIPEFAIL_ON_ARGS_RE.search(args) is not None,
        PIPEFAIL_OFF_ARGS_RE.search(args) is not None,
        match.group("rest"),
    )


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
        run_has_dash = match.group("dash") is not None
        run_key_indent = run_indent + 2 if run_has_dash else run_indent
        shell = _shell_for_run(
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
        set_prologue = True
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
            set_state = _set_state(cleaned)

            # Only an initial sequence of direct `set` builtins can establish
            # pipefail. Once ordinary/control-flow shell code begins, a later
            # textual `set -o pipefail` could be conditional, nested or
            # unexecuted, so it cannot prove parent-shell state to this parser.
            on = off = False
            rest = ""
            if set_state is not None:
                on, off, rest = set_state

            semicolon = cleaned.find(";")
            set_completes_before_pipeline = (
                pipeline is None
                or (semicolon != -1 and semicolon < pipeline)
            )
            can_enable_here = (
                set_prologue
                and set_state is not None
                and on
                and not off
                and set_completes_before_pipeline
            )

            if off and set_state is not None:
                # Conservatively honor a direct-looking disable even after the
                # prologue. A nested false positive is safer than masking a
                # pipeline failure.
                pipefail_enabled = False

            enabled_before_pipeline = pipefail_enabled or can_enable_here
            if pipeline is not None and not enabled_before_pipeline:
                findings.append(
                    Finding(
                        path=path,
                        line=block_start + offset + 1,
                        message="shell pipeline executes before `set -o pipefail`",
                    )
                )
                break

            if can_enable_here:
                pipefail_enabled = True

            if set_prologue:
                if set_state is None:
                    set_prologue = False
                else:
                    # A control/operator suffix containing another command ends
                    # the simple-set prologue after this line. A trailing ';' by
                    # itself remains harmless.
                    suffix = rest.strip()
                    if suffix not in {"", ";"}:
                        set_prologue = False

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
