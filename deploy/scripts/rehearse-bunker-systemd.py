#!/usr/bin/env python3
"""Disposable real-nak systemd rehearsal; synthetic scalars and loopback only."""
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import shutil
import socket
import subprocess
import sys
import tempfile
import time
from urllib.parse import quote

HERE = Path(__file__).resolve().parent
SPEC = importlib.util.spec_from_file_location("sandbox", HERE / "rehearse-systemd.py")
SANDBOX = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(SANDBOX)
DIGEST = "b44b36c792fbc3fb73b7ba3bbc94beda2219826271aa8d5f130f569c3817c3b9"
SIGNER = "79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798"
CLIENT = "c6047f9441ed7d6d3045406e95c07cd85c778e4b8cef3ca7abac09b95c709ee5"


def run(args, **kwargs):
    return subprocess.run(list(map(str, args)), capture_output=True, text=True,
                          timeout=20, env={"PATH": os.defpath}, **kwargs)


def checked(args, **kwargs):
    result = run(args, **kwargs)
    if result.returncode:
        # Never forward child output, even in this synthetic rehearsal.
        raise RuntimeError(f"command failed: {Path(str(args[0])).name}")
    return result.stdout


def wait_for(predicate, message):
    deadline = time.monotonic() + 15
    while time.monotonic() < deadline:
        if predicate():
            return
        time.sleep(0.1)
    raise RuntimeError(message)


