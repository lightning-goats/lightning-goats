#!/usr/bin/env python3
"""Replace inspected LNbits proxy locations with 410; preserve other site routes.

Default prints a plan. --apply backs up and validates the complete nginx tree,
then installs only the retired-route edits and reloads nginx. No firewall flush,
DNS, WireGuard, daemon, database or certificate changes.
"""
import argparse
import os
from pathlib import Path
import re
import shutil
import subprocess
import tempfile
import time


def block_end(text, start):
    depth = 0
    quoted = None
    escaped = False
    comment = False
    for i in range(start, len(text)):
        c = text[i]
        if comment:
            if c == "\n": comment = False
            continue
        if escaped:
            escaped = False
            continue
        if c == "\\":
            escaped = True
            continue
        if quoted:
            if c == quoted: quoted = None
            continue
        if c in "\"'": quoted = c
        elif c == "#": comment = True
        elif c == "{": depth += 1
        elif c == "}":
            depth -= 1
            if depth == 0: return i + 1
    raise ValueError("unclosed nginx block")


def retire(text):
    edits = []
    for match in re.finditer(r"(?m)^([ \t]*)location [^\n]+\{[ \t]*$", text):
        if edits and match.start() < edits[-1][1]: continue
        opening = match.end() - len(match.group(0)) + match.group(0).rfind("{")
        end = block_end(text, opening)
        body = text[opening:end]
        if re.search(r"proxy_pass\s+http://10\.8\.0\.6:3002(?:[/;])", body):
            edits.append((match.start(), end, text[match.start():opening+1] + " return 410; }"))
    for start, end, replacement in reversed(edits):
        text = text[:start] + replacement + text[end:]
    text = "\n".join(line for line in text.splitlines()
        if not (line.lstrip().startswith("#") and re.search(r"lnbits|10\.8\.0\.6:3002", line, re.I))) + "\n"
    if "10.8.0.6:3002" in text or "include /etc/nginx/snippets/lnbits-private-access.conf" in text:
        raise ValueError("unhandled LNbits reference; no files changed")
    return text, len(edits)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--apply", action="store_true")
    args = parser.parse_args()
    if os.geteuid() != 0: raise ValueError("run with sudo python3")
    root = Path("/etc/nginx")
    edits = {}
    for name in ("lightning-goats.com.conf", "bolverker.com.conf", "lnb.bolverker.com.conf"):
        for folder in ("conf.d", "prepared-domains"):
            file = root / folder / name
            if file.exists():
                rewritten, count = retire(file.read_text())
                if count:
                    edits[file] = rewritten
                    print(str(file) + ": retire " + str(count) + " LNbits proxy locations")
    if not args.apply or not edits: return
    with tempfile.TemporaryDirectory(prefix="lg-retire-ingress-") as temp:
        temp = Path(temp)
        proposed = temp / "conf.d"
        shutil.copytree(root / "conf.d", proposed)
        for file, rewritten in edits.items():
            if file.parent.name == "conf.d": (proposed / file.name).write_text(rewritten)
        config, count = re.subn(r"include\s+/etc/nginx/conf\.d/\*\.conf;", f"include {proposed}/*.conf;", (root / "nginx.conf").read_text())
        if count != 1: raise ValueError("cannot bind full nginx preflight")
        (temp / "nginx.conf").write_text(config)
        check = subprocess.run(["nginx", "-t", "-c", str(temp / "nginx.conf")], capture_output=True, text=True)
        if check.returncode: raise ValueError("proposed nginx configuration rejected; no files changed")
        backup = Path("/var/backups/lightning-goats") / time.strftime("retire-lnbits-%Y%m%dT%H%M%SZ", time.gmtime())
        backup.mkdir(parents=True, mode=0o700)
        backup.parent.chmod(0o700)
        for file, rewritten in edits.items():
            target = backup / file.relative_to("/")
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(file, target)
            with tempfile.NamedTemporaryFile(mode="w", dir=file.parent, delete=False) as output:
                output.write(rewritten); os.fchmod(output.fileno(), 0o644)
                staged = Path(output.name)
            os.replace(staged, file)
            subprocess.run(["restorecon", str(file)], check=True, capture_output=True)
        # Remove the now-unreferenced ACL snippet; retain it only in the private backup.
        acl = root / "snippets/lnbits-private-access.conf"
        references = any("lnbits-private-access.conf" in p.read_text() for p in (root / "conf.d").glob("*.conf"))
        if acl.exists() and not references:
            shutil.copy2(acl, backup / "lnbits-private-access.conf")
            acl.unlink()
        subprocess.run(["nginx", "-t"], check=True, capture_output=True)
        subprocess.run(["systemctl", "reload", "nginx"], check=True)
        print("Retired LNbits ingress. Private backup: " + str(backup))


if __name__ == "__main__":
    main()
