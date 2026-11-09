#!/usr/bin/env python3
"""Upgrade receive binaries and the local operator wrapper, retaining all paid state.

Default verifies and prints a plan. --apply stops the daemon, backs up state,
replaces root-owned binaries, records provenance, then restarts the same mode.
On failure no database is restored and no legacy payment route is reactivated.
"""
import argparse
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import shutil
import sqlite3
import subprocess
import tempfile
import time
import tomllib
import urllib.request


def run(*args):
    return subprocess.run(args, check=True, capture_output=True, text=True)


def digest(path):
    with Path(path).open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("archive", type=Path)
    parser.add_argument("source")
    parser.add_argument("sha256")
    parser.add_argument("--apply", action="store_true")
    args = parser.parse_args()
    spec = importlib.util.spec_from_file_location("release", Path(__file__).with_name("smoke-release.py"))
    release = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(release)
    with tempfile.TemporaryDirectory(prefix="lg-receive-upgrade-") as temporary:
        temporary = Path(temporary)
        archive = temporary / "release.tar.gz"
        shutil.copyfile(args.archive, archive)
        if digest(archive) != args.sha256:
            raise ValueError("archive SHA256 mismatch")
        payload = temporary / "payload"
        payload.mkdir()
        release.verify_archive(archive, args.source, payload)
        print(json.dumps({"source": args.source, "action": "upgrade" if args.apply else "plan",
                          "configuration_preserved": True, "database_preserved": True}), flush=True)
        if not args.apply:
            return
        if os.geteuid() != 0:
            raise ValueError("root required")
        config_path = Path("/etc/lightning-goats/config.receive.toml")
        config_bytes = config_path.read_bytes()
        config = tomllib.loads(config_bytes.decode())
        if config["database"]["url"] != "sqlite:///var/lib/lightning-goats/lightning-goats.db":
            raise ValueError("unexpected database location; review upgrade explicitly")
        marker = Path("/etc/lightning-goats/RECEIVE-INSTALL.json")
        previous = json.loads(marker.read_text())
        unit = "lightning-goats-receive.service"
        run("systemctl", "is-active", "--quiet", unit)
        backup = Path("/var/backups/lightning-goats") / time.strftime("receive-upgrade-%Y%m%dT%H%M%SZ", time.gmtime())
        backup.mkdir(mode=0o700)
        run("systemctl", "stop", unit)
        # Back up only after all daemon workers are quiesced. Never restore an
        # older database automatically: that could replay financial/physical work.
        shutil.copytree("/var/lib/lightning-goats", backup / "state")
        shutil.copytree("/etc/lightning-goats", backup / "configuration")
        database = Path("/var/lib/lightning-goats/lightning-goats.db")
        with sqlite3.connect(f"file:{database}?mode=ro", uri=True) as connection:
            if connection.execute("PRAGMA quick_check").fetchall() != [("ok",)]:
                raise ValueError("database integrity check failed; service remains stopped")
        for name in ("lightning-goatsd", "lightning-goatsctl"):
            target = Path("/usr/local/bin") / name
            if target.is_symlink() or target.stat().st_uid != 0:
                raise ValueError("unexpected installed binary ownership")
            shutil.copy2(target, backup / name)
            pending = target.with_name(name + ".pending")
            if pending.exists() or pending.is_symlink():
                raise ValueError("pending binary already exists")
            shutil.copyfile(payload / name, pending)
            pending.chmod(0o755)
            os.chown(pending, 0, 0)
            run("restorecon", str(pending))
            os.replace(pending, target)
            run("restorecon", str(target))
        wrapper = Path("/usr/local/bin/lg-credit")
        if wrapper.is_symlink():
            raise ValueError("unexpected operator wrapper symlink")
        if wrapper.exists():
            shutil.copy2(wrapper, backup / "lg-credit")
        shutil.copyfile(payload / "deploy/scripts/lg-credit", wrapper)
        wrapper.chmod(0o755)
        os.chown(wrapper, 0, 0)
        run("restorecon", str(wrapper))
        if config_path.read_bytes() != config_bytes:
            raise ValueError("configuration changed during upgrade; service remains stopped")
        record = {"source": args.source, "archive_sha256": args.sha256,
                  "daemon_sha256": digest("/usr/local/bin/lightning-goatsd"),
                  "backup": str(backup), "previous": previous,
                  "mode_preserved": config["service"]["mode"]}
        pending = marker.with_suffix(".pending")
        if pending.exists() or pending.is_symlink():
            raise ValueError("pending install record already exists")
        pending.write_text(json.dumps(record, indent=2) + "\n")
        pending.chmod(0o644)
        os.replace(pending, marker)
        run("restorecon", str(marker))
        run("systemctl", "start", unit)
        opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))
        for attempt in range(20):
            try:
                with opener.open("http://127.0.0.1:8787/healthz", timeout=2) as response:
                    if response.status == 200:
                        print(json.dumps({"health": "ok", "backup": str(backup),
                                          "mode": config["service"]["mode"]}))
                        return
            except OSError:
                pass
            time.sleep(0.5)
        raise ValueError("upgraded daemon health failed; state retained, inspect service")


if __name__ == "__main__":
    main()