def main(nak_path):
    if os.geteuid() != 0:
        raise ValueError("root required for disposable transient services")
    links = json.loads(checked(["ip", "-j", "link"]))
    if len(links) != 1 or links[0]["ifname"] != "lo":
        raise ValueError("requires a fresh loopback-only network namespace")
    nak_path = Path(nak_path).resolve()
    if hashlib.sha256(nak_path.read_bytes()).hexdigest() != DIGEST:
        raise ValueError("pinned nak digest mismatch")
    if not Path("/var/lib/systemd/credential.secret").exists():
        raise ValueError("existing host credential key required; no key creation here")
    checked(["ip", "link", "set", "lo", "up"])
    template = HERE.parent / "systemd/lightning-goats-nostr-bunker.service"
    properties = SANDBOX.service_properties(template)
    with tempfile.TemporaryDirectory(prefix="lg-bunker-rehearsal-", dir="/run") as temp:
        root = Path(temp)
        root.chmod(0o755)
        unit = root.name + ".service"
        nak = root / "nak"
        wrapper = root / "run-nak-bunker"
        shutil.copyfile(nak_path, nak)
        shutil.copyfile(HERE / "run-nak-bunker", wrapper)
        nak.chmod(0o755)
        wrapper.chmod(0o755)
        if Path("/sys/fs/selinux/enforce").exists():
            # Label only disposable copies as their shipped destinations; do not
            # change SELinux policy or weaken any service sandbox constraint.
            for path, destination in [(nak, "/usr/local/bin/nak"),
                                      (wrapper, "/usr/local/libexec/lightning-goats/run-nak-bunker")]:
                label = checked(["matchpathcon", "-n", destination]).strip()
                checked(["chcon", label, path])
        # Public test scalar, not a project secret. Delete plaintext before launch.
        plain, encrypted = root / "plain", root / "encrypted"
        plain.write_text("01\n")
        plain.chmod(0o600)
        checked(["systemd-creds", "encrypt", "--with-key=host", "--name=nostr-key", plain, encrypted])
        plain.unlink()
        encrypted.chmod(0o600)
        with socket.socket() as sock:
            sock.bind(("127.0.0.1", 0))
            port = sock.getsockname()[1]
        relay_url = f"ws://127.0.0.1:{port}"
        envfile = root / "bunker.env"
        envfile.write_text(f"NAK_BIN={nak}\nLG_NOSTR_CLIENT_PUBKEY={CLIENT}\nLG_NOSTR_RELAYS={relay_url}\n")
        envfile.chmod(0o600)
        replacements = {"RuntimeDirectory": root.name + "-runtime",
                        "EnvironmentFile": str(envfile),
                        "LoadCredentialEncrypted": f"nostr-key:{encrypted}"}
        # ExecStart is passed as the transient command. Preserve every sandbox
        # directive and restart policy from the shipped template without filtering.
        args = ["systemd-run", "--quiet", "--no-ask-password", f"--unit={unit}"]
        for key, value in properties:
            if key == "ExecStart":
                if value != "/usr/local/libexec/lightning-goats/run-nak-bunker":
                    raise ValueError("unexpected shipped command")
                continue
            args.append(f"--property={key}={replacements.get(key, value)}")
        args.append(f"--property=NetworkNamespacePath=/proc/{os.getpid()}/ns/net")
        relay = subprocess.Popen([str(nak), "--config-path", str(root / "relay"),
                                  "serve", "--hostname", "127.0.0.1", "--port", str(port)],
                                 env={"PATH": os.defpath}, stdout=subprocess.DEVNULL,
                                 stderr=subprocess.DEVNULL)
        try:
            def ready():
                if relay.poll() is not None:
                    raise RuntimeError("synthetic relay exited")
                try:
                    with socket.create_connection(("127.0.0.1", port), timeout=0.2):
                        return True
                except OSError:
                    return False
            wait_for(ready, "relay not ready")
            checked([*args, "--", wrapper])
            wait_for(lambda: checked(["systemctl", "show", unit, "-p", "ActiveState", "--value"]).strip() == "active",
                     "bunker not active")
            def sign():
                return subprocess.run([str(nak), "--config-path", str(root / "client"), "event"],
                                      input=json.dumps({"kind": 1, "content": "synthetic sandbox acceptance", "tags": []}),
                                      capture_output=True, text=True, timeout=15,
                                      env={"PATH": os.defpath, "NOSTR_CLIENT_KEY": "02",
                                           "NOSTR_SECRET_KEY": f"bunker://{SIGNER}?relay={quote(relay_url, safe='')}"})
            signed = sign()
            if signed.returncode:
                raise RuntimeError("systemd bunker signing failed")
            event = json.loads(signed.stdout)
            assert event["pubkey"] == SIGNER
            assert event["content"] == "synthetic sandbox acceptance"
            checked([nak, "verify"], input=json.dumps(event))
            notes = checked([nak, "--config-path", root / "reader", "req", "--kind", "1", relay_url])
            assert not notes.strip(), "signing published a kind-1 note"
            pid = int(checked(["systemctl", "show", unit, "-p", "MainPID", "--value"]).strip())
            status = dict(line.split(":", 1) for line in Path(f"/proc/{pid}/status").read_text().splitlines() if ":" in line)
            assert int(status["Uid"].split()[0]) != 0
            assert status["NoNewPrivs"].strip() == "1"
            assert int(status["CapEff"].strip(), 16) == 0
            assert os.readlink(f"/proc/{pid}/ns/net") == os.readlink("/proc/self/ns/net")
            # Inspect only the synthetic unit's process output, excluding manager messages.
            journal = checked(["journalctl", f"_SYSTEMD_UNIT={unit}", "-o", "cat", "--no-pager"])
            assert not journal.strip(), "bunker process output reached journal"
            checked(["systemctl", "restart", unit])
            signed_again = sign()
            assert signed_again.returncode == 0, "signing after encrypted-credential restart failed"
            assert json.loads(signed_again.stdout)["pubkey"] == SIGNER
            checked([nak, "verify"], input=signed_again.stdout)
            assert not checked([nak, "--config-path", root / "reader", "req", "--kind", "1", relay_url]).strip()
            journal = checked(["journalctl", f"_SYSTEMD_UNIT={unit}", "-o", "cat", "--no-pager"])
            assert not journal.strip(), "restarted bunker process output reached journal"
            runtime = Path("/run") / replacements["RuntimeDirectory"]
            # nak initializes LMDB caches and a local bunkerconn socket even
            # without --persist. The separate bunker config holds signer keys.
            assert not (runtime / "bunker").exists(), "bunker persisted signer configuration"
            checked(["systemctl", "stop", unit])
            encrypted.write_bytes(b"invalid-encrypted-credential")
            # The same sandbox must reject corruption before the wrapper runs.
            run([*args, "--", wrapper])
            wait_for(lambda: checked(["systemctl", "show", unit, "-p", "ExecMainStatus", "--value"]).strip() == "243",
                     "corrupt credential was not rejected before exec")
            print(json.dumps({"scope": "synthetic systemd signer; no production acceptance",
                              "nak_sha256": DIGEST, "template_sha256": hashlib.sha256(template.read_bytes()).hexdigest(),
                              "signing": "pass", "restart_signing": "pass", "signing_publication_count": 0,
                              "process_journal_bytes": len(journal.encode()), "non_root": True,
                              "no_new_privileges": True, "effective_capabilities": 0,
                              "network": "loopback-only namespace", "encrypted_credential": "host-key decrypted",
                              "corrupt_credential": "rejected before exec, status 243",
                              "persisted_bunker_configuration": False}))
        finally:
            try:
                run(["systemctl", "stop", unit])
                state = checked(["systemctl", "show", unit, "-p", "ActiveState", "--value"]).strip()
                if state not in {"inactive", "failed"}:
                    raise RuntimeError("disposable bunker did not stop")
                run(["systemctl", "reset-failed", unit])
            finally:
                relay.terminate()
                relay.wait(timeout=10)


if __name__ == "__main__":
    main(sys.argv[1])
