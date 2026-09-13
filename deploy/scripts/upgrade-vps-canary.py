#!/usr/bin/env python3
"""Prepare/apply/roll back a reviewed EMPTY, INACTIVE VPS installation.

No activation, database mutation, credentials, unit change or daemon-reload.
An operator must exclude competing administrators and service activation.
"""
import sys
sys.dont_write_bytecode = True
# The authenticated launcher injects this callback only after checking every
# local module. Direct CLI use must stop before importing any local code.
if "verified_tool_identity" not in globals():
    if __name__ == "__main__":
        raise SystemExit("use the externally verified root-owned tool launcher")

    def verified_tool_identity():
        raise ValueError("verified tool launcher required")

import argparse
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import pwd
import stat
import uuid

import inactive_file_transaction as TX


def load_helper(name, filename):
    spec = importlib.util.spec_from_file_location(name, Path(__file__).with_name(filename))
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


PREFLIGHT = load_helper("upgrade_release", "preflight-release.py")
INSTALL = load_helper("upgrade_install", "prepare-vps-canary.py")
BASE = Path("/var/lib/lightning-goats-upgrades")
TARGETS = {
    "lightning-goatsd": Path("/usr/local/bin/lightning-goatsd"),
    "lightning-goatsctl": Path("/usr/local/bin/lightning-goatsctl"),
    "deploy/config.canary.toml.example": Path("/etc/lightning-goats/config.canary.toml.example"),
}
UNIT = Path("/etc/systemd/system/lightning-goats-canary.service")
RECORD = Path("/etc/lightning-goats/STAGING-INSTALL.json")
CONFIG = Path("/etc/lightning-goats")
STATE = Path("/var/lib/lightning-goats")


def trusted(path, directory=False):
    path = Path(path)
    for parent in [*reversed(path.parents), path] if directory else reversed(path.parents):
        info = parent.lstat()
        if not stat.S_ISDIR(info.st_mode) or info.st_uid != 0 or info.st_mode & 0o022:
            raise ValueError("untrusted or aliased administrative parent")
        if any(name.startswith("system.posix_acl") for name in os.listxattr(parent)):
            raise ValueError("unreviewed administrative directory ACL")
    if not directory:
        info = path.lstat()
        if not stat.S_ISREG(info.st_mode) or info.st_uid != 0 or info.st_mode & 0o022:
            raise ValueError("untrusted administrative file")


def root_only():
    if os.geteuid() != 0:
        raise ValueError("root required for authoritative inactive-host checks")


def require_empty_state(user):
    info = STATE.lstat()
    if (not stat.S_ISDIR(info.st_mode) or info.st_uid != user.pw_uid
            or info.st_gid != user.pw_gid or stat.S_IMODE(info.st_mode) != 0o700
            or list(STATE.iterdir())):
        raise ValueError("requires original empty private runtime state")
    trusted(STATE.parent, directory=True)


def snapshot():
    root_only()
    verified_tool_identity()
    user = pwd.getpwnam("lightning-goats")
    if user.pw_uid == 0 or user.pw_gid == 0 or user.pw_shell != "/usr/sbin/nologin" or user.pw_dir != str(STATE):
        raise ValueError("unexpected runtime identity")
    if set(os.getgrouplist(user.pw_name, user.pw_gid)) != {user.pw_gid}:
        raise ValueError("unexpected runtime supplementary groups")
    INSTALL.require_no_sudo()
    locked = INSTALL.command(["passwd", "-S", user.pw_name]).stdout.split()
    if len(locked) < 2 or locked[1] not in ["L", "LK"]:
        raise ValueError("runtime password is not locked")
    for process in Path("/proc").iterdir():
        if process.name.isdigit():
            try:
                lines = (process / "status").read_text().splitlines()
            except FileNotFoundError:
                continue
            for line in lines:
                if line.startswith("Uid:") and user.pw_uid in map(int, line.split()[1:]):
                    raise ValueError("runtime process exists")
    trusted(CONFIG, directory=True)
    require_empty_state(user)
    if set(p.name for p in CONFIG.iterdir()) != {RECORD.name, "config.canary.toml.example"}:
        raise ValueError("active configuration, credentials or unknown config entries present")
    services, files = {}, {}
    properties = "LoadState,ActiveState,SubState,UnitFileState,FragmentPath,DropInPaths,NeedDaemonReload,MainPID"
    for name in ["lightning-goats-canary.service", "lightning-goats.service"]:
        result = INSTALL.command(["systemctl", "show", name, "--property=" + properties])
        values = dict(line.split("=", 1) for line in result.stdout.splitlines() if "=" in line)
        if (values.get("ActiveState") != "inactive" or values.get("MainPID") != "0"
                or values.get("NeedDaemonReload") != "no"):
            raise ValueError("service state changed or manager reload required")
        if name == "lightning-goats-canary.service":
            if (values.get("LoadState") != "loaded" or values.get("SubState") != "dead"
                    or values.get("UnitFileState") != "disabled" or values.get("FragmentPath") != str(UNIT)):
                raise ValueError("unexpected installed canary unit")
        elif values.get("LoadState") != "not-found":
            raise ValueError("production unit must remain absent")
        for path in values.get("DropInPaths", "").split():
            trusted(path)
            files[path] = TX.fingerprint(path)
        services[name] = values
    for path in [*TARGETS.values(), UNIT, RECORD]:
        trusted(path)
        value = TX.fingerprint(path)
        expected_mode = 0o755 if path.name in ["lightning-goatsd", "lightning-goatsctl"] else 0o644
        if value["gid"] != 0 or value["mode"] != expected_mode or any(n.startswith("system.posix_acl") or n == "security.capability" for n in value["xattrs"]):
            raise ValueError("unexpected administrative file metadata")
        files[str(path)] = value
    return {"version": 1, "runtime": {"uid": user.pw_uid, "gid": user.pw_gid},
            "services": services, "files": files, "state_empty": True}


