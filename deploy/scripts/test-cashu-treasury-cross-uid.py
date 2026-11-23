#!/usr/bin/env python3
"""Qualify synthetic treasury admission/restart under distinct UIDs in a fresh netns.

Requires explicitly selected native test artifacts. No installed users, services,
credentials, provider requests or real financial/physical operations are used.
"""
import argparse
import grp
import hashlib
import json
import os
from pathlib import Path
import pwd
import shutil
import subprocess
import tempfile
import time
import uuid

CORE_TEST = "cashu::treasury_client::cross_uid_fixture::isolated_treasury_core_child"
WALLET_TEST = "treasury_ingress::cross_uid_fixture::isolated_treasury_wallet_child"


def sha256(path):
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--core-test", required=True, type=Path)
    parser.add_argument("--wallet-test", required=True, type=Path)
    args = parser.parse_args()
    if os.geteuid() != 0 or os.readlink("/proc/self/ns/net") == os.readlink("/proc/1/ns/net"):
        raise ValueError("requires root inside a fresh network namespace")
    links = json.loads(subprocess.check_output(["/usr/sbin/ip", "-j", "link", "show"]))
    if [link["ifname"] for link in links] != ["lo"]:
        raise ValueError("fixture namespace must contain only loopback")
    used = {entry.pw_uid for entry in pwd.getpwall()} | {entry.gr_gid for entry in grp.getgrall()}
    used |= {int(value) for value in subprocess.check_output(["ps", "-e", "-o", "uid="], text=True).split()}
    available = [value for value in range(60000, 65000) if value not in used]
    if len(available) < 4:
        raise ValueError("no unused fixture identities")
    core_uid, wallet_uid, rogue_uid, ipc_gid = available[:4]
    with tempfile.TemporaryDirectory(prefix="lg-cashu-treasury-", dir="/tmp") as temporary:
        root = Path(temporary)
        root.chmod(0o711)
        for name, mode in [("wallet", 0o700), ("run", 0o710)]:
            directory = root / name
            directory.mkdir(mode=mode)
            os.chown(directory, wallet_uid, ipc_gid)
        environment = {"PATH": "/usr/sbin:/usr/bin:/sbin:/bin", "LG_CASHU_TREASURY_FIXTURE": str(root)}

        def command(uid, binary, *arguments):
            return ["/usr/bin/setpriv", f"--reuid={uid}", f"--regid={ipc_gid}", "--clear-groups", "--no-new-privs", "--", str(binary), *arguments]

        binaries = []
        digests = {}
        for name, source, uid, test in [("core-test", args.core_test, core_uid, CORE_TEST), ("wallet-test", args.wallet_test, wallet_uid, WALLET_TEST)]:
            source = source.resolve(strict=True)
            if not source.is_file() or source.stat().st_size > 512 * 1024 * 1024:
                raise ValueError("invalid trusted native test artifact")
            binary = root / name
            shutil.copyfile(source, binary)
            binary.chmod(0o555)
            digests[name] = sha256(binary)
            listed = subprocess.run(command(uid, binary, "--list"), env=environment, stdin=subprocess.DEVNULL, capture_output=True, text=True, timeout=15, check=True)
            if f"{test}: test" not in listed.stdout.splitlines():
                raise ValueError("required current fixture is missing")
            binaries.append(binary)
        (root / "bootstrap.json").write_text(json.dumps({"core_uid": core_uid, "ipc_gid": ipc_gid,
            "wallet_instance": str(uuid.uuid4()), "wallet_epoch": str(uuid.uuid4()), "core_epoch": str(uuid.uuid4())}))
        (root / "bootstrap.json").chmod(0o644)
        socket = root / "run/treasury.sock"
        first_ready = None
        for phase in ["initial", "paused"]:
            ready = root / "wallet/ready.json"
            ready.unlink(missing_ok=True)
            (root / "stop").unlink(missing_ok=True)
            env = dict(environment, LG_CASHU_TREASURY_PHASE=phase)
            started = time.time_ns()
            server = subprocess.Popen(command(wallet_uid, binaries[1], WALLET_TEST, "--exact", "--ignored", "--nocapture"), env=env, stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
            try:
                deadline = time.monotonic() + 15
                while not (ready.exists() and socket.exists() and socket.stat().st_mtime_ns >= started):
                    if server.poll() is not None:
                        output, error = server.communicate(timeout=5)
                        raise ValueError("wallet startup failed: " + (output + error)[-8192:])
                    if time.monotonic() >= deadline:
                        raise ValueError("wallet listener did not become ready")
                    time.sleep(0.05)
                body = ready.read_bytes()
                if first_ready is None:
                    first_ready = body
                elif body != first_ready:
                    raise ValueError("restart changed frozen fixture instructions")
                (root / "ready.json").write_bytes(body)
                (root / "ready.json").chmod(0o644)
                clients = [(rogue_uid, "rogue", "TREASURY_ROGUE_PEER_REJECTED")] if phase == "initial" else []
                clients.append((core_uid, phase, f"TREASURY_CORE_REPLAY_VERIFIED_{phase}"))
                for uid, mode, marker in clients:
                    client = subprocess.run(command(uid, binaries[0], CORE_TEST, "--exact", "--ignored", "--nocapture"), env=dict(environment, LG_CASHU_TREASURY_PHASE=mode), stdin=subprocess.DEVNULL, capture_output=True, text=True, timeout=15)
                    if client.returncode or marker not in client.stdout:
                        raise ValueError("core fixture failed: " + (client.stdout + client.stderr)[-8192:])
                (root / "stop").touch()
                output, error = server.communicate(timeout=5)
                if server.returncode or f"TREASURY_WALLET_ONE_RETAINED_ZERO_SPEND_{phase}" not in output:
                    raise ValueError("wallet verification failed: " + (output + error)[-8192:])
            finally:
                if server.poll() is None:
                    server.kill()
                    server.communicate(timeout=5)
        print(json.dumps({"result": "CASHU_TREASURY_CROSS_UID_VERIFIED", "core_uid": core_uid,
            "wallet_uid": wallet_uid, "rogue_uid": rogue_uid, "retained_instructions": 1,
            "quote_requests": 0, "melt_requests": 0, "paused_restart_replay": True,
            "artifacts_sha256": digests}, sort_keys=True))


if __name__ == "__main__":
    main()
