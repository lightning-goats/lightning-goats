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
    "state_maintenance::inclusive_fee_upgrade::tests::root_inclusive_fee_upgrade_compares_private_images_and_retains_original_backup": "ROOT_INCLUSIVE_FEE_UPGRADE_FIXTURE_VERIFIED",
    "ledger::feed_cap::tests::root_retirement_preserves_runtime_owner_and_exact_paid_history": "ROOT_CREDIT_RETIREMENT_FIXTURE_VERIFIED",
    "payout::cost_recovery::tests::root_owned_approval_and_evidence_checks_are_real_filesystem_authority": "ROOT_OWNERSHIP_FIXTURE_VERIFIED",
    "state_maintenance::tests::root_four_journal_snapshot_preserves_wal_paid_pending_and_signed_state_and_blocks_restore_startup": "ROOT_FOUR_JOURNAL_FIXTURE_VERIFIED",
    "state_bootstrap::tests::root_offline_bootstrap_qualifies_three_actual_runtime_uids_and_refuses_replacing_journals": "ROOT_OFFLINE_BOOTSTRAP_FIXTURE_VERIFIED",
    "state_maintenance::legacy::tests::root_legacy_core_upgrade_preserves_wal_pending_receipts_epoch_signed_bytes_and_compares_migrated_copies": "ROOT_LEGACY_CORE_UPGRADE_FIXTURE_VERIFIED",
    "state_maintenance::contact_upgrade::tests::root_contact_upgrade_compares_private_images_and_retains_original_backup": "ROOT_CONTACT_UPGRADE_FIXTURE_VERIFIED",
}
binary = str(binaries.pop())
listed = subprocess.run([binary, "--list"], capture_output=True, text=True, check=True)
for name, marker in fixtures.items():
    if name + ": test" not in listed.stdout.splitlines():
        raise ValueError("required native ownership fixture is missing")
    result = subprocess.run(["sudo", "unshare", "--net", "--", binary, name, "--exact", "--include-ignored", "--nocapture"],
                            capture_output=True, text=True, check=False)
    if result.returncode:
        # Synthetic disposable fixtures only; preserve bounded failure evidence.
        print(result.stdout[-65536:], end="")
        print(result.stderr[-65536:], end="", file=sys.stderr)
        raise ValueError("isolated native ownership fixture failed")
    if marker not in result.stdout or "1 passed; 0 failed" not in result.stdout:
        raise ValueError("actual root-owned positive fixture did not complete")
    print(result.stdout, end="")
