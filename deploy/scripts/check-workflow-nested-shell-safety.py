#!/usr/bin/env python3
"""Reject child ``bash/sh -c`` pipelines that can hide failures.

The primary workflow guard intentionally ignores shell operators inside quoted
text. That is correct for ordinary data such as ``printf 'left|right'``, but a
quoted string becomes executable shell code when it is the script argument to a
child ``bash -c`` or ``sh -c`` invocation. Parent-shell ``pipefail`` does not
propagate into that child shell.

This supplemental guard recognizes simple direct child-shell invocations (plus
``env`` wrapping), inspects the ``-c`` script independently, and requires local
child protection. For Bash, either ``bash -o pipefail -c ...`` or an explicit
``set -o pipefail;`` before the nested pipeline is accepted. ``sh -c`` pipelines
are rejected conservatively because ``pipefail`` is not portable across ``sh``
implementations. Complex launch wrappers should be rewritten explicitly rather
than teaching this static checker a full shell grammar.
"""

from __future__ import annotations

import argparse
import importlib.util
import re
import shlex
import sys
from pathlib import Path


BASE_CHECKER = Path(__file__).with_name("check-workflow-shell-safety.py")
_SPEC = importlib.util.spec_from_file_location("workflow_shell_safety_nested_base", BASE_CHECKER)
if _SPEC is None or _SPEC.loader is None:
    raise RuntimeError(f"cannot load workflow shell checker: {BASE_CHECKER}")
_BASE = importlib.util.module_from_spec(_SPEC)
sys.modules[_SPEC.name] = _BASE
_SPEC.loader.exec_module(_BASE)

_ASSIGNMENT_RE = re.compile(r"^[A-Za-z_][A-Za-z0-9_]*=")
_CONTROL_CHARS = frozenset(";&|()")


def _is_control_token(token: str) -> bool:
    return bool(token) and all(char in _CONTROL_CHARS for char in token)


def _shell_tokens(command: str) -> list[str] | None:
    command = _BASE._unquote_simple_yaml_scalar(command)
    lexer = shlex.shlex(command, posix=True, punctuation_chars=";&|()")
    lexer.whitespace_split = True
    lexer.commenters = "#"
    try:
        return list(lexer)
    except ValueError:
        # The base checker remains responsible for ordinary pipelines. An
        # unterminated/complex quoted child command is not accepted as proof of
        # a safe nested pipeline by this narrow supplemental parser.
        return None


def _command_segments(tokens: list[str]) -> list[list[str]]:
    segments: list[list[str]] = []
    current: list[str] = []
    for token in tokens:
        if _is_control_token(token):
            if current:
                segments.append(current)
                current = []
            continue
        current.append(token)
    if current:
        segments.append(current)
    return segments


def _skip_env_wrapper(segment: list[str], index: int) -> int:
    """Return the first command token after a simple ``env`` wrapper."""
    if index >= len(segment) or Path(segment[index]).name != "env":
        return index
    index += 1
    while index < len(segment):
        token = segment[index]
        if _ASSIGNMENT_RE.match(token):
            index += 1
            continue
        if token in {"-i", "--ignore-environment", "-0", "--null"}:
            index += 1
            continue
        if token in {"-u", "--unset", "-C", "--chdir", "-S", "--split-string"}:
            # These options consume one argument. If it is absent, there is no
            # child command for this conservative parser to analyze.
            return min(index + 2, len(segment))
        if token.startswith("--unset=") or token.startswith("--chdir="):
            index += 1
            continue
        if token == "--":
            return index + 1
        if token.startswith("-"):
            index += 1
            continue
        break
    return index


def _child_shell_script(segment: list[str]) -> tuple[str, bool, str] | None:
    index = 0
    while index < len(segment) and _ASSIGNMENT_RE.match(segment[index]):
        index += 1
    index = _skip_env_wrapper(segment, index)
    while index < len(segment) and _ASSIGNMENT_RE.match(segment[index]):
        index += 1
    if index >= len(segment):
        return None

    shell = Path(segment[index]).name.lower()
    if shell not in {"bash", "sh"}:
        return None
    index += 1

    child_pipefail = False
    while index < len(segment):
        token = segment[index]
        if token == "--":
            return None
        if token == "-o":
            if index + 1 >= len(segment):
                return None
            if segment[index + 1] == "pipefail":
                child_pipefail = True
            index += 2
            continue
        if token == "-O":
            if index + 1 >= len(segment):
                return None
            index += 2
            continue
        if token.startswith("--"):
            index += 1
            continue
        if token.startswith("-") and len(token) > 1:
            flags = token[1:]
            if "c" in flags:
                if index + 1 >= len(segment):
                    return None
                return shell, child_pipefail, segment[index + 1]
            index += 1
            continue
        # A non-option before -c is a script/file argument, so later tokens are
        # arguments to that script rather than Bash invocation options.
        return None
    return None


def _script_has_pipeline(script: str) -> bool:
    cleaned = _BASE._strip_quotes_and_comment(script)
    return _BASE._pipeline_index(cleaned) is not None


def _unsafe_child_in_command(command: str) -> bool:
    tokens = _shell_tokens(command)
    if tokens is None:
        return False
    for segment in _command_segments(tokens):
        child = _child_shell_script(segment)
        if child is None:
            continue
        shell, invocation_pipefail, script = child
        if not _script_has_pipeline(script):
            continue
        if shell == "sh":
            return True
        if invocation_pipefail:
            continue
        if "\n" in script or "\r" in script:
            # Do not infer state across a multiline child program. Callers can
            # make the contract explicit with ``bash -o pipefail -c``.
            return True
        if _BASE._single_line_pipeline_is_unsafe(script):
            return True
    return False


def _folded_paragraphs(
    block_lines: list[str], content_indent: int
) -> list[tuple[int, str]]:
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
        block_match = _BASE.RUN_BLOCK_RE.match(lines[idx])
        if block_match is None:
            scalar_match = _BASE.RUN_SCALAR_RE.match(lines[idx])
            if scalar_match is not None:
                run_indent = len(scalar_match.group("indent"))
                run_has_dash = scalar_match.group("dash") is not None
                shell = _BASE._shell_for_run(
                    lines, idx, run_indent, run_has_dash=run_has_dash
                )
                if _BASE._is_shell_like(shell) and _unsafe_child_in_command(
                    scalar_match.group("value")
                ):
                    findings.append(
                        _BASE.Finding(
                            path=path,
                            line=idx + 1,
                            message=(
                                "child shell pipeline lacks local pipefail protection"
                            ),
                        )
                    )
            idx += 1
            continue

        run_indent = len(block_match.group("indent"))
        run_has_dash = block_match.group("dash") is not None
        run_key_indent = run_indent + 2 if run_has_dash else run_indent
        shell = _BASE._shell_for_run(
            lines, idx, run_indent, run_has_dash=run_has_dash
        )
        style = block_match.group("style")
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

        if style == ">":
            for offset, command in _folded_paragraphs(block_lines, content_indent):
                if _unsafe_child_in_command(command):
                    findings.append(
                        _BASE.Finding(
                            path=path,
                            line=block_start + offset + 1,
                            message=(
                                "child shell pipeline lacks local pipefail protection"
                            ),
                        )
                    )
                    break
            continue

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
            if _unsafe_child_in_command(command):
                findings.append(
                    _BASE.Finding(
                        path=path,
                        line=block_start + offset + 1,
                        message="child shell pipeline lacks local pipefail protection",
                    )
                )
                break
            heredoc_match = _BASE.HEREDOC_RE.search(command)
            if heredoc_match:
                heredoc_end = heredoc_match.group("word")

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