def guard(expected, replacing=False):
    observed = snapshot()
    if replacing:
        expected = json.loads(json.dumps(expected))
        observed = json.loads(json.dumps(observed))
        for path in TARGETS.values():
            expected["files"].pop(str(path))
            observed["files"].pop(str(path))
    if observed != expected:
        raise ValueError("reviewed host baseline drift")


def read_json(path, expected_digest=None):
    with PREFLIGHT.regular_archive(path) as source:
        data = source.read(1024 * 1024 + 1)
    if len(data) > 1024 * 1024:
        raise ValueError("manifest exceeds budget")
    if expected_digest is not None and hashlib.sha256(data).hexdigest() != expected_digest:
        raise ValueError("reviewed manifest digest mismatch")
    return json.loads(data)


def write_json(path, value):
    with path.open("x") as output:
        json.dump(value, output, sort_keys=True, indent=2)
        output.write("\n")
        output.flush()
        os.fsync(output.fileno())
    TX.sync_directory(path.parent)


def prepare(archive, source, digest, baseline_path, baseline_digest):
    root_only()
    tool = verified_tool_identity()
    baseline = read_json(baseline_path, baseline_digest)
    guard(baseline)
    if not BASE.exists():
        trusted(BASE.parent, directory=True)
        BASE.mkdir(mode=0o700)
        TX.sync_directory(BASE.parent)
    trusted(BASE, directory=True)
    if stat.S_IMODE(BASE.stat().st_mode) != 0o700:
        raise ValueError("transaction base must be private")
    bundle = BASE / str(uuid.uuid4())
    bundle.mkdir(mode=0o700)
    TX.sync_directory(BASE)
    # Retain failed preparation for inspection. Copy and pin before verification.
    copy = bundle / "release.tar.gz"
    with PREFLIGHT.regular_archive(archive) as incoming, copy.open("xb") as outgoing:
        total = 0
        while chunk := incoming.read(1024 * 1024):
            total += len(chunk)
            if total > PREFLIGHT.MAX_ARCHIVE_BYTES:
                raise ValueError("archive exceeds budget")
            outgoing.write(chunk)
        outgoing.flush()
        os.fsync(outgoing.fileno())
    release = PREFLIGHT.preflight(copy, source, digest)
    payload = bundle / "payload"
    payload.mkdir()
    PREFLIGHT.RELEASE.verify_archive(copy, source, payload)
    if release["payload_sha256"]["deploy/systemd/lightning-goats-canary.service"] != baseline["files"][str(UNIT)]["sha256"]:
        raise ValueError("unit upgrade requires separate reviewed procedure")
    TX.prepare(bundle / "files", [(p, payload / member, baseline["files"][str(p)])
                                  for member, p in TARGETS.items()], lambda: guard(baseline))
    write_json(bundle / "UPGRADE.json", {"version": 2, "tool": tool, "baseline": baseline,
                                        "release": release})
    return {"prepared_directory": str(bundle), "applied": False, "activated": False}


