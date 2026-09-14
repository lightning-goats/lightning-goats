#!/usr/bin/env python3
"""Baseline bridge for child-shell workflow safety regression.

This temporary draft delegates to the existing workflow shell checker so the
nested ``bash/sh -c`` false-negative can be captured by a focused regression
before the supplemental guard is implemented. PR #87 is draft; do not merge a
head where the focused regression is red.
"""

from __future__ import annotations

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


if __name__ == "__main__":
    raise SystemExit(_BASE.main())
