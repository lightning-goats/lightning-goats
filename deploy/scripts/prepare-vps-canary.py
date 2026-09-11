#!/usr/bin/env python3
"""Prepare a fresh, inactive VPS canary from an explicitly trusted release.

Default: verify and print the plan. --apply creates a locked non-admin runtime
identity and new root-managed files. Existing project accounts/paths are refused.
Does not install credentials or an active configuration, start/enable services,
reload systemd, modify SSH/networking, or install the home gateway on the VPS.
"""
import sys
sys.dont_write_bytecode = True

import argparse
from datetime import datetime, timezone
import grp
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import pwd
import re
import shutil
import stat
import subprocess
import tempfile

SPEC = importlib.util.spec_from_file_location("release", Path(__file__).with_name("smoke-release.py"))
RELEASE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(RELEASE)
USER = "lightning-goats"
SERVICE = "lightning-goats-canary.service"
CONFIG = Path("/etc/lightning-goats")
STATE = Path("/var/lib/lightning-goats")
FILES = {
    "lightning-goatsd": Path("/usr/local/bin/lightning-goatsd"),
    "lightning-goatsctl": Path("/usr/local/bin/lightning-goatsctl"),
    "deploy/config.canary.toml.example": CONFIG / "config.canary.toml.example",
    "deploy/systemd/lightning-goats-canary.service": Path("/etc/systemd/system") / SERVICE,
}
ENV = {"PATH": "/usr/sbin:/usr/bin:/sbin:/bin", "LC_ALL": "C"}


def command(args, check=True):
    return subprocess.run(args, env=ENV, capture_output=True, text=True, timeout=30, check=check)


def require_fresh(paths):
    for path in paths:
        if path.exists() or path.is_symlink():
            raise ValueError(f"existing project path requires review; refusing overwrite: {path}")


def require_trusted_parents(path):
    for parent in path.parents:
        if not parent.exists():
            continue
        info = parent.lstat()
        if not stat.S_ISDIR(info.st_mode) or info.st_uid != 0 or info.st_mode & 0o022:
            raise ValueError(f"installation parent must be a non-writable root-owned directory: {parent}")


def require_inactive():
    for name in [SERVICE, "lightning-goats.service"]:
        active = command(["systemctl", "is-active", name], check=False).stdout.strip()
        enabled = command(["systemctl", "is-enabled", name], check=False).stdout.strip()
        if active not in ["inactive", "unknown"] or enabled not in ["not-found", "disabled"]:
            raise ValueError(f"project service must be inactive and disabled/absent: {name}")


def preflight():
    required = ["useradd", "passwd", "sudo", "setpriv", "systemctl", "systemd-analyze", "python3"]
    if Path("/sys/fs/selinux/enforce").exists():
        required.append("restorecon")
    if any(shutil.which(name, path=ENV["PATH"]) is None for name in required):
        raise ValueError("required staging installation command is unavailable")
    try:
        pwd.getpwnam(USER)
    except KeyError:
        pass
    else:
        raise ValueError("existing runtime account requires review; this helper is for a fresh installation")
    try:
        grp.getgrnam(USER)
    except KeyError:
        pass
    else:
        raise ValueError("existing runtime group requires review")
    require_fresh([CONFIG, STATE, *FILES.values()])
    for path in [CONFIG, STATE, *FILES.values()]:
        require_trusted_parents(path)
    if not Path("/usr/sbin/nologin").is_file():
        raise ValueError("required non-login shell is unavailable")
    require_inactive()


def copy_verified_archive(source, destination, expected):
    if not re.fullmatch(r"[0-9a-f]{64}", expected):
        raise ValueError("expected a reviewed SHA256 for the trusted archive")
    digest, total = hashlib.sha256(), 0
    with source.open("rb") as incoming, destination.open("xb") as outgoing:
        if not stat.S_ISREG(os.fstat(incoming.fileno()).st_mode):
            raise ValueError("release archive must be a regular file")
        while chunk := incoming.read(1024 * 1024):
            total += len(chunk)
            if total > 512 * 1024 * 1024:
                raise ValueError("compressed archive exceeds size budget")
            digest.update(chunk)
            outgoing.write(chunk)
    if digest.hexdigest() != expected:
        raise ValueError("trusted archive SHA256 mismatch")


def install_new_file(source, destination, executable=False):
    # O_EXCL refuses existing files and dangling symlinks; trusted parents are
    # checked before mutation. Files become readable/executable only when complete.
    with source.open("rb") as incoming, destination.open("xb") as outgoing:
        shutil.copyfileobj(incoming, outgoing)
        outgoing.flush()
        os.fsync(outgoing.fileno())
    os.chown(destination, 0, 0)
    destination.chmod(0o755 if executable else 0o644)


def require_no_sudo():
    result = command(["sudo", "-n", "-l", "-U", USER], check=False)
    # sudo versions differ on the status of a successful listing with no grants.
    # Accept only the complete C-locale denial, never an exit code by itself or
    # a substring embedded in a policy listing/error.
    denial = rf"User {re.escape(USER)} is not allowed to run sudo on [^\s]+\.\n?"
    if (result.returncode not in [0, 1] or result.stderr
            or re.fullmatch(denial, result.stdout) is None):
        raise ValueError("cannot establish that runtime identity has no sudo authority")