def validate_records(records, metadata):
    if records.get("version") != 1 or metadata.get("version") != 2:
        raise ValueError("unsupported upgrade manifest")
    rows = records.get("files", [])
    if len(rows) != len(TARGETS) or {r["destination"] for r in rows} != {str(p) for p in TARGETS.values()}:
        raise ValueError("transaction destinations differ from fixed allowlist")
    by_path = {str(p): member for member, p in TARGETS.items()}
    for row in rows:
        old = metadata["baseline"]["files"][row["destination"]]
        new = dict(old, sha256=metadata["release"]["payload_sha256"][by_path[row["destination"]]])
        if row["old"] != old or row["new"] != new:
            raise ValueError("transaction payload/metadata differs from reviewed preparation")


WRITE_PROBE = """import os,sys
fd=os.open(sys.argv[1], os.O_WRONLY | os.O_TMPFILE | os.O_CLOEXEC, 0o600)
try:
    assert os.fstat(fd).st_nlink == 0
    payload=b'synthetic permission probe'
    assert os.write(fd,payload) == len(payload)
    os.fsync(fd)
finally:
    os.close(fd)
"""
READ_PROBE = """import os,sys
assert all(os.access(p,os.R_OK) and not os.access(p,os.W_OK) for p in sys.argv[1:])
"""


def runtime_command():
    user = pwd.getpwnam("lightning-goats")
    return ["setpriv", f"--reuid={user.pw_uid}", f"--regid={user.pw_gid}", "--clear-groups", "--no-new-privs", "--"]


def runtime_probe(write_state=False):
    # No named-file fallback: unsupported/denied O_TMPFILE fails before TX.
    prefix = [*runtime_command(), "/usr/bin/python3", "-I", "-S", "-B", "-c"]
    if write_state:
        INSTALL.command([*prefix, WRITE_PROBE, str(STATE)])
    INSTALL.command([*prefix, READ_PROBE, str(CONFIG), *map(str, TARGETS.values()), str(UNIT), str(RECORD)])


def execute(bundle, rollback=False):
    root_only()
    bundle = Path(bundle)
    if bundle.parent != BASE or str(uuid.UUID(bundle.name)) != bundle.name:
        raise ValueError("expected a canonical prepared transaction directory")
    trusted(bundle, directory=True)
    for path in [bundle / "UPGRADE.json", bundle / "files/TRANSACTION.json", bundle / "files/LOCK"]:
        trusted(path)
    metadata = read_json(bundle / "UPGRADE.json")
    records = read_json(bundle / "files/TRANSACTION.json")
    if metadata.get("tool") != verified_tool_identity():
        raise ValueError("prepared tool generation mismatch; retain legacy evidence")
    validate_records(records, metadata)
    for index in range(len(TARGETS)):
        for version in ["old", "new"]:
            trusted(bundle / "files" / f"{index}.{version}")
    # Resumed apply/rollback may already have old/new replacement fingerprints.
    # TX validates those exact states; every other baseline property stays fixed.
    guard(metadata["baseline"], replacing=True)
    runtime_probe(write_state=True)
    guard(metadata["baseline"], replacing=True)
    result = TX.execute(bundle / "files", lambda: guard(metadata["baseline"], replacing=True), rollback=rollback)
    runtime_probe()
    run_as = runtime_command()
    for name in ["lightning-goatsd", "lightning-goatsctl"]:
        if not INSTALL.command([*run_as, str(TARGETS[name]), "--help"]).stdout.strip():
            raise ValueError("installed help verification failed")
    guard(metadata["baseline"], replacing=True)
    result.update(tool=metadata["tool"], prepared_release_source_commit=metadata["release"]["source_commit"], archive_sha256=metadata["release"]["archive_sha256"], runtime_permissions_verified=True,
                  installed_files={str(path): TX.fingerprint(path) for path in TARGETS.values()})
    write_json(bundle / ("RESULT-" + str(uuid.uuid4()) + ".json"), result)
    return result


if __name__ == "__main__":
    os.umask(0o077)
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    commands.add_parser("snapshot")
    prep = commands.add_parser("prepare")
    for arg in ["archive", "source", "digest", "baseline", "baseline_digest"]:
        prep.add_argument(arg)
    for action in ["apply", "rollback"]:
        commands.add_parser(action).add_argument("directory")
    args = parser.parse_args()
    if args.command == "snapshot":
        result = snapshot()
    elif args.command == "prepare":
        result = prepare(args.archive, args.source, args.digest, args.baseline, args.baseline_digest)
    else:
        result = execute(args.directory, rollback=args.command == "rollback")
    print(json.dumps(result, sort_keys=True, indent=2))
