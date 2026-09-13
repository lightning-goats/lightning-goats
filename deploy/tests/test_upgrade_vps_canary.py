"""Coordinator refusal boundaries; no tests invoke a host apply command."""
import copy
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import pwd
import sys
import tempfile
import unittest
from unittest.mock import patch
import test_release

SCRIPTS = Path(__file__).resolve().parents[1] / "scripts"
sys.path.insert(0, str(SCRIPTS))
SPEC = importlib.util.spec_from_file_location("upgrade", SCRIPTS / "upgrade-vps-canary.py")
UP = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(UP)


class UpgradeTests(unittest.TestCase):
    @unittest.skipUnless(os.geteuid() == 0, "isolated root fixture runs separately in deployment CI")
    def test_disposable_root_prepare_apply_and_rollback(self):
        # All host boundaries are redirected into this disposable /run tree.
        # Service/account snapshot is a fixture; the file transaction and
        # non-root permission/help probes execute for real.
        with tempfile.TemporaryDirectory(prefix="lg-upgrade-fixture-", dir="/run") as directory:
            root = Path(directory)
            root.chmod(0o755)
            config, state, binaries = [root / name for name in ["config", "state", "bin"]]
            for path in [config, state, binaries]:
                path.mkdir(mode=0o755)
                path.chmod(0o755)
            user = pwd.getpwnam("daemon")
            os.chown(state, user.pw_uid, user.pw_gid)
            state.chmod(0o700)
            targets = {"lightning-goatsd": binaries / "lightning-goatsd",
                       "lightning-goatsctl": binaries / "lightning-goatsctl",
                       "deploy/config.canary.toml.example": config / "config.canary.toml.example"}
            unit, record = root / "unit.service", config / "STAGING-INSTALL.json"
            unit.write_bytes(b"unit fixture")
            record.write_bytes(b"historical record")
            for member, path in targets.items():
                path.write_bytes(b'#!/bin/sh\nprintf "old help\\n"\n' if member.startswith("lightning-") else b"old example")
                path.chmod(0o755 if member.startswith("lightning-") else 0o644)
            baseline = {"version": 1, "state_empty": True, "files": {str(p): UP.TX.fingerprint(p) for p in [*targets.values(), unit, record]}}
            baseline_path = root / "baseline.json"
            baseline_path.write_text(json.dumps(baseline))
            archive = root / "release.tar.gz"
            fixture = test_release.ReleaseArchiveTests()
            fixture.archive = archive
            fixture.files = {name: b"fixture" for name in test_release.SMOKE.REQUIRED}
            fixture.files.update({name: b'#!/bin/sh\nprintf "new help\\n"\n' for name in test_release.SMOKE.BINARIES})
            fixture.files.update({"BUILD-INFO.txt": ("source_commit=" + test_release.SOURCE + "\n").encode(),
                                  "deploy/systemd/lightning-goats-canary.service": unit.read_bytes(),
                                  "deploy/config.canary.toml.example": b"new example"})
            fixture.write_archive()
            def observed():
                return dict(baseline, files={str(p): UP.TX.fingerprint(p) for p in [*targets.values(), unit, record]})
            with patch.multiple(UP, BASE=root / "transactions", TARGETS=targets, UNIT=unit, RECORD=record, CONFIG=config, STATE=state), patch.object(UP, "snapshot", side_effect=observed), patch.object(UP.pwd, "getpwnam", return_value=user):
                prepared = UP.prepare(archive, test_release.SOURCE, hashlib.sha256(archive.read_bytes()).hexdigest(), baseline_path, hashlib.sha256(baseline_path.read_bytes()).hexdigest())
                self.assertFalse(prepared["applied"])
                result = UP.execute(prepared["prepared_directory"])
                self.assertTrue(result["runtime_permissions_verified"])
                self.assertEqual(targets["deploy/config.canary.toml.example"].read_bytes(), b"new example")
                UP.execute(prepared["prepared_directory"], rollback=True)
            self.assertEqual(observed(), baseline)
            self.assertEqual(record.read_bytes(), b"historical record")
            self.assertEqual(list(state.iterdir()), [])

    def fixtures(self):
        baseline = {"version": 1, "services": {"canary": "inactive"}, "state_empty": True, "files": {}}
        release = {"payload_sha256": {}}
        rows = []
        for member, path in UP.TARGETS.items():
            old = {"sha256": "a" * 64, "uid": 0, "gid": 0, "mode": 0o755, "xattrs": {}}
            baseline["files"][str(path)] = old
            release["payload_sha256"][member] = "b" * 64
            rows.append({"destination": str(path), "old": old, "new": dict(old, sha256="b" * 64)})
        baseline["files"][str(UP.UNIT)] = {"sha256": "c" * 64}
        return {"version": 1, "files": rows}, {"version": 1, "baseline": baseline, "release": release}

    def test_only_exact_fixed_destination_payload_and_metadata_are_accepted(self):
        records, metadata = self.fixtures()
        UP.validate_records(records, metadata)
        mutations = [
            lambda r: r["files"][0].update(destination="/etc/shadow"),
            lambda r: r["files"].append(copy.deepcopy(r["files"][0])),
            lambda r: r["files"][0]["new"].update(mode=0o4755),
            lambda r: r["files"][0]["new"].update(sha256="d" * 64),
            lambda r: r["files"][0]["old"].update(uid=123),
        ]
        for mutate in mutations:
            candidate = copy.deepcopy(records)
            mutate(candidate)
            with self.assertRaises(ValueError):
                UP.validate_records(candidate, metadata)

    def test_guard_allows_only_replacement_fingerprints_to_change(self):
        _, metadata = self.fixtures()
        original = metadata["baseline"]
        current = copy.deepcopy(original)
        current["files"][str(next(iter(UP.TARGETS.values())))]["sha256"] = "b" * 64
        with patch.object(UP, "snapshot", return_value=current):
            with self.assertRaisesRegex(ValueError, "drift"):
                UP.guard(original)
            UP.guard(original, replacing=True)
        for altered in [dict(current, state_empty=False), dict(current, services={"canary": "active"})]:
            with patch.object(UP, "snapshot", return_value=altered):
                with self.assertRaisesRegex(ValueError, "drift"):
                    UP.guard(original, replacing=True)
        current["files"][str(UP.UNIT)]["sha256"] = "d" * 64
        with patch.object(UP, "snapshot", return_value=current):
            with self.assertRaisesRegex(ValueError, "drift"):
                UP.guard(original, replacing=True)

    def test_manifest_pin_and_final_symlink_are_not_ignored(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "baseline.json"
            path.write_text('{"version":1}')
            digest = hashlib.sha256(path.read_bytes()).hexdigest()
            self.assertEqual(UP.read_json(path, digest), {"version": 1})
            with self.assertRaisesRegex(ValueError, "digest mismatch"):
                UP.read_json(path, "0" * 64)
            link = path.with_name("link")
            link.symlink_to(path)
            with self.assertRaisesRegex(ValueError, "regular file"):
                UP.read_json(link, digest)

    def test_unprivileged_entrypoints_stop_before_host_inspection(self):
        with patch.object(UP.os, "geteuid", return_value=1000), patch.object(UP.pwd, "getpwnam", side_effect=AssertionError("host read")):
            with self.assertRaisesRegex(ValueError, "root required"):
                UP.snapshot()
            with self.assertRaisesRegex(ValueError, "root required"):
                UP.prepare("missing", "x", "y", "missing", "z")
            with self.assertRaisesRegex(ValueError, "root required"):
                UP.execute("missing")

    def test_noncanonical_transaction_path_stops_before_file_operations(self):
        with patch.object(UP, "root_only"), patch.object(UP, "trusted", side_effect=AssertionError("file operation")):
            for directory in ["/tmp/00000000-0000-0000-0000-000000000000", str(UP.BASE / "../bad"), str(UP.BASE / "BAD")]:
                with self.assertRaises(ValueError):
                    UP.execute(directory)