def apply_install(package, plan):
    command(["useradd", "--system", "--user-group", "--no-create-home",
             "--home-dir", str(STATE), "--shell", "/usr/sbin/nologin",
             "--password", "!", USER])
    user = pwd.getpwnam(USER)
    if user.pw_uid == 0 or user.pw_gid == 0 or user.pw_shell != "/usr/sbin/nologin":
        raise ValueError("new runtime identity is not an unprivileged non-login account")
    if set(os.getgrouplist(USER, user.pw_gid)) != {user.pw_gid}:
        raise ValueError("new runtime identity has unexpected supplementary groups")
    require_no_sudo()
    locked = command(["passwd", "-S", USER]).stdout.split()
    if len(locked) < 2 or locked[1] not in ["L", "LK"]:
        raise ValueError("runtime password is not locked")
    CONFIG.mkdir(mode=0o755)
    os.chown(CONFIG, 0, 0)
    CONFIG.chmod(0o755)
    STATE.mkdir(mode=0o700)
    os.chown(STATE, user.pw_uid, user.pw_gid)
    for name, destination in FILES.items():
        install_new_file(package / name, destination, name in ["lightning-goatsd", "lightning-goatsctl"])
    if Path("/sys/fs/selinux/enforce").exists():
        command(["restorecon", "-F", str(CONFIG), str(STATE), *map(str, FILES.values())])
    for destination in FILES.values():
        expected = plan["files"][str(destination)]
        info = destination.lstat()
        if (info.st_uid, info.st_gid, stat.S_IMODE(info.st_mode)) != (0, 0, int(expected["mode"], 8)):
            raise ValueError("installed file ownership/mode mismatch")
        if hashlib.sha256(destination.read_bytes()).hexdigest() != expected["sha256"]:
            raise ValueError("installed file checksum mismatch")
    command(["systemd-analyze", "verify", str(FILES["deploy/systemd/lightning-goats-canary.service"])])
    run_as = ["setpriv", f"--reuid={user.pw_uid}", f"--regid={user.pw_gid}",
              "--clear-groups", "--no-new-privs", "--"]
    probe = """import os,sys,json,pathlib
paths=json.loads(sys.argv[1]); state=sys.argv[2]
assert os.geteuid()!=0 and os.getegid()!=0
assert all(os.access(p,os.R_OK) and not os.access(p,os.W_OK) for p in paths)
assert os.access(state,os.W_OK)
test=pathlib.Path(state)/'.staging-write-probe'
with test.open('x') as output: output.write('synthetic staging probe')
test.unlink()
"""
    command([*run_as, "python3", "-c", probe, json.dumps([str(CONFIG), *map(str, FILES.values())]), str(STATE)])
    for binary in ["lightning-goatsd", "lightning-goatsctl"]:
        if not command([*run_as, str(FILES[binary]), "--help"]).stdout.strip():
            raise ValueError("empty installed binary help output")
    require_inactive()
    plan.update({"applied": True, "observed_at_utc": datetime.now(timezone.utc).isoformat(),
                 "runtime_identity": {"user": USER, "uid": user.pw_uid, "gid": user.pw_gid,
                                      "password_locked": True, "login_shell_disabled": True, "sudo_allowed": False},
                 "runtime_code_config_nonwritable": True, "state_writable": True,
                 "services_started_or_enabled": False, "active_configuration_installed": False,
                 "credentials_installed": False, "systemd_reloaded": False})
    record = CONFIG / "STAGING-INSTALL.json"
    with record.open("x") as output:
        json.dump(plan, output, indent=2)
        output.write("\n")
        output.flush()
        os.fsync(output.fileno())
    os.chown(record, 0, 0)
    record.chmod(0o644)
    return plan


def prepare(archive, source_commit, archive_sha256, apply=False):
    if apply and os.geteuid() != 0:
        raise ValueError("--apply requires root staging privileges")
    os.umask(0o077)
    with tempfile.TemporaryDirectory(prefix="lg-prepare-vps-", dir="/var/tmp") as directory:
        root = Path(directory)
        copied = root / "release.tar.gz"
        copy_verified_archive(archive, copied, archive_sha256)
        package = root / "package"
        package.mkdir()
        RELEASE.verify_archive(copied, source_commit, package)  # execute no archive payload as root
        if not FILES.keys() <= {str(p.relative_to(package)) for p in package.rglob("*") if p.is_file()}:
            raise ValueError("missing VPS canary installation payload")
        preflight()
        plan = {"scope": "fresh inactive VPS canary preparation only", "source_commit": source_commit,
                "archive_sha256": archive_sha256, "applied": False, "production": "HOLD",
                "installer_sha256": hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
                "runtime_identity_policy": {"user": USER, "group": USER, "system_account": True,
                                            "home": str(STATE), "shell": "/usr/sbin/nologin",
                                            "password_locked": True, "sudo_allowed": False,
                                            "supplementary_groups": []},
                "directories": {str(CONFIG): {"uid": 0, "gid": 0, "mode": "0755"},
                                str(STATE): {"owner": USER, "group": USER, "mode": "0700"}},
                "files": {str(destination): {"sha256": hashlib.sha256((package/name).read_bytes()).hexdigest(),
                          "uid": 0, "gid": 0, "mode": "0755" if name in ["lightning-goatsd", "lightning-goatsctl"] else "0644"}
                          for name, destination in FILES.items()}}
        return apply_install(package, plan) if apply else plan


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("archive", type=Path)
    parser.add_argument("source_commit")
    parser.add_argument("archive_sha256")
    parser.add_argument("--apply", action="store_true")
    args = parser.parse_args()
    print(json.dumps(prepare(args.archive, args.source_commit, args.archive_sha256, args.apply), indent=2))
