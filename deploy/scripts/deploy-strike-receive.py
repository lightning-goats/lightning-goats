#!/usr/bin/env python3
"""Stage a verified Strike receive-only release on the existing Fedora VPS.

Default prints a plan. --install backs up and installs inactive code/config;
--activate starts the receive service and switches only lightning-goats.com.
No DNS/WireGuard/home changes, real payments, or physical dispatch are performed.
"""
import argparse
import hashlib
import importlib.util
import json
import os
from pathlib import Path, PurePosixPath
import pwd
import shutil
import subprocess
import tarfile
import tempfile
import time
import urllib.request
import ipaddress

SPEC = importlib.util.spec_from_file_location("release", Path(__file__).with_name("smoke-release.py"))
RELEASE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(RELEASE)


def run(*args):
    return subprocess.run(args, check=True, capture_output=True, text=True)


def digest(path):
    with Path(path).open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def install(source, target, mode=0o644):
    target = Path(target)
    if target.is_symlink():
        raise ValueError(f"refusing symlink: {target}")
    shutil.copyfile(source, target)
    target.chmod(mode)
    os.chown(target, 0, 0)


def write(path, value, mode=0o644):
    path = Path(path)
    if path.is_symlink():
        raise ValueError(f"refusing symlink: {path}")
    path.write_text(value)
    path.chmod(mode)
    os.chown(path, 0, 0)


