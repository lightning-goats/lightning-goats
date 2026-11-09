#!/usr/bin/env python3
"""Install the verified OBS static release and isolated display-data cache.

No payment binary, credential, DNS, WireGuard, feed mode or paid state is replaced.
--apply also enables the existing overlay-only informational/weather scheduler.
"""
import argparse
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import pwd
import re
import shutil
import subprocess
import tempfile
import time
import tomllib


def run(*args):
    return subprocess.run(args, capture_output=True, text=True, check=True)


def install(source, destination, mode=0o644):
    destination = Path(destination)
    if destination.is_symlink():
        raise ValueError("refusing symlink installation target")
    shutil.copyfile(source, destination)
    destination.chmod(mode)
    os.chown(destination, 0, 0)


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
    with tempfile.TemporaryDirectory(prefix="lg-obs-install-") as temporary:
        temporary = Path(temporary)
        archive = temporary / "release.tar.gz"
        shutil.copyfile(args.archive, archive)
        with archive.open("rb") as stream:
            if hashlib.file_digest(stream, "sha256").hexdigest() != args.sha256:
                raise ValueError("archive hash mismatch")
        payload = temporary / "payload"
        payload.mkdir()
        release.verify_archive(archive, args.source, payload)
        print(json.dumps({"source": args.source, "action": "install" if args.apply else "plan",
                          "physical_mode_preserved": True, "payment_database_preserved": True,
                          "display_identity": "lightning-goats-display"}), flush=True)
        if not args.apply:
            return
        if os.geteuid() != 0:
            raise ValueError("root required")
        site_path = Path("/etc/nginx/conf.d/lightning-goats.com.conf")
        site = site_path.read_text()
        roots = re.findall(r"(?m)^\s*root (/var/www/lightning-goats-releases/[0-9a-f]{40});", site)
        if len(roots) != 1:
            raise ValueError("review unexpected current static root")
        original_web = Path(roots[0])
        config_path = Path("/etc/lightning-goats/config.receive.toml")
        config_bytes = config_path.read_bytes()
        config = tomllib.loads(config_bytes.decode())
        if config["database"]["url"] != "sqlite:///var/lib/lightning-goats/lightning-goats.db":
            raise ValueError("unexpected financial database")
        run("systemctl", "is-active", "--quiet", "lightning-goats-receive.service")
        backup = Path("/var/backups/lightning-goats") / time.strftime("obs-%Y%m%dT%H%M%SZ", time.gmtime())
        backup.mkdir(mode=0o700)
        paths = [site_path, config_path, Path("/etc/nginx/snippets/lightning-goats-production.conf"),
                 Path("/usr/local/libexec/lightning-goats/update-overlay-data.py"),
                 Path("/etc/nftables/lightning-goats-display.nft")]
        for unit in ("lightning-goats-display.service", "lightning-goats-display.timer", "lightning-goats-display-firewall.service"):
            paths.append(Path("/etc/systemd/system") / unit)
        for path in paths:
            if path.exists():
                target = backup / path.relative_to("/")
                target.parent.mkdir(parents=True, exist_ok=True)
                shutil.copy2(path, target)
        try:
            user = pwd.getpwnam("lightning-goats-display")
        except KeyError:
            run("useradd", "--system", "--user-group", "--home-dir", "/var/cache/lightning-goats-display", "--shell", "/usr/sbin/nologin", "lightning-goats-display")
            user = pwd.getpwnam("lightning-goats-display")
        if user.pw_uid == 0 or user.pw_gid == 0 or user.pw_shell != "/usr/sbin/nologin" or set(os.getgrouplist(user.pw_name, user.pw_gid)) != {user.pw_gid}:
            raise ValueError("display writer must have a separate non-admin identity")
        cache = Path("/var/cache/lightning-goats-display")
        if cache.is_symlink():
            raise ValueError("refusing symlink display cache")
        cache.mkdir(mode=0o755, exist_ok=True)
        cache.chmod(0o755)
        os.chown(cache, user.pw_uid, user.pw_gid)
        labeling = r"/var/cache/lightning-goats-display(/.*)?"
        result = subprocess.run(["semanage", "fcontext", "-a", "-t", "httpd_sys_content_t", labeling], capture_output=True, text=True)
        if result.returncode:
            run("semanage", "fcontext", "-m", "-t", "httpd_sys_content_t", labeling)
        run("restorecon", "-R", str(cache))
        libexec = Path("/usr/local/libexec/lightning-goats")
        libexec.mkdir(parents=True, exist_ok=True)
        install(payload / "deploy/scripts/update-overlay-data.py", libexec / "update-overlay-data.py", 0o755)
        for unit in ("lightning-goats-display.service", "lightning-goats-display.timer", "lightning-goats-display-firewall.service"):
            install(payload / "deploy/systemd" / unit, Path("/etc/systemd/system") / unit)
        policy = (payload / "deploy/nftables/lightning-goats-display.nft.example").read_text().replace("@UID@", str(user.pw_uid))
        firewall = temporary / "display.nft"
        firewall.write_text(policy)
        run("nft", "-c", "-f", str(firewall))
        install(firewall, "/etc/nftables/lightning-goats-display.nft")
        web = Path("/var/www/lightning-goats-releases") / args.source
        if web.exists():
            raise ValueError("versioned static root exists; select a new reviewed source")
        shutil.copytree(payload / "web", web)
        for path in [web, *web.rglob("*")]:
            os.chown(path, 0, 0)
            path.chmod(0o755 if path.is_dir() else 0o644)
        install(original_web / "site-config.js", web / "site-config.js")
        # Preserve uploaded original artwork on subsequent frontend upgrades.
        for name in ("newton.png", "nova.png", "cosmo.png", "dexter.png", "rowan.png", "lightning.png"):
            original = original_web / "images" / name
            if original.is_file() and not original.is_symlink() and not (web / "images" / name).exists():
                install(original, web / "images" / name)
        run("restorecon", "-R", str(web), str(libexec), *[str(Path("/etc/systemd/system") / name) for name in ("lightning-goats-display.service", "lightning-goats-display.timer", "lightning-goats-display-firewall.service")])
        run("systemctl", "daemon-reload")
        run("systemd-analyze", "verify", "/etc/systemd/system/lightning-goats-display.service", "/etc/systemd/system/lightning-goats-display.timer")
        run("systemctl", "enable", "--now", "lightning-goats-display-firewall.service")
        run("systemctl", "start", "lightning-goats-display.service")
        run("systemctl", "enable", "--now", "lightning-goats-display.timer")
        install(payload / "deploy/nginx/lightning-goats-production.conf.example", "/etc/nginx/snippets/lightning-goats-production.conf")
        site_path.write_text(site.replace(str(original_web), str(web)))
        try:
            run("nginx", "-t")
        except subprocess.CalledProcessError:
            install(backup / site_path.relative_to("/"), site_path)
            snippet = Path("/etc/nginx/snippets/lightning-goats-production.conf")
            install(backup / snippet.relative_to("/"), snippet)
            raise
        run("systemctl", "reload", "nginx")
        new_config = re.sub(r"(?m)^interface_info_enabled = false$", "interface_info_enabled = true", config_bytes.decode())
        new_config = re.sub(r"(?m)^weather_enabled = false$", "weather_enabled = true", new_config)
        parsed = tomllib.loads(new_config)
        if parsed["service"] != config["service"] or parsed["database"] != config["database"]:
            raise ValueError("unexpected service or ledger change")
        if config_path.read_bytes() != config_bytes:
            raise ValueError("concurrent runtime configuration change")
        if new_config.encode() != config_bytes:
            run("systemctl", "stop", "lightning-goats-receive.service")
            shutil.copytree("/var/lib/lightning-goats", backup / "quiesced-state")
            config_path.write_text(new_config)
            config_path.chmod(0o644)
            try:
                run("systemctl", "start", "lightning-goats-receive.service")
                time.sleep(1)
                run("systemctl", "is-active", "--quiet", "lightning-goats-receive.service")
            except subprocess.CalledProcessError:
                config_path.write_bytes(config_bytes)
                run("systemctl", "start", "lightning-goats-receive.service")
                raise
        record = {"source": args.source, "archive_sha256": args.sha256,
                  "web_root": str(web), "backup": str(backup), "mode_preserved": config["service"]["mode"],
                  "display_uid": user.pw_uid, "weather_source": "gateway /v1/weather", "market_source": "CoinGecko"}
        marker = Path("/etc/lightning-goats/OVERLAY-INSTALL.json")
        marker.write_text(json.dumps(record, indent=2) + "\n")
        marker.chmod(0o644)
        print(json.dumps(record))


if __name__ == "__main__":
    main()
