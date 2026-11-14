#!/usr/bin/env python3
"""Arm operator-approved feeding behind the existing gateway's local override.

Default prints a read-only plan. --apply changes shadow to canary, preserving
paid state and keeping Nostr disabled. Requires FeederOverride ON throughout the
transition. Never changes a home Item or submits a feeder request itself.
"""
import argparse
import json
import os
from pathlib import Path
import re
import shutil
import sqlite3
import subprocess
import time
import tomllib
import urllib.request

CONFIG = Path("/etc/lightning-goats/config.receive.toml")
UNIT = "lightning-goats-receive.service"


def run(*args):
    return subprocess.run(args, capture_output=True, text=True, check=True)


class NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        return None


def read_json(url):
    opener = urllib.request.build_opener(urllib.request.ProxyHandler({}), NoRedirect())
    with opener.open(url, timeout=10) as response:
        if response.status != 200 or response.headers.get_content_type() != "application/json":
            raise ValueError("unexpected gateway/status response")
        body = response.read(4097)
        if len(body) > 4096:
            raise ValueError("gateway/status response too large")
        return json.loads(body)


def status():
    result = run("/usr/local/bin/lightning-goatsctl", "--config", str(CONFIG), "status")
    return dict(line.split("=", 1) for line in result.stdout.splitlines() if "=" in line)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--apply", action="store_true")
    args = parser.parse_args()
    if os.geteuid() != 0:
        raise ValueError("root required for authoritative state/config checks")
    before = CONFIG.read_bytes()
    config = tomllib.loads(before.decode())
    if config["service"]["mode"] not in ("shadow", "canary"):
        raise ValueError("expected shadow/canary; do not change an active signer deployment")
    if config["database"]["url"] != "sqlite:///var/lib/lightning-goats/lightning-goats.db" or config["gateway"]["url"] != "http://10.8.0.6:8789/":
        raise ValueError("unexpected ledger/gateway; review explicitly")
    run("systemctl", "is-active", "--quiet", UNIT)
    state = status()
    if state["unresolved_feed_attempt_id"] != "none":
        raise ValueError("unresolved feeder UUID requires reconciliation, not fresh dispatch")
    safety = read_json("http://10.8.0.6:8789/v1/feeder/override")
    if safety != {"override_enabled": True, "remote_enabled": True}:
        raise ValueError("requires operator-held FeederOverride ON and local remote-enable ON")
    plan = {"action": "arm_feeding" if args.apply else "plan", "mode": "canary",
            "override_item": "FeederOverride", "override_held_on": True,
            "credit_sats": int(state["feed_credit_sats"]), "feeds_due": int(state["feeds_due"]),
            "unresolved": None, "nostr_enabled": False, "gateway_command_submitted": False}
    print(json.dumps(plan), flush=True)
    if not args.apply or config["service"]["mode"] == "canary":
        return
    mode = re.compile(r'(?m)^mode = "shadow"$')
    after, changed = mode.subn('mode = "canary"', before.decode())
    if changed != 1 or tomllib.loads(after)["service"]["mode"] != "canary":
        raise ValueError("unexpected mode configuration")
    site = Path("/etc/nginx/conf.d/lightning-goats.com.conf").read_text()
    roots = re.findall(r"(?m)^\s*root (/var/www/lightning-goats-releases/[0-9a-f]{40});", site)
    if len(roots) != 1:
        raise ValueError("unexpected public static root")
    site_config = Path(roots[0]) / "site-config.js"
    if site_config.is_symlink() or site_config.stat().st_uid != 0:
        raise ValueError("expected protected public site configuration")
    availability = site_config.read_bytes()
    updated = re.sub(r"feedingAvailable:\s*false", "feedingAvailable: true", availability.decode())
    if updated == availability.decode():
        raise ValueError("review unexpected existing feeding-availability settings")
    backup = Path("/var/backups/lightning-goats") / time.strftime("feeding-arm-%Y%m%dT%H%M%SZ", time.gmtime())
    backup.mkdir(mode=0o700)
    (backup / "config.receive.toml").write_bytes(before)
    (backup / "site-config.js").write_bytes(availability)
    unit_path = Path("/etc/systemd/system") / UNIT
    unit_bytes = unit_path.read_bytes()
    (backup / UNIT).write_bytes(unit_bytes)
    if CONFIG.read_bytes() != before:
        raise ValueError("concurrent configuration change")
    run("systemctl", "stop", UNIT)
    shutil.copytree("/var/lib/lightning-goats", backup / "quiesced-state")
    database = Path("/var/lib/lightning-goats/lightning-goats.db")
    with sqlite3.connect(f"file:{database}?mode=ro", uri=True) as connection:
        if connection.execute("PRAGMA quick_check").fetchall() != [("ok",)]:
            raise ValueError("database check failed; service remains stopped")
    # Check the actual local gate once more while all payment/feed workers are
    # quiesced. No OpenHAB token, Item command or fresh gateway UUID is used.
    if read_json("http://10.8.0.6:8789/v1/feeder/override") != safety:
        run("systemctl", "start", UNIT)
        raise ValueError("operator safety state changed; mode remains shadow")
    CONFIG.write_text(after)
    CONFIG.chmod(0o644)
    new_unit = unit_bytes.decode().replace("Description=Lightning Goats Strike receives (physical dispatch disabled)", "Description=Lightning Goats Strike payments and feeder automation")
    new_unit = new_unit.replace("# Receive-only deployment uses shadow mode: no new physical feeds or Nostr signing.", "# Operator-approved canary feeding follows gateway-local safety; Nostr remains disabled.")
    unit_path.write_text(new_unit)
    unit_path.chmod(0o644)
    run("systemctl", "daemon-reload")
    try:
        run("systemctl", "start", UNIT)
        for attempt in range(20):
            try:
                live = read_json("http://127.0.0.1:8787/api/v1/status")
                if live["mode"] == "canary" and live["gateway_reachable"] and live["feeder_override_active"] is True:
                    break
            except (OSError, ValueError):
                pass
            time.sleep(.5)
        else:
            raise ValueError("armed service did not confirm paused state")
    except (OSError, ValueError, subprocess.CalledProcessError):
        # Restore configuration only; never restore an older financial database.
        CONFIG.write_bytes(before)
        unit_path.write_bytes(unit_bytes)
        run("systemctl", "daemon-reload")
        run("systemctl", "restart", UNIT)
        raise
    site_config.write_text(updated)
    site_config.chmod(0o644)
    record = {**plan, "backup": str(backup), "enabled_at": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
              "verified_status": {key: live[key] for key in ("mode", "feed_credit_sats", "threshold_sats", "feeds_due", "unresolved_feed_attempt", "feeder_override_active", "remote_feeding_enabled")}}
    marker = Path("/etc/lightning-goats/FEEDING-ENABLE.json")
    marker.write_text(json.dumps(record, indent=2) + "\n")
    marker.chmod(0o644)
    print(json.dumps(record))


if __name__ == "__main__":
    main()