def directory(path, mode=0o755):
    path = Path(path)
    if path.is_symlink():
        raise ValueError(f"refusing symlink: {path}")
    path.mkdir(parents=True, exist_ok=True)
    path.chmod(mode)
    os.chown(path, 0, 0)
    return path


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("archive", type=Path)
    parser.add_argument("source")
    parser.add_argument("sha256")
    parser.add_argument("crs_archive", type=Path)
    parser.add_argument("--listen-address", required=True, type=ipaddress.IPv4Address)
    action = parser.add_mutually_exclusive_group()
    action.add_argument("--install", action="store_true")
    action.add_argument("--activate", action="store_true")
    args = parser.parse_args()
    # Retain a private copy before verifying so later use cannot race the input.
    with tempfile.TemporaryDirectory(prefix="lg-receive-") as temp:
        temp = Path(temp)
        archive = temp / "release.tar.gz"
        shutil.copyfile(args.archive, archive)
        if digest(archive) != args.sha256:
            raise ValueError("release archive SHA256 mismatch")
        payload = temp / "payload"
        payload.mkdir()
        RELEASE.verify_archive(archive, args.source, payload)
        pins = json.loads((payload / "deploy/waf/pins.json").read_text())
        crs_archive = temp / "crs.tar.gz"
        shutil.copyfile(args.crs_archive, crs_archive)
        if digest(crs_archive) != pins["crs_sha256"]:
            raise ValueError("CRS archive SHA256 mismatch")
        print(json.dumps({"source": args.source, "domain": "lightning-goats.com",
                          "mode": "shadow", "physical_dispatch": False,
                          "crs_version": pins["crs_version"],
                          "action": "install" if args.install else "activate" if args.activate else "plan"}))
        if not (args.install or args.activate):
            return
        if os.geteuid() != 0:
            raise ValueError("installation and activation require root")
        for unit in ("lightning-goats", "lightning-goats-canary", "lightning-goats-receive"):
            if subprocess.run(["systemctl", "is-active", "--quiet", unit]).returncode == 0:
                raise ValueError(f"{unit} is active; quiesce/backup/review before replacing code")
        runtime = pwd.getpwnam("lightning-goats")
        if runtime.pw_uid == 0 or runtime.pw_gid == 0 or runtime.pw_shell not in ("/usr/sbin/nologin", "/sbin/nologin"):
            raise ValueError("expected existing non-admin nologin runtime identity")
        for name, version in pins["fedora_packages"].items():
            if run("rpm", "-q", "--qf", "%{VERSION}-%{RELEASE}", name).stdout != version:
                raise ValueError(f"installed WAF ABI/version mismatch: {name}")
        if args.install:
            if Path("/etc/lightning-goats/config.receive.toml").exists():
                raise ValueError("receive config exists; preserve/review it instead of overwriting")
            backup = directory("/var/backups/lightning-goats", 0o700) / time.strftime("receive-%Y%m%dT%H%M%SZ", time.gmtime())
            backup.mkdir(mode=0o700)
            paths = [Path("/etc/nginx/conf.d/lightning-goats.com.conf"), Path("/etc/lightning-goats"),
                     Path("/var/lib/lightning-goats"), Path("/usr/local/bin/lightning-goatsd"),
                     Path("/usr/local/bin/lightning-goatsctl")]
            for source in paths:
                if source.exists():
                    destination = backup / source.relative_to("/")
                    destination.parent.mkdir(parents=True, exist_ok=True)
                    if source.is_dir():
                        shutil.copytree(source, destination)
                    else:
                        shutil.copy2(source, destination)
            for name in ("lightning-goatsd", "lightning-goatsctl"):
                install(payload / name, "/usr/local/bin/" + name, 0o755)
            config_root = directory("/etc/lightning-goats")
            config = config_root / "config.receive.toml"
            install(payload / "deploy/config.receive.toml.example", config)
            install(payload / "deploy/systemd/lightning-goats-receive.service", "/etc/systemd/system/lightning-goats-receive.service")
            install(payload / "deploy/systemd/lightning-goats-firewall.service", "/etc/systemd/system/lightning-goats-firewall.service")
            directory("/etc/nftables")
            policy = (payload / "deploy/nftables/lightning-goats-runtime.nft.example").read_text().replace("@UID@", str(runtime.pw_uid))
            write("/etc/nftables/lightning-goats-runtime.nft", policy)
            run("nft", "-c", "-f", "/etc/nftables/lightning-goats-runtime.nft")
            crs_root = directory("/usr/local/share/lightning-goats")
            with tarfile.open(crs_archive) as bundle:
                for member in bundle:
                    path = PurePosixPath(member.name)
                    if path.is_absolute() or ".." in path.parts or not (member.isdir() or member.isfile()):
                        raise ValueError("unsafe CRS archive")
                bundle.extractall(crs_root, filter="data")
            crs = crs_root / ("coreruleset-" + pins["crs_version"])
            web = directory("/var/www/lightning-goats-releases") / args.source
            if web.exists():
                raise ValueError("versioned website already exists; do not overwrite")
            shutil.copytree(payload / "web", web)
            for file in web.rglob("*"):
                file.chmod(0o755 if file.is_dir() else 0o644)
            # Credentials are still required; payments become public only on activation.
            write(web / "site-config.js", "window.LightningGoatsSite = Object.freeze({ paymentsEnabled: true, feedingAvailable: false });\n")
            tmp = directory("/var/lib/nginx/tmp/lightning-goats-waf", 0o700)
            nginx = pwd.getpwnam("nginx")
            os.chown(tmp, nginx.pw_uid, nginx.pw_gid)
            waf = (payload / "deploy/waf/lightning-goats.conf.example").read_text().replace("@CRS@", str(crs)).replace("@TMP@", str(tmp))
            write("/etc/nginx/lightning-goats-waf.conf", waf)
            snippets = directory("/etc/nginx/snippets")
            install(payload / "deploy/nginx/lightning-goats-production.conf.example", snippets / "lightning-goats-production.conf")
            install(payload / "deploy/nginx/lightning-goats-nip05.conf.example", snippets / "lightning-goats-nip05.conf")
            stage = directory("/etc/lightning-goats/nginx-staged", 0o700)
            common = (payload / "deploy/nginx/lightning-goats-http.conf.example").read_text()
            common += '\nlog_format lightning_goats_private "$time_iso8601 $request_method $status $bytes_sent $request_time";\n'
            write(stage / "00-lightning-goats-http.conf", common)
            site = f'''server {{
    listen {args.listen_address}:80;
    server_name lightning-goats.com www.lightning-goats.com;
    access_log /var/log/nginx/lightning-goats-access.log lightning_goats_private;
    error_log /var/log/nginx/lightning-goats-error.log crit;
    location ^~ /.well-known/acme-challenge/ {{ root /var/www/letsencrypt; try_files $uri =404; }}
    location / {{ return 308 https://lightning-goats.com$request_uri; }}
}}
server {{
    listen {args.listen_address}:443 ssl;
    server_name lightning-goats.com www.lightning-goats.com;
    ssl_certificate /etc/letsencrypt/live/lightning-goats.com/fullchain.pem;
    ssl_certificate_key /etc/letsencrypt/live/lightning-goats.com/privkey.pem;
    ssl_protocols TLSv1.2 TLSv1.3;
    server_tokens off;
    if ($host = www.lightning-goats.com) {{ return 308 https://lightning-goats.com$request_uri; }}
    client_header_timeout 10s;
    client_body_timeout 10s;
    client_max_body_size 32k;
    keepalive_timeout 15s;
    proxy_cache off;
    access_log /var/log/nginx/lightning-goats-access.log lightning_goats_private;
    error_log /var/log/nginx/lightning-goats-error.log crit;
    add_header Cache-Control no-store always;
    add_header X-Content-Type-Options nosniff always;
    add_header Referrer-Policy no-referrer always;
    modsecurity on;
    modsecurity_rules_file /etc/nginx/lightning-goats-waf.conf;
    root {web};
    index index.html;
    include /etc/nginx/snippets/lightning-goats-production.conf;
    location ^~ /.well-known/acme-challenge/ {{ root /var/www/letsencrypt; try_files $uri =404; }}
    include /etc/nginx/snippets/lightning-goats-nip05.conf;
    location ~ ^/(leaderboard|extensions)(/|$) {{ return 404; }}
    location ~ /\\. {{ deny all; }}
    location / {{ limit_except GET {{ deny all; }} try_files $uri $uri/ =404; }}
}}
'''
            write(stage / "lightning-goats.com.conf", site)
            # Fedora labels 8787 as JBoss debug by default. nginx's restricted
            # relay boolean permits connections only to appropriate port types.
            run("semanage", "port", "-m", "-t", "http_port_t", "-p", "tcp", "8787")
            # Validate the proposed complete host config without touching active conf.d.
            proposed = temp / "conf.d"
            shutil.copytree("/etc/nginx/conf.d", proposed)
            for file in stage.glob("*.conf"):
                shutil.copyfile(file, proposed / file.name)
            main_config = Path("/etc/nginx/nginx.conf").read_text()
            # Accommodate nginx's aligned whitespace while requiring one exact include.
            import re
            main_config, count = re.subn(r"include\s+/etc/nginx/conf\.d/\*\.conf;", f"include {proposed}/*.conf;", main_config)
            if count != 1:
                raise ValueError("could not bind host nginx configuration for preflight")
            (temp / "nginx.conf").write_text(main_config)
            run("nginx", "-t", "-c", str(temp / "nginx.conf"))
            run("systemd-analyze", "verify", "/etc/systemd/system/lightning-goats-receive.service")
            run("systemd-analyze", "verify", "/etc/systemd/system/lightning-goats-firewall.service")
            run("restorecon", "-R", "/etc/lightning-goats", "/etc/nginx", str(web), "/usr/local/bin/lightning-goatsd", "/usr/local/bin/lightning-goatsctl", str(tmp))
            write(config_root / "RECEIVE-INSTALL.json", json.dumps({"source": args.source, "archive_sha256": args.sha256,
                  "daemon_sha256": digest("/usr/local/bin/lightning-goatsd"), "backup": str(backup)}, indent=2) + "\n")
            print("Inactive install and full nginx/systemd preflight passed. Backup: " + str(backup))
            return
        marker = json.loads(Path("/etc/lightning-goats/RECEIVE-INSTALL.json").read_text())
        if marker["source"] != args.source or digest("/usr/local/bin/lightning-goatsd") != marker["daemon_sha256"]:
            raise ValueError("installed source/binary mismatch")
        for name in ("strike-api-key", "strike-webhook-secret"):
            path = Path("/etc/credstore.encrypted/lightning-goats-" + name)
            if path.is_symlink() or not path.is_file() or path.stat().st_uid != 0 or path.stat().st_mode & 0o077:
                raise ValueError("missing or unsafe encrypted credential: " + name)
        config = Path("/etc/lightning-goats/config.receive.toml").read_text()
        import tomllib
        if tomllib.loads(config)["service"]["mode"] != "shadow":
            raise ValueError("this activation supports payment-only shadow mode")
        run("systemctl", "daemon-reload")
        run("systemctl", "start", "lightning-goats-receive.service")
        opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))
        try:
            for _ in range(40):
                try:
                    with opener.open("http://127.0.0.1:8787/healthz", timeout=1) as response:
                        if json.load(response).get("status") == "ok":
                            break
                except OSError:
                    time.sleep(0.25)
            else:
                raise ValueError("receive service did not become healthy")
            for file in Path("/etc/lightning-goats/nginx-staged").glob("*.conf"):
                install(file, Path("/etc/nginx/conf.d") / file.name)
            run("nginx", "-t")
            run("systemctl", "reload", "nginx")
            run("systemctl", "enable", "lightning-goats-receive.service")
            run("systemctl", "enable", "lightning-goats-firewall.service")
        except Exception:
            # Keep paid state; stop new workers. Do not restore an old database.
            run("systemctl", "stop", "lightning-goats-receive.service")
            # The old ingress points at exploited LNbits. Never restore it
            # automatically. Keep the verified static site and close money paths.
            safe = Path("/etc/lightning-goats/nginx-staged/lightning-goats.com.conf").read_text()
            safe = safe.replace("include /etc/nginx/snippets/lightning-goats-production.conf;",
                'location ~ "^/(api|lnurlp|ws)(/|$)" { return 503; }\n'
                '    location ~ "^/\\\\.well-known/lnurlp(/|$)" { return 503; }')
            write("/etc/nginx/conf.d/lightning-goats.com.conf", safe)
            run("nginx", "-t")
            run("systemctl", "reload", "nginx")
            raise
        print("Strike receive-only service activated; physical dispatch and public Nostr remain disabled.")


if __name__ == "__main__":
    main()
