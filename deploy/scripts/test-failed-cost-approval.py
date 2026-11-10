#!/usr/bin/env python3
"""Run the compiled native file-authority fixture as root in a fresh network namespace.

Only for the disposable CI host. Does not install services or read credentials.
"""
import json
from pathlib import Path
import subprocess
import sys

root = Path(__file__).resolve().parents[2]
artifacts = Path(sys.argv[1])
if artifacts.stat().st_size > 4 * 1024 * 1024:
    raise ValueError("native artifact manifest exceeds bound")
binaries = set()
for line in artifacts.read_text().splitlines():
    row = json.loads(line)
    if row.get("reason") == "compiler-artifact" and row.get("profile", {}).get("test") and row.get("target", {}).get("kind") == ["lib"] and row.get("executable"):
        binary = Path(row["executable"]).resolve(strict=True)
        if not binary.is_relative_to(root / "target") or Path(row["manifest_path"]).resolve() != root / "Cargo.toml":
            continue
        binaries.add(binary)
if len(binaries) != 1:
    raise ValueError("select exactly the current crate's compiled native test binary")
fixtures = {
    "payout::cost_recovery::tests::root_owned_approval_and_evidence_checks_are_real_filesystem_authority": "ROOT_OWNERSHIP_FIXTURE_VERIFIED",
    "state_maintenance::tests::root_four_journal_snapshot_preserves_wal_paid_pending_and_signed_state_and_blocks_restore_startup": "ROOT_FOUR_JOURNAL_FIXTURE_VERIFIED",
}
binary = str(binaries.pop())
listed = subprocess.run([binary, "--list"], capture_output=True, text=True, check=True)
for name, marker in fixtures.items():
    if name + ": test" not in listed.stdout.splitlines():
        raise ValueError("required native ownership fixture is missing")
    result = subprocess.run(["sudo", "unshare", "--net", "--", binary, name, "--exact", "--nocapture"],
                            capture_output=True, text=True, check=True)
    if marker not in result.stdout or "1 passed; 0 failed" not in result.stdout:
        raise ValueError("actual root-owned positive fixture did not complete")
    print(result.stdout, end="")
