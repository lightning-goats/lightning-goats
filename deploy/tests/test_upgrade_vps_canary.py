"""Coordinator refusal boundaries; no tests invoke a host apply command."""
import copy
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import pwd
import sys
import subprocess
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
    def setUp(self):
        # Explicit unit-fixture trust provider; real launcher checked separately.
        self.tool_patch = patch.object(UP, "verified_tool_identity", return_value={
            "source_commit": "a" * 40, "manifest_sha256": "b" * 64})
        self.tool_patch.start()
        self.addCleanup(self.tool_patch.stop)

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
                with patch.object(UP, "STATE", state):
                    UP.require_empty_state(user)
                return dict(baseline, files={str(p): UP.TX.fingerprint(p) for p in [*targets.values(), unit, record]})
            with patch.multiple(UP, BASE=root / "transactions", TARGETS=targets, UNIT=unit, RECORD=record, CONFIG=config, STATE=state), patch.object(UP, "snapshot", side_effect=observed), patch.object(UP.pwd, "getpwnam", return_value=user):
                prepared = UP.prepare(archive, test_release.SOURCE, hashlib.sha256(archive.read_bytes()).hexdigest(), baseline_path, hashlib.sha256(baseline_path.read_bytes()).hexdigest())
                self.assertFalse(prepared["applied"])
                for rollback in [False, True]:
                    with patch.object(UP, "verified_tool_identity", return_value={"manifest_sha256": "changed"}), patch.object(UP.TX, "execute", side_effect=AssertionError("replacement reached")):
                        with self.assertRaisesRegex(ValueError, "tool generation mismatch"):
                            UP.execute(prepared["prepared_directory"], rollback=rollback)
                metadata_path = Path(prepared["prepared_directory"]) / "UPGRADE.json"
                original_metadata = metadata_path.read_bytes()
                legacy = json.loads(original_metadata)
                legacy.pop("tool")
                legacy["version"] = 1
                metadata_path.write_text(json.dumps(legacy))
                with patch.object(UP.TX, "execute", side_effect=AssertionError("replacement reached")):
                    with self.assertRaisesRegex(ValueError, "tool generation mismatch"):
                        UP.execute(prepared["prepared_directory"])
                metadata_path.write_bytes(original_metadata)
                # Exercise the production invariant, not a mocked state_empty flag.
                residue = state / ".stranded-control"
                residue.write_text("negative control")
                with self.assertRaisesRegex(ValueError, "empty private runtime state"):
                    observed()
                residue.unlink()  # Negative-control setup only, never recovery cleanup.
                for mode in ["unsupported", "denied"]:
                    with self.subTest(write_preflight=mode):
                        if mode == "denied":
                            state.chmod(0o500)
                            probe = UP.WRITE_PROBE
                        else:
                            probe = "import errno; raise OSError(errno.EOPNOTSUPP, 'synthetic unsupported filesystem')"
                        # The failing subprocess retains the real non-root runtime identity.
                        # For denial, exercise O_TMPFILE itself under that identity.
                        try:
                            with patch.object(UP, "WRITE_PROBE", probe), patch.object(UP.TX, "execute", side_effect=AssertionError("replacement reached")):
                                if mode == "denied":
                                    # Keep host metadata valid: syscall-denial is tested below;
                                    # the real guard must reject this mode change even earlier.
                                    with self.assertRaisesRegex(ValueError, "empty private runtime state"):
                                        UP.execute(prepared["prepared_directory"])
                                else:
                                    with self.assertRaises(subprocess.CalledProcessError):
                                        UP.execute(prepared["prepared_directory"])
                        finally:
                            state.chmod(0o700)
                        self.assertEqual(observed(), baseline)
                tx_execute = UP.TX.execute
                retained = {p: p.read_bytes() for p in (Path(prepared["prepared_directory"]) / "files").iterdir()}
                for interrupted_rollback in [False, True]:
                    if interrupted_rollback:
                        UP.execute(prepared["prepared_directory"])
                    def interrupted(*args, **kwargs):
                        tx_execute(*args, **kwargs)
                        raise RuntimeError("interruption after transaction")
                    receipts = set(Path(prepared["prepared_directory"]).glob("RESULT-*.json"))
                    with patch.object(UP.TX, "execute", side_effect=interrupted):
                        with self.assertRaisesRegex(RuntimeError, "interruption after transaction"):
                            UP.execute(prepared["prepared_directory"], rollback=interrupted_rollback)
                    self.assertEqual(list(state.iterdir()), [])
                    self.assertEqual(set(Path(prepared["prepared_directory"]).glob("RESULT-*.json")), receipts)
                    # Normal coordinator rollback must succeed without manual STATE cleanup.
                    UP.execute(prepared["prepared_directory"], rollback=True)
                    self.assertEqual(observed(), baseline)
                    for path, data in retained.items():
                        self.assertEqual(path.read_bytes(), data)
                result = UP.execute(prepared["prepared_directory"])
                self.assertTrue(result["runtime_permissions_verified"])
                self.assertEqual(targets["deploy/config.canary.toml.example"].read_bytes(), b"new example")
                UP.execute(prepared["prepared_directory"], rollback=True)
            self.assertEqual(observed(), baseline)
            self.assertEqual(record.read_bytes(), b"historical record")
            self.assertEqual(list(state.iterdir()), [])

    @unittest.skipUnless(os.geteuid() == 0, "isolated root probe fixture runs in CI")
    def test_unnamed_probe_runtime_authority_denial_and_process_death(self):
        with tempfile.TemporaryDirectory(prefix="lg-probe-fixture-", dir="/run") as directory:
            root = Path(directory)
            root.chmod(0o755)
            state = root / "state"
            state.mkdir(mode=0o700)
            user = pwd.getpwnam("daemon")
            os.chown(state, user.pw_uid, user.pw_gid)
            with patch.object(UP.pwd, "getpwnam", return_value=user):
                command = [*UP.runtime_command(), "/usr/bin/python3", "-I", "-S", "-B", "-c"]
                UP.INSTALL.command([*command, UP.WRITE_PROBE, str(state)])
                self.assertEqual(list(state.iterdir()), [])
                death = UP.WRITE_PROBE.replace("    os.fsync(fd)", "    os.fsync(fd)\n    os._exit(23)")
                result = UP.INSTALL.command([*command, death, str(state)], check=False)
                self.assertEqual(result.returncode, 23)
                self.assertEqual(list(state.iterdir()), [])
                state.chmod(0o500)
                with self.assertRaises(subprocess.CalledProcessError):
                    UP.INSTALL.command([*command, UP.WRITE_PROBE, str(state)])
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
        return {"version": 1, "files": rows}, {"version": 2, "baseline": baseline, "release": release}

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
