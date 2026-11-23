#!/usr/bin/env python3
"""Synthetic receipt exchange under two temporary numeric UIDs in a fresh net namespace.

Run via sudo unshare --net. Creates only scratch files, no users or services.
Both executable arguments must be the explicitly trusted current native test builds.
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

CORE_TEST = "cashu::receipt_server::cross_uid_fixture::isolated_receipt_server_child"
WALLET_TEST = "receipt_delivery::cross_uid_fixture::isolated_receipt_wallet_child"


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
    if len(available) < 3:
        raise ValueError("no unused numeric fixture identities")
    core_uid, wallet_uid, ipc_gid = available[:3]
    with tempfile.TemporaryDirectory(prefix="lg-cashu-ipc-", dir="/tmp") as temporary:
        root = Path(temporary)
        root.chmod(0o711)
        for name, owner, mode in [("core", core_uid, 0o700), ("wallet", wallet_uid, 0o700), ("run", core_uid, 0o710)]:
            directory = root / name
            directory.mkdir(mode=mode)
            os.chown(directory, owner, ipc_gid)
        environment = {"PATH": "/usr/sbin:/usr/bin:/sbin:/bin", "LG_CASHU_IPC_FIXTURE": str(root)}

        def command(uid, binary, *arguments):
            return ["/usr/bin/setpriv", f"--reuid={uid}", f"--regid={ipc_gid}", "--clear-groups", "--no-new-privs", "--", str(binary), *arguments]

        digests = {}
        binaries = []
        for name, source, uid, test in [("core-test", args.core_test, core_uid, CORE_TEST), ("wallet-test", args.wallet_test, wallet_uid, WALLET_TEST)]:
            source = source.resolve(strict=True)
            if not source.is_file() or source.stat().st_size > 512 * 1024 * 1024:
                raise ValueError("invalid trusted native test artifact")
            binary = root / name
            shutil.copyfile(source, binary)
            binary.chmod(0o555)
            digest = hashlib.sha256()
            with binary.open("rb") as stream:
                for chunk in iter(lambda: stream.read(1024 * 1024), b""):
                    digest.update(chunk)
            digests[name] = digest.hexdigest()
            listed = subprocess.run(command(uid, binary, "--list"), env=environment, stdin=subprocess.DEVNULL, capture_output=True, text=True, timeout=15, check=True)
            if f"{test}: test" not in listed.stdout.splitlines():
                raise ValueError("required current native fixture is missing")
            binaries.append(binary)
        socket = root / "run" / "receipts.sock"
        (root / "core.json").write_text(json.dumps({
            "socket": str(socket), "ipc_gid": ipc_gid, "wallet_uid": wallet_uid,
            "wallet_instance": str(uuid.uuid4()), "wallet_epoch": str(uuid.uuid4()), "core_epoch": str(uuid.uuid4()),
            "import_enabled": True, "receipts_per_24h": 10, "credit_sats_per_24h": 1000, "maximum_custody_sats": 1000,
            "approved": [{"mint_id": "fixture", "policy_version": "v1", "maximum_gross_sats": 100, "maximum_net_sats": 100}],
            "deadline_seconds": 3, "connections_per_second": 64,
        }))
        (root / "core.json").chmod(0o644)
        server = subprocess.Popen(command(core_uid, binaries[0], CORE_TEST, "--exact", "--ignored", "--nocapture"), env=environment, stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
        try:
            deadline = time.monotonic() + 15
            ready = root / "core" / "ready.json"
            while not (ready.exists() and socket.exists()):
                if server.poll() is not None or time.monotonic() >= deadline:
                    raise ValueError("isolated core fixture did not become ready")
                time.sleep(0.05)
            (root / "client.json").write_bytes(ready.read_bytes())
            (root / "client.json").chmod(0o644)
            client = subprocess.run(command(wallet_uid, binaries[1], WALLET_TEST, "--exact", "--ignored", "--nocapture"), env=environment, stdin=subprocess.DEVNULL, capture_output=True, text=True, timeout=15)
            if client.returncode or "CASHU_WALLET_CROSS_UID_LOST_ACK_REPLAY_VERIFIED" not in client.stdout:
                raise ValueError("wallet fixture failed: " + (client.stdout + client.stderr)[-8192:])
            (root / "stop").touch()
            output, error = server.communicate(timeout=5)
            if server.returncode or "CASHU_CORE_CROSS_UID_ONE_CREDIT_VERIFIED" not in output:
                raise ValueError("core fixture failed: " + (output + error)[-8192:])
            print(json.dumps({"result": "CASHU_CROSS_UID_RECEIPT_VERIFIED", "core_uid": core_uid, "wallet_uid": wallet_uid, "credited_sats": 7, "payment_events": 1, "artifacts_sha256": digests}, sort_keys=True))
        finally:
            if server.poll() is None:
                server.kill()
                server.communicate(timeout=5)


if __name__ == "__main__":
    main()
